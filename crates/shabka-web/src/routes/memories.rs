use std::sync::Arc;
use std::time::Duration;

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use chrono::Utc;
use serde::Deserialize;
use shabka_core::dedup::{self, DedupDecision};
use shabka_core::history::{EventAction, MemoryEvent};
use shabka_core::model::*;
use shabka_core::storage::StorageBackend;
use uuid::Uuid;

use shabka_core::config::EmbeddingState;

use crate::error::AppError;
use crate::AppState;

/// Returns the stale threshold from config (or default 90).
fn stale_days(config: &shabka_core::config::ShabkaConfig) -> i64 {
    config.graph.stale_days as i64
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_memories))
        .route("/memories/new", get(new_memory_form))
        .route("/memories", post(create_memory))
        .route("/memories/{id}", get(show_memory))
        .route("/memories/{id}/edit", get(edit_memory_form))
        .route("/memories/{id}/update", post(update_memory))
        .route("/memories/{id}/delete", post(delete_memory))
}

// -- Templates --

#[derive(Template)]
#[template(path = "memories/list.html")]
struct MemoryListTemplate {
    memories: Vec<MemoryListEntry>,
    filter_kind: String,
    filter_project: String,
    embedding_provider: String,
    embedding_model: String,
    embedding_dimensions: usize,
    migration_warning: Option<String>,
    page: usize,
    total_pages: usize,
    total_count: usize,
}

struct MemoryListEntry {
    entry: TimelineEntry,
    days_inactive: i64,
    is_stale: bool,
}

#[derive(Template)]
#[template(path = "memories/detail.html")]
struct MemoryDetailTemplate {
    memory: Memory,
    relations: Vec<RelationDisplay>,
    days_inactive: i64,
    is_stale: bool,
    history_events: Vec<MemoryEvent>,
    similar_memories: Vec<SimilarMemoryEntry>,
}

struct SimilarMemoryEntry {
    id: Uuid,
    title: String,
    kind: String,
    score: f32,
}

#[derive(Template)]
#[template(path = "memories/form.html")]
struct MemoryFormTemplate {
    memory: Option<Memory>,
    kind_options: Vec<KindOption>,
}

struct KindOption {
    name: String,
    selected: bool,
}

struct RelationDisplay {
    relation: MemoryRelation,
    target_title: String,
}

// -- Query params --

const PAGE_SIZE: usize = 50;

#[derive(Deserialize)]
pub struct ListParams {
    kind: Option<String>,
    page: Option<usize>,
    project: Option<String>,
}

#[derive(Deserialize)]
pub struct MemoryFormInput {
    title: String,
    content: String,
    kind: String,
    tags: String,
    importance: f32,
    project: Option<String>,
}

// -- Handlers --

async fn list_memories(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<ListParams>,
) -> Result<Html<String>, AppError> {
    // Fetch a large batch for filtering/pagination
    let query = TimelineQuery {
        limit: 10000,
        ..Default::default()
    };
    let mut entries = state.storage.timeline(&query).await?;

    // Filter by privacy
    entries.retain(|e| shabka_core::sharing::is_visible(e.privacy, &e.created_by, &state.user_id));

    // Filter by kind if specified
    let filter_kind = params.kind.unwrap_or_default();
    if !filter_kind.is_empty() {
        if let Ok(k) = filter_kind.parse::<MemoryKind>() {
            entries.retain(|m| m.kind == k);
        }
    }

    // Filter by project if specified
    let filter_project = params.project.unwrap_or_default();
    if !filter_project.is_empty() {
        entries.retain(|e| e.project_id.as_deref() == Some(filter_project.as_str()));
    }

    // Pagination
    let total_count = entries.len();
    let total_pages = if total_count == 0 {
        1
    } else {
        total_count.div_ceil(PAGE_SIZE)
    };
    let page = params.page.unwrap_or(1).max(1).min(total_pages);
    let start = (page - 1) * PAGE_SIZE;
    let page_entries: Vec<TimelineEntry> =
        entries.into_iter().skip(start).take(PAGE_SIZE).collect();

    // Fetch full memories to get accessed_at for staleness
    let ids: Vec<Uuid> = page_entries.iter().map(|e| e.id).collect();
    let full_memories = if ids.is_empty() {
        vec![]
    } else {
        state.storage.get_memories(&ids).await.unwrap_or_default()
    };

    let now = Utc::now();
    let stale_threshold = stale_days(&state.config);
    let memories: Vec<MemoryListEntry> = page_entries
        .into_iter()
        .map(|entry| {
            let days_inactive = full_memories
                .iter()
                .find(|m| m.id == entry.id)
                .map(|m| (now - m.accessed_at).num_days())
                .unwrap_or(0);
            let is_stale = days_inactive >= stale_threshold;
            MemoryListEntry {
                entry,
                days_inactive,
                is_stale,
            }
        })
        .collect();

    let migration_warning =
        EmbeddingState::migration_warning(&state.config.embedding, state.embedding.dimensions());

    let tmpl = MemoryListTemplate {
        memories,
        filter_kind,
        filter_project,
        embedding_provider: state.embedding.provider_name().to_string(),
        embedding_model: state.embedding.model_id().to_string(),
        embedding_dimensions: state.embedding.dimensions(),
        migration_warning,
        page,
        total_pages,
        total_count,
    };
    Ok(Html(tmpl.render()?))
}

