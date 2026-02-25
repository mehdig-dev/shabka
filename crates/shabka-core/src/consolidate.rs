//! Memory consolidation — merge clusters of related small memories into comprehensive summaries.
//!
//! Finds groups of similar memories via vector search, then uses an LLM to merge each
//! cluster into a single comprehensive memory. Original memories are superseded.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

use crate::embedding::EmbeddingService;
use crate::error::Result;
use crate::graph;
use crate::history::{EventAction, HistoryLogger, MemoryEvent};
use crate::llm::LlmService;
use crate::model::*;
use crate::storage::StorageBackend;

/// Raw JSON response from the LLM for consolidation.
#[derive(Deserialize, Debug)]
struct ConsolidateLlmResponse {
    title: String,
    content: String,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_importance")]
    importance: f64,
}

fn default_kind() -> String {
    "observation".to_string()
}

fn default_importance() -> f64 {
    0.5
}

/// Configuration for memory consolidation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidateConfig {
    /// Minimum cluster size to trigger consolidation.
    #[serde(default = "default_min_cluster")]
    pub min_cluster_size: usize,
    /// Similarity threshold for grouping memories.
    #[serde(default = "default_sim_threshold")]
    pub similarity_threshold: f32,
    /// Maximum cluster size.
    #[serde(default = "default_max_cluster")]
    pub max_cluster_size: usize,
    /// Minimum age in days before a memory is eligible for consolidation.
    #[serde(default = "default_min_age")]
    pub min_age_days: u64,
    /// Enable automatic consolidation on MCP server startup.
    #[serde(default)]
    pub auto: bool,
    /// How often to run auto-consolidation: "daily", "weekly", or "on_startup".
    #[serde(default = "default_interval")]
    pub interval: String,
}

fn default_interval() -> String {
    "daily".to_string()
}

fn default_min_cluster() -> usize {
    3
}
fn default_sim_threshold() -> f32 {
    0.7
}
fn default_max_cluster() -> usize {
    10
}
fn default_min_age() -> u64 {
    7
}

impl Default for ConsolidateConfig {
    fn default() -> Self {
        Self {
            min_cluster_size: default_min_cluster(),
            similarity_threshold: default_sim_threshold(),
            max_cluster_size: default_max_cluster(),
            min_age_days: default_min_age(),
            auto: false,
            interval: default_interval(),
        }
    }
}

/// Result of a consolidation run.
#[derive(Debug, Clone, Serialize)]
pub struct ConsolidateResult {
    pub clusters_found: usize,
    pub clusters_consolidated: usize,
    pub memories_superseded: usize,
    pub memories_created: usize,
}

/// A consolidated memory produced by the LLM.
#[derive(Debug, Clone)]
pub struct ConsolidatedMemory {
    pub title: String,
    pub content: String,
    pub kind: MemoryKind,
    pub tags: Vec<String>,
    pub importance: f32,
}

/// System prompt for LLM-powered consolidation.
const CONSOLIDATE_SYSTEM_PROMPT: &str = r#"You are a developer knowledge-base consolidator. Given a cluster of related memories, merge them into a single comprehensive memory.

