mod event;
mod handlers;
mod relate;
mod session;

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use chrono::Utc;
use shabka_core::assess::{self, AssessConfig};
use shabka_core::config::{self, ShabkaConfig};
use shabka_core::embedding::EmbeddingService;
use shabka_core::model::{Memory, MemorySource};
use shabka_core::sharing;
use shabka_core::storage::{HelixStorage, StorageBackend};
use tracing::Level;

use crate::event::{CaptureIntent, HookEvent};
use crate::session::{BufferedEvent, CompressedMemory, SessionBuffer};

/// Derive a project ID from the working directory.
/// Uses the directory basename, e.g. "/home/user/projects/shabka" → "shabka".
fn derive_project_id(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Entry point for the shabka-hooks binary.
///
/// Reads a Claude Code hook event from stdin, classifies it,
/// and saves interesting events as memories in HelixDB.
///
/// CRITICAL: Always exits 0. A non-zero exit could block Claude Code operations.
fn main() -> ExitCode {
    // Set up stderr logging (hooks must not write to stdout)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(Level::WARN)
        .compact()
        .init();

    if let Err(e) = run() {
        tracing::warn!("shabka-hooks: {e:#}");
    }

    ExitCode::SUCCESS
}

fn run() -> anyhow::Result<()> {
    // Read stdin
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    // Parse event (bail silently on malformed input)
    let event: HookEvent = match serde_json::from_str(&input) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!("failed to parse hook event: {e}");
            return Ok(());
        }
    };

    // Load config
    let cwd = Path::new(&event.cwd);
    let config = ShabkaConfig::load(Some(cwd)).unwrap_or_else(|_| ShabkaConfig::default_config());

    // Check if capture is enabled
    if !config.capture.enabled {
        tracing::debug!("capture disabled, skipping");
        return Ok(());
    }

    // Dimension mismatch guard — prevent saving with incompatible embeddings
    if let Err(msg) = config::check_dimensions(&config.embedding) {
        tracing::warn!("shabka-hooks: {msg}");
        return Ok(());
    }

    let session_compression = config.capture.session_compression;

    // Handle Stop event separately — it triggers session compression
    if event.hook_event_name == "Stop" {
        return handle_stop(&event, &config);
    }

    // Classify event
    let intent = handlers::classify(&event, session_compression);

    match intent {
        CaptureIntent::Skip { reason } => {
            tracing::debug!("skipping: {reason}");
            Ok(())
        }
        CaptureIntent::Buffer {
            kind,
            title,
            content,
            importance,
            tags,
            file_path,
            event_type,
        } => {
            // Write to session buffer for later compression
            let buffer = SessionBuffer::new(&event.session_id);
            let buffered = BufferedEvent {
                timestamp: Utc::now().to_rfc3339(),
                kind,
                title,
                content,
                importance,
                tags,
                file_path,
                event_type,
            };
            buffer.append(&buffered)?;
            tracing::debug!("buffered event for session {}", event.session_id);
            Ok(())
        }
        CaptureIntent::Save {
            kind,
            title,
            content,
            importance,
            tags,
        } => {
            // Check importance threshold
            if importance < config.capture.min_importance {
                tracing::debug!(
                    "importance {importance} below threshold {}, skipping",
                    config.capture.min_importance
                );
                return Ok(());
            }
            save_memory_immediate(&event, &config, kind, title, content, importance, tags)
        }
    }
}