async fn show_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Html<String>, AppError> {
    let memory = state.storage.get_memory(id).await?;
    let raw_relations = state.storage.get_relations(id).await?;

    // Fetch titles for related memories
    let related_ids: Vec<Uuid> = raw_relations
        .iter()
        .map(|r| {
            if r.source_id == id {
                r.target_id
            } else {
                r.source_id
            }
        })
        .collect();

    let related_memories = if related_ids.is_empty() {
        vec![]
    } else {
        state.storage.get_memories(&related_ids).await?
    };

    let relations = raw_relations
        .into_iter()
        .map(|r| {
            let other_id = if r.source_id == id {
                r.target_id
            } else {
                r.source_id
            };
            let title = related_memories
                .iter()
                .find(|m| m.id == other_id)
                .map(|m| m.title.clone())
                .unwrap_or_else(|| other_id.to_string());
            RelationDisplay {
                relation: r,
                target_title: title,
            }
        })
        .collect();

    let days_inactive = (Utc::now() - memory.accessed_at).num_days();
    let is_stale = days_inactive >= stale_days(&state.config);

    // Fetch audit history for this memory
    let history_events = state.history.history_for(id);

    // Find similar memories via vector search (with 3s timeout to avoid blocking on slow providers)
    let similar_memories = match tokio::time::timeout(Duration::from_secs(3), async {
        let embedding = state.embedding.embed(&memory.embedding_text()).await?;
        let results = state.storage.vector_search(&embedding, 6).await?;
        Ok::<Vec<SimilarMemoryEntry>, anyhow::Error>(
            results
                .into_iter()
                .filter(|(m, _)| m.id != id)
                .take(5)
                .map(|(m, score)| SimilarMemoryEntry {
                    id: m.id,
                    title: m.title,
                    kind: m.kind.to_string(),
                    score,
                })
                .collect(),
        )
    })
    .await
    {
        Ok(Ok(entries)) => entries,
        _ => vec![],
    };

    let tmpl = MemoryDetailTemplate {
        memory,
        relations,
        days_inactive,
        is_stale,
        history_events,
        similar_memories,
    };
    Ok(Html(tmpl.render()?))
}

async fn new_memory_form() -> Result<Html<String>, AppError> {
    let tmpl = MemoryFormTemplate {
        memory: None,
        kind_options: make_kind_options("observation"),
    };
    Ok(Html(tmpl.render()?))
}