Rules:
- Combine ALL unique technical details from every memory in the cluster
- Use the most specific and accurate title
- Organize content logically (don't just concatenate)
- Preserve all code snippets, error messages, config keys, file paths
- Choose the most appropriate kind for the merged content
- Suggest 3-8 specific, lowercase tags
- Rate importance 0.0-1.0 based on the combined significance

Return ONLY valid JSON (no markdown fences, no extra text):
{"title":"merged title","content":"comprehensive merged content","kind":"observation","tags":["tag1","tag2"],"importance":0.7}"#;

/// Find clusters of similar memories eligible for consolidation.
pub async fn find_clusters(
    storage: &impl StorageBackend,
    embedding_svc: &EmbeddingService,
    config: &ConsolidateConfig,
) -> Vec<Vec<Memory>> {
    let cutoff = Utc::now() - chrono::Duration::days(config.min_age_days as i64);

    // Fetch all active memories
    let entries = match storage
        .timeline(&TimelineQuery {
            limit: 10000,
            ..Default::default()
        })
        .await
    {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
    let all_memories = match storage.get_memories(&ids).await {
        Ok(m) => m,
        Err(_) => return vec![],
    };

    // Only consider active memories old enough
    let eligible: Vec<&Memory> = all_memories
        .iter()
        .filter(|m| m.status == MemoryStatus::Active && m.created_at < cutoff)
        .collect();

    let mut used: HashSet<Uuid> = HashSet::new();
    let mut clusters: Vec<Vec<Memory>> = Vec::new();

    for memory in &eligible {
        if used.contains(&memory.id) {
            continue;
        }

        // Embed this memory and find similar ones
        let embedding = match embedding_svc.embed(&memory.embedding_text()).await {
            Ok(e) => e,
            Err(_) => continue,
        };

        let results = match storage
            .vector_search(&embedding, config.max_cluster_size + 1)
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        let mut cluster: Vec<Memory> = vec![(*memory).clone()];
        for (candidate, score) in results {
            if candidate.id == memory.id || used.contains(&candidate.id) {
                continue;
            }
            if score < config.similarity_threshold {
                continue;
            }
            if candidate.status != MemoryStatus::Active || candidate.created_at >= cutoff {
                continue;
            }
            cluster.push(candidate);
            if cluster.len() >= config.max_cluster_size {
                break;
            }
        }

        if cluster.len() >= config.min_cluster_size {
            for m in &cluster {
                used.insert(m.id);
            }
            clusters.push(cluster);
        }
    }

    clusters
}

/// Use LLM to merge a cluster into a single comprehensive memory.
pub async fn consolidate_cluster(
    cluster: &[Memory],
    llm: &LlmService,
) -> std::result::Result<ConsolidatedMemory, String> {
    let mut prompt = String::from("MEMORIES TO CONSOLIDATE:\n\n");
    for (idx, memory) in cluster.iter().enumerate() {
        prompt.push_str(&format!(
            "--- Memory {} ---\nTitle: {}\nKind: {}\nContent: {}\nTags: {}\n\n",
            idx + 1,
            memory.title,
            memory.kind,
            memory.content,
            memory.tags.join(", "),
        ));
    }
    prompt.push_str("Merge these into a single comprehensive memory.");

    let response: ConsolidateLlmResponse = llm
        .generate_structured(&prompt, Some(CONSOLIDATE_SYSTEM_PROMPT))
        .await
        .map_err(|e| format!("LLM call failed: {e}"))?;

    let kind: MemoryKind = response.kind.parse().unwrap_or(MemoryKind::Observation);
    let tags: Vec<String> = response
        .tags
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect();
    let importance = (response.importance as f32).clamp(0.0, 1.0);

    Ok(ConsolidatedMemory {
        title: response.title,
        content: response.content,
        kind,
        tags,
        importance,
    })
}

/// Run the full consolidation pipeline: find clusters, consolidate, save, supersede.
pub async fn consolidate(
    storage: &impl StorageBackend,
    embedding_svc: &EmbeddingService,
    llm: &LlmService,
    config: &ConsolidateConfig,
    user_id: &str,
    history: &HistoryLogger,
    dry_run: bool,
) -> Result<ConsolidateResult> {
    let clusters = find_clusters(storage, embedding_svc, config).await;
    let clusters_found = clusters.len();
    let mut clusters_consolidated = 0;
    let mut memories_superseded = 0;
    let mut memories_created = 0;

    for cluster in &clusters {
        let consolidated = match consolidate_cluster(cluster, llm).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("failed to consolidate cluster: {e}");
                continue;
            }
        };

        if dry_run {
            clusters_consolidated += 1;
            memories_superseded += cluster.len();
            memories_created += 1;
            continue;
        }

        // Create the consolidated memory
        let new_memory = Memory::new(
            consolidated.title,
            consolidated.content,
            consolidated.kind,
            user_id.to_string(),
        )
        .with_tags(consolidated.tags)
        .with_importance(consolidated.importance)
        .with_source(MemorySource::AutoCapture {
            hook: "Consolidation".to_string(),
        });

        // Embed and save
        let embedding = match embedding_svc.embed(&new_memory.embedding_text()).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("embedding failed for consolidated memory: {e}");
                continue;
            }
        };

        if let Err(e) = storage.save_memory(&new_memory, Some(&embedding)).await {
            tracing::warn!("failed to save consolidated memory: {e}");
            continue;
        }

        history.log(
            &MemoryEvent::new(new_memory.id, EventAction::Created, user_id.to_string())
                .with_title(&new_memory.title),
        );

        // Supersede original memories and create relations
        for original in cluster {
            let _ = storage
                .update_memory(
                    original.id,
                    &UpdateMemoryInput {
                        status: Some(MemoryStatus::Superseded),
                        ..Default::default()
                    },
                )
                .await;

            let relation = MemoryRelation {
                source_id: new_memory.id,
                target_id: original.id,
                relation_type: RelationType::Supersedes,
                strength: 1.0,
            };
            let _ = storage.add_relation(&relation).await;

            history.log(
                &MemoryEvent::new(original.id, EventAction::Superseded, user_id.to_string())
                    .with_title(&original.title),
            );

            memories_superseded += 1;
        }

        // Auto-relate the new memory
        graph::semantic_auto_relate(storage, new_memory.id, &embedding, None, None).await;

        clusters_consolidated += 1;
        memories_created += 1;
    }

    Ok(ConsolidateResult {
        clusters_found,
        clusters_consolidated,
        memories_superseded,
        memories_created,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: deserialize JSON (with optional markdown fences) into ConsolidatedMemory.
    fn parse_response(raw: &str) -> std::result::Result<ConsolidatedMemory, String> {
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let response: ConsolidateLlmResponse =
            serde_json::from_str(cleaned).map_err(|e| format!("invalid JSON: {e}"))?;
        let kind: MemoryKind = response.kind.parse().unwrap_or(MemoryKind::Observation);
        let tags: Vec<String> = response
            .tags
            .into_iter()
            .map(|t| t.to_lowercase())
            .collect();
        let importance = (response.importance as f32).clamp(0.0, 1.0);
        Ok(ConsolidatedMemory {
            title: response.title,
            content: response.content,
            kind,
            tags,
            importance,
        })
    }

    #[test]
    fn test_consolidate_config_defaults() {
        let config = ConsolidateConfig::default();
        assert_eq!(config.min_cluster_size, 3);
        assert!((config.similarity_threshold - 0.7).abs() < f32::EPSILON);
        assert_eq!(config.max_cluster_size, 10);
        assert_eq!(config.min_age_days, 7);
    }

    #[test]
    fn test_parse_consolidated_response_valid() {
        let response = r#"{"title":"Combined knowledge","content":"Full details","kind":"pattern","tags":["rust","config"],"importance":0.8}"#;
        let result = parse_response(response).unwrap();
        assert_eq!(result.title, "Combined knowledge");
        assert_eq!(result.content, "Full details");
        assert_eq!(result.kind, MemoryKind::Pattern);
        assert_eq!(result.tags, vec!["rust", "config"]);
        assert!((result.importance - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_consolidated_response_with_fences() {
        let response = "```json\n{\"title\":\"Test\",\"content\":\"Body\",\"kind\":\"fact\"}\n```";
        let result = parse_response(response).unwrap();
        assert_eq!(result.title, "Test");
        assert_eq!(result.kind, MemoryKind::Fact);
    }

    #[test]
    fn test_parse_consolidated_response_invalid_json() {
        let result = parse_response("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_consolidated_response_missing_title() {
        let response = r#"{"content":"body","kind":"fact"}"#;
        let result = parse_response(response);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_consolidated_response_defaults() {
        let response = r#"{"title":"Test","content":"Body"}"#;
        let result = parse_response(response).unwrap();
        assert_eq!(result.kind, MemoryKind::Observation);
        assert!(result.tags.is_empty());
        assert!((result.importance - 0.5).abs() < f32::EPSILON);
    }
}