/// Handle the Stop event: compress buffered events and save compressed memories.
fn handle_stop(event: &HookEvent, config: &ShabkaConfig) -> anyhow::Result<()> {
    let buffer = SessionBuffer::new(&event.session_id);
    let events = buffer.read_all()?;

    // Also compress any stale buffers from previous sessions
    let stale_buffers = session::find_stale_buffers(std::time::Duration::from_secs(2 * 60 * 60));

    if events.is_empty() && stale_buffers.is_empty() {
        tracing::debug!("no buffered events, skipping stop handler");
        return Ok(());
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        // Compress current session
        if !events.is_empty() {
            let memories = compress_events(&events, config).await;
            save_compressed_memories(&memories, event, config).await?;
            buffer.delete()?;
            tracing::info!(
                "compressed {} events into {} memories for session {}",
                events.len(),
                memories.len(),
                event.session_id,
            );
        }

        // Compress stale buffers
        for stale_path in &stale_buffers {
            let stale_buf = SessionBuffer {
                path: stale_path.clone(),
            };
            match stale_buf.read_all() {
                Ok(stale_events) if !stale_events.is_empty() => {
                    let memories = compress_events(&stale_events, config).await;
                    save_compressed_memories(&memories, event, config).await?;
                    stale_buf.delete()?;
                    tracing::info!(
                        "compressed {} stale events from {:?}",
                        stale_events.len(),
                        stale_path
                    );
                }
                _ => {
                    // Empty or unreadable — just clean up
                    stale_buf.delete().ok();
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

/// Compress events — try LLM first, fall back to heuristic.
async fn compress_events(events: &[BufferedEvent], config: &ShabkaConfig) -> Vec<CompressedMemory> {
    // Try LLM compression if enabled
    if config.llm.enabled {
        match shabka_core::llm::LlmService::from_config(&config.llm) {
            Ok(llm) => match session::compress_with_llm(events, &llm).await {
                Ok(memories) => return memories,
                Err(e) => {
                    tracing::warn!("LLM compression failed, falling back to heuristic: {e}");
                }
            },
            Err(e) => {
                tracing::warn!("failed to create LLM service: {e}");
            }
        }
    }

    // Heuristic fallback
    session::compress_heuristic(events)
}

/// Check a new memory for quality issues and log warnings.
fn log_quality_warnings(memory: &Memory) {
    let issues = assess::check_new_memory(memory, &AssessConfig::default());
    if !issues.is_empty() {
        let labels: Vec<&str> = issues.iter().map(|i| i.label()).collect();
        tracing::info!(
            "quality warnings for '{}': {}",
            memory.title,
            labels.join(", ")
        );
    }
}

/// Save a list of compressed memories to HelixDB.
async fn save_compressed_memories(
    memories: &[CompressedMemory],
    event: &HookEvent,
    config: &ShabkaConfig,
) -> anyhow::Result<()> {
    if memories.is_empty() {
        return Ok(());
    }

    let embedding_service = EmbeddingService::from_config(&config.embedding)?;
    let storage = HelixStorage::new(
        Some(&config.helix.url),
        Some(config.helix.port),
        config.helix.api_key.as_deref(),
    );

    let llm_service = if config.llm.enabled && config.graph.dedup_llm {
        shabka_core::llm::LlmService::from_config(&config.llm).ok()
    } else {
        None
    };

    let user_id = config::resolve_user_id(&config.sharing);
    let privacy = sharing::parse_default_privacy(&config.privacy);

    // Create LLM service for auto-tagging if enabled
    let auto_tag_llm = if config.capture.auto_tag && config.llm.enabled {
        shabka_core::llm::LlmService::from_config(&config.llm).ok()
    } else {
        None
    };

    for compressed in memories {
        let mut memory = Memory::new(
            compressed.title.clone(),
            compressed.content.clone(),
            compressed.kind,
            user_id.clone(),
        )
        .with_source(MemorySource::AutoCapture {
            hook: "SessionCompression".to_string(),
        })
        .with_tags(compressed.tags.clone())
        .with_importance(compressed.importance)
        .with_privacy(privacy)
        .with_project(derive_project_id(&event.cwd));

        // Auto-tag with LLM if enabled
        if let Some(ref llm) = auto_tag_llm {
            if let Some(result) = shabka_core::auto_tag::auto_tag(&memory, llm).await {
                // Keep system tags, add LLM-suggested tags
                let mut tags = memory.tags.clone();
                for tag in result.tags {
                    if !tags.contains(&tag) {
                        tags.push(tag);
                    }
                }
                memory.tags = tags;
                memory.importance = result.importance;
                tracing::debug!(
                    "auto-tagged '{}': {:?} importance={}",
                    memory.title,
                    memory.tags,
                    memory.importance
                );
            }
        }

        log_quality_warnings(&memory);

        let embedding_text = memory.embedding_text();
        let embedding = match embedding_service.embed(&embedding_text).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("embedding failed for '{}': {e}", memory.title);
                continue;
            }
        };

        // Dedup check
        let dedup_decision = shabka_core::dedup::check_duplicate(
            &storage,
            &embedding,
            &config.graph,
            None,
            llm_service.as_ref(),
            &memory.title,
            &memory.content,
        )
        .await;

        match dedup_decision {
            shabka_core::dedup::DedupDecision::Skip {
                existing_title,
                similarity,
                ..
            } => {
                tracing::info!(
                    "dedup skip ({similarity:.2}): '{}' matches '{existing_title}'",
                    memory.title,
                );
                continue;
            }
            shabka_core::dedup::DedupDecision::Supersede {
                existing_id,
                existing_title,
                similarity,
            } => {
                tracing::info!(
                    "dedup supersede ({similarity:.2}): '{}' supersedes '{existing_title}'",
                    memory.title,
                );
                let _ = storage
                    .update_memory(
                        existing_id,
                        &shabka_core::model::UpdateMemoryInput {
                            status: Some(shabka_core::model::MemoryStatus::Superseded),
                            ..Default::default()
                        },
                    )
                    .await;
                let _ = storage
                    .add_relation(&shabka_core::model::MemoryRelation {
                        source_id: memory.id,
                        target_id: existing_id,
                        relation_type: shabka_core::model::RelationType::Supersedes,
                        strength: similarity,
                    })
                    .await;
            }
            shabka_core::dedup::DedupDecision::Update {
                existing_id,
                existing_title,
                merged_content,
                merged_title,
                similarity,
            } => {
                tracing::info!(
                    "dedup merge ({similarity:.2}): new info merged into '{existing_title}' ({existing_id})",
                );
                let _ = storage
                    .update_memory(
                        existing_id,
                        &shabka_core::model::UpdateMemoryInput {
                            title: Some(merged_title),
                            content: Some(merged_content),
                            ..Default::default()
                        },
                    )
                    .await;
                continue;
            }
            shabka_core::dedup::DedupDecision::Contradict {
                existing_id,
                existing_title,
                similarity,
                reason,
            } => {
                tracing::info!(
                    "dedup contradict ({similarity:.2}): '{}' contradicts '{existing_title}': {reason}",
                    memory.title,
                );
                // Save the new memory and link contradiction, then continue
                // (memory is already saved — don't fall through to the second save)
                if let Err(e) = storage.save_memory(&memory, Some(&embedding)).await {
                    tracing::warn!(
                        "failed to save contradicting memory '{}': {e}",
                        memory.title
                    );
                    continue;
                }
                let _ = storage
                    .add_relation(&shabka_core::model::MemoryRelation {
                        source_id: memory.id,
                        target_id: existing_id,
                        relation_type: shabka_core::model::RelationType::Contradicts,
                        strength: similarity,
                    })
                    .await;
                shabka_core::graph::semantic_auto_relate(
                    &storage, memory.id, &embedding, None, None,
                )
                .await;
                continue;
            }
            shabka_core::dedup::DedupDecision::Add => {}
        }

        if let Err(e) = storage.save_memory(&memory, Some(&embedding)).await {
            tracing::warn!("failed to save compressed memory '{}': {e}", memory.title);
            continue;
        }

        tracing::info!(
            "saved compressed {} memory: {} (importance: {})",
            memory.kind,
            memory.title,
            memory.importance,
        );

        // Semantic auto-relate
        shabka_core::graph::semantic_auto_relate(&storage, memory.id, &embedding, None, None).await;
    }

    Ok(())
}

/// Save a single memory immediately (legacy path when session_compression is off).
fn save_memory_immediate(
    event: &HookEvent,
    config: &ShabkaConfig,
    kind: shabka_core::model::MemoryKind,
    title: String,
    content: String,
    importance: f32,
    tags: Vec<String>,
) -> anyhow::Result<()> {
    let user_id = config::resolve_user_id(&config.sharing);
    let privacy = sharing::parse_default_privacy(&config.privacy);
    let mut memory = Memory::new(title, content, kind, user_id)
        .with_source(MemorySource::AutoCapture {
            hook: event.hook_event_name.clone(),
        })
        .with_tags(tags)
        .with_importance(importance)
        .with_privacy(privacy)
        .with_project(derive_project_id(&event.cwd));

    log_quality_warnings(&memory);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        // Auto-tag with LLM if enabled
        if config.capture.auto_tag && config.llm.enabled {
            if let Ok(llm) = shabka_core::llm::LlmService::from_config(&config.llm) {
                if let Some(result) = shabka_core::auto_tag::auto_tag(&memory, &llm).await {
                    let mut tags = memory.tags.clone();
                    for tag in result.tags {
                        if !tags.contains(&tag) {
                            tags.push(tag);
                        }
                    }
                    memory.tags = tags;
                    memory.importance = result.importance;
                }
            }
        }

        let embedding_service = EmbeddingService::from_config(&config.embedding)?;
        let storage = HelixStorage::new(
            Some(&config.helix.url),
            Some(config.helix.port),
            config.helix.api_key.as_deref(),
        );

        let llm_service = if config.llm.enabled && config.graph.dedup_llm {
            shabka_core::llm::LlmService::from_config(&config.llm).ok()
        } else {
            None
        };

        let embedding_text = memory.embedding_text();
        let embedding = embedding_service.embed(&embedding_text).await?;

        // Dedup check
        let dedup_decision = shabka_core::dedup::check_duplicate(
            &storage,
            &embedding,
            &config.graph,
            None,
            llm_service.as_ref(),
            &memory.title,
            &memory.content,
        )
        .await;

        match dedup_decision {
            shabka_core::dedup::DedupDecision::Skip {
                existing_id,
                existing_title,
                similarity,
            } => {
                tracing::info!(
                    "dedup skip ({similarity:.2}): '{}' matches existing '{existing_title}' ({existing_id})",
                    memory.title,
                );
                return Ok(());
            }
            shabka_core::dedup::DedupDecision::Supersede {
                existing_id,
                existing_title,
                similarity,
            } => {
                tracing::info!(
                    "dedup supersede ({similarity:.2}): '{}' supersedes '{existing_title}' ({existing_id})",
                    memory.title,
                );
                let _ = storage
                    .update_memory(
                        existing_id,
                        &shabka_core::model::UpdateMemoryInput {
                            status: Some(shabka_core::model::MemoryStatus::Superseded),
                            ..Default::default()
                        },
                    )
                    .await;
                let _ = storage
                    .add_relation(&shabka_core::model::MemoryRelation {
                        source_id: memory.id,
                        target_id: existing_id,
                        relation_type: shabka_core::model::RelationType::Supersedes,
                        strength: similarity,
                    })
                    .await;
            }
            shabka_core::dedup::DedupDecision::Update {
                existing_id,
                existing_title,
                merged_content,
                merged_title,
                similarity,
            } => {
                tracing::info!(
                    "dedup merge ({similarity:.2}): new info merged into '{existing_title}' ({existing_id})",
                );
                let _ = storage
                    .update_memory(
                        existing_id,
                        &shabka_core::model::UpdateMemoryInput {
                            title: Some(merged_title),
                            content: Some(merged_content),
                            ..Default::default()
                        },
                    )
                    .await;
                return Ok(());
            }
            shabka_core::dedup::DedupDecision::Contradict {
                existing_id,
                existing_title,
                similarity,
                reason,
            } => {
                tracing::info!(
                    "dedup contradict ({similarity:.2}): '{}' contradicts '{existing_title}': {reason}",
                    memory.title,
                );
                storage.save_memory(&memory, Some(&embedding)).await?;
                let _ = storage
                    .add_relation(&shabka_core::model::MemoryRelation {
                        source_id: memory.id,
                        target_id: existing_id,
                        relation_type: shabka_core::model::RelationType::Contradicts,
                        strength: similarity,
                    })
                    .await;
                shabka_core::graph::semantic_auto_relate(&storage, memory.id, &embedding, None, None).await;
                return Ok(());
            }
            shabka_core::dedup::DedupDecision::Add => {}
        }

        storage.save_memory(&memory, Some(&embedding)).await?;

        tracing::info!(
            "captured {} memory: {} (importance: {importance})",
            memory.kind,
            memory.title,
        );

        // Auto-create relations
        relate::auto_relate(&storage, &memory, &event.session_id).await;
        shabka_core::graph::semantic_auto_relate(&storage, memory.id, &embedding, None, None).await;

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