async fn edit_memory_form(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Html<String>, AppError> {
    let memory = state.storage.get_memory(id).await?;
    let selected = memory.kind.to_string();
    let tmpl = MemoryFormTemplate {
        kind_options: make_kind_options(&selected),
        memory: Some(memory),
    };
    Ok(Html(tmpl.render()?))
}

async fn create_memory(
    State(state): State<Arc<AppState>>,
    Form(input): Form<MemoryFormInput>,
) -> Result<Redirect, AppError> {
    let kind: MemoryKind = input.kind.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let tags: Vec<String> = input
        .tags
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    shabka_core::model::validate_create_input(&input.title, &input.content, input.importance)?;

    let privacy = shabka_core::sharing::parse_default_privacy(&state.config.privacy);
    let mut memory = Memory::new(input.title, input.content, kind, state.user_id.clone())
        .with_tags(tags)
        .with_importance(input.importance)
        .with_privacy(privacy);

    if let Some(ref project) = input.project {
        let p = project.trim();
        if !p.is_empty() {
            memory = memory.with_project(p.to_string());
        }
    }

    let embedding_text = memory.embedding_text();
    let embedding = state.embedding.embed(&embedding_text).await?;

    // Smart dedup check
    let llm_ref = state.llm.as_ref();
    let decision = dedup::check_duplicate(
        &state.storage,
        &embedding,
        &state.config.graph,
        None,
        llm_ref,
        &memory.title,
        &memory.content,
    )
    .await;

    match decision {
        DedupDecision::Skip { existing_id, .. } => {
            return Ok(Redirect::to(&format!(
                "/memories/{existing_id}?toast=Near-duplicate%20found%20%E2%80%94%20memory%20not%20saved&toast_type=warning"
            )));
        }
        DedupDecision::Supersede {
            existing_id,
            existing_title,
            similarity,
        } => {
            state.storage.save_memory(&memory, Some(&embedding)).await?;
            let _ = state
                .storage
                .update_memory(
                    existing_id,
                    &UpdateMemoryInput {
                        status: Some(MemoryStatus::Superseded),
                        ..Default::default()
                    },
                )
                .await;
            let relation = MemoryRelation {
                source_id: memory.id,
                target_id: existing_id,
                relation_type: RelationType::Supersedes,
                strength: similarity,
            };
            let _ = state.storage.add_relation(&relation).await;
            state.history.log(
                &MemoryEvent::new(memory.id, EventAction::Created, state.user_id.clone())
                    .with_title(&memory.title),
            );
            state.history.log(
                &MemoryEvent::new(existing_id, EventAction::Superseded, state.user_id.clone())
                    .with_title(&existing_title),
            );
            return Ok(Redirect::to(&format!(
                "/memories/{}?toast=Memory%20created%20(superseded%20existing)&toast_type=info",
                memory.id
            )));
        }
        DedupDecision::Update {
            existing_id,
            merged_content,
            merged_title,
            ..
        } => {
            let _ = state
                .storage
                .update_memory(
                    existing_id,
                    &UpdateMemoryInput {
                        title: Some(merged_title.clone()),
                        content: Some(merged_content),
                        ..Default::default()
                    },
                )
                .await;
            state.history.log(
                &MemoryEvent::new(existing_id, EventAction::Updated, state.user_id.clone())
                    .with_title(&merged_title),
            );
            return Ok(Redirect::to(&format!(
                "/memories/{existing_id}?toast=Memory%20merged%20into%20existing&toast_type=info"
            )));
        }
        DedupDecision::Contradict {
            existing_id,
            similarity,
            ..
        } => {
            state.storage.save_memory(&memory, Some(&embedding)).await?;
            let relation = MemoryRelation {
                source_id: memory.id,
                target_id: existing_id,
                relation_type: RelationType::Contradicts,
                strength: similarity,
            };
            let _ = state.storage.add_relation(&relation).await;
            state.history.log(
                &MemoryEvent::new(memory.id, EventAction::Created, state.user_id.clone())
                    .with_title(&memory.title),
            );
            return Ok(Redirect::to(&format!(
                "/memories/{}?toast=Memory%20created%20(contradicts%20existing)&toast_type=warning",
                memory.id,
            )));
        }
        DedupDecision::Add => {
            state.storage.save_memory(&memory, Some(&embedding)).await?;
            state.history.log(
                &MemoryEvent::new(memory.id, EventAction::Created, state.user_id.clone())
                    .with_title(&memory.title),
            );
        }
    }

    Ok(Redirect::to(&format!(
        "/memories/{}?toast=Memory%20created",
        memory.id
    )))
}

async fn update_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Form(input): Form<MemoryFormInput>,
) -> Result<Redirect, AppError> {
    let kind: MemoryKind = input.kind.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let tags: Vec<String> = input
        .tags
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    shabka_core::model::validate_create_input(&input.title, &input.content, input.importance)?;

    let old_memory = state.storage.get_memory(id).await?;

    let update = UpdateMemoryInput {
        title: Some(input.title),
        content: Some(input.content),
        kind: Some(kind),
        tags: Some(tags),
        importance: Some(input.importance),
        status: None,
        privacy: None,
        verification: None,
    };

    let memory = state.storage.update_memory(id, &update).await?;

    let changes = shabka_core::history::diff_update(&old_memory, &update);
    state.history.log(
        &MemoryEvent::new(id, EventAction::Updated, state.user_id.clone())
            .with_title(&memory.title)
            .with_changes(changes),
    );

    Ok(Redirect::to(&format!(
        "/memories/{}?toast=Memory%20updated",
        id
    )))
}

async fn delete_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Redirect, AppError> {
    let title = state.storage.get_memory(id).await.ok().map(|m| m.title);
    state.storage.delete_memory(id).await?;

    let mut event = MemoryEvent::new(id, EventAction::Deleted, state.user_id.clone());
    if let Some(t) = title {
        event = event.with_title(t);
    }
    state.history.log(&event);

    Ok(Redirect::to("/?toast=Memory%20deleted"))
}

fn make_kind_options(selected: &str) -> Vec<KindOption> {
    [
        "observation",
        "decision",
        "pattern",
        "error",
        "fix",
        "preference",
        "fact",
        "lesson",
        "todo",
    ]
    .iter()
    .map(|&name| KindOption {
        name: name.to_string(),
        selected: name == selected,
    })
    .collect()
}
