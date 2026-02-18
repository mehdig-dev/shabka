use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::Json;
use axum::routing::{delete, get, post, put};
use axum::Router;
use serde::{Deserialize, Serialize};
use shabka_core::dedup::{self, DedupDecision};
use shabka_core::graph;
use shabka_core::history::{EventAction, MemoryEvent};
use shabka_core::model::*;
use shabka_core::ranking::{self, RankCandidate, RankingWeights};
use shabka_core::sharing;
use shabka_core::storage::StorageBackend;
use uuid::Uuid;

use crate::error::ApiError;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/memories", post(create_memory))
        .route("/api/v1/memories", get(list_memories))
        .route("/api/v1/memories/{id}", get(get_memory))
        .route("/api/v1/memories/{id}", put(update_memory))
        .route("/api/v1/memories/{id}", delete(delete_memory))
        .route("/api/v1/memories/{id}/relate", post(add_relation))
        .route("/api/v1/memories/{id}/relations", get(get_relations))
        .route("/api/v1/memories/{id}/history", get(get_history))
        .route("/api/v1/search", get(search))
        .route("/api/v1/timeline", get(timeline))
        .route("/api/v1/stats", get(stats))
        .route("/api/v1/memories/bulk/archive", post(bulk_archive))
        .route("/api/v1/memories/bulk/delete", post(bulk_delete))
}

// -- Request/Response types --

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateMemoryRequest {
    pub title: String,
    pub content: String,
    pub kind: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_importance")]
    pub importance: f32,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub related_to: Vec<String>,
    #[serde(default)]
    pub privacy: Option<String>,
}

fn default_importance() -> f32 {
    0.5
}

#[derive(Debug, Deserialize)]
pub struct UpdateMemoryRequest {
    pub title: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub importance: Option<f32>,
    pub status: Option<String>,
    pub privacy: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddRelationRequest {
    pub target_id: String,
    pub relation_type: String,
    #[serde(default = "default_importance")]
    pub strength: f32,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub kind: Option<String>,
    pub status: Option<String>,
    #[serde(default = "default_list_limit")]
    pub limit: usize,
}

fn default_list_limit() -> usize {
    50
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
    pub kind: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    pub tag: Option<String>,
}

fn default_search_limit() -> usize {
    10
}

#[derive(Debug, Deserialize)]
pub struct TimelineParams {
    #[serde(default = "default_list_limit")]
    pub limit: usize,
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BulkIdsRequest {
    pub ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateMemoryResponse {
    pub action: String,
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct MemoryResponse {
    pub memory: Memory,
    pub relations: Vec<MemoryRelation>,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub total_memories: usize,
    pub by_kind: Vec<KindCount>,
    pub by_status: StatusCounts,
    pub total_relations: usize,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub embedding_dimensions: usize,
}

#[derive(Debug, Serialize)]
pub struct KindCount {
    pub kind: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct StatusCounts {
    pub active: usize,
    pub archived: usize,
    pub superseded: usize,
}

#[derive(Debug, Serialize)]
pub struct BulkResult {
    pub processed: usize,
    pub errors: usize,
}

// -- Handlers --

async fn create_memory(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateMemoryRequest>,
) -> Result<Json<CreateMemoryResponse>, ApiError> {
    let kind: MemoryKind = input
        .kind
        .parse()
        .map_err(|e: String| ApiError::bad_request(e))?;

    shabka_core::model::validate_create_input(&input.title, &input.content, input.importance)?;

    let privacy = input
        .privacy
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| sharing::parse_default_privacy(&state.config.privacy));

    let mut memory = Memory::new(input.title, input.content, kind, state.user_id.clone())
        .with_tags(input.tags)
        .with_importance(input.importance)
        .with_privacy(privacy);

    if let Some(scope) = input.scope {
        if scope != "global" {
            memory = memory.with_scope(MemoryScope::Project { id: scope });
        }
    }

    let embedding = state
        .embedding
        .embed(&memory.embedding_text())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Smart dedup
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
        DedupDecision::Skip {
            existing_id,
            existing_title,
            similarity,
        } => Ok(Json(CreateMemoryResponse {
            action: "skipped".to_string(),
            id: existing_id.to_string(),
            title: existing_title,
            superseded_id: None,
            similarity: Some(similarity),
        })),
        DedupDecision::Supersede {
            existing_id,
            existing_title,
            similarity,
        } => {
            state
                .storage
                .save_memory(&memory, Some(&embedding))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

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

            Ok(Json(CreateMemoryResponse {
                action: "superseded".to_string(),
                id: memory.id.to_string(),
                title: memory.title,
                superseded_id: Some(existing_id.to_string()),
                similarity: Some(similarity),
            }))
        }
        DedupDecision::Update {
            existing_id,
            existing_title,
            merged_content,
            merged_title,
            similarity,
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

            Ok(Json(CreateMemoryResponse {
                action: "merged".to_string(),
                id: existing_id.to_string(),
                title: merged_title,
                superseded_id: Some(existing_title),
                similarity: Some(similarity),
            }))
        }
        DedupDecision::Contradict {
            existing_id,
            existing_title: _,
            similarity,
            reason,
        } => {
            state
                .storage
                .save_memory(&memory, Some(&embedding))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

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

            Ok(Json(CreateMemoryResponse {
                action: "contradicted".to_string(),
                id: memory.id.to_string(),
                title: format!("{} (contradicts: {})", memory.title, reason),
                superseded_id: Some(existing_id.to_string()),
                similarity: Some(similarity),
            }))
        }
        DedupDecision::Add => {
            state
                .storage
                .save_memory(&memory, Some(&embedding))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            // Add explicit relations
            for related_id in &input.related_to {
                if let Ok(target_id) = Uuid::parse_str(related_id) {
                    let relation = MemoryRelation {
                        source_id: memory.id,
                        target_id,
                        relation_type: RelationType::Related,
                        strength: 0.5,
                    };
                    let _ = state.storage.add_relation(&relation).await;
                }
            }

            // Auto-relate
            let _ = graph::semantic_auto_relate(
                &state.storage,
                memory.id,
                &embedding,
                Some(state.config.graph.similarity_threshold),
                Some(state.config.graph.max_relations),
            )
            .await;

            state.history.log(
                &MemoryEvent::new(memory.id, EventAction::Created, state.user_id.clone())
                    .with_title(&memory.title),
            );

            Ok(Json(CreateMemoryResponse {
                action: "added".to_string(),
                id: memory.id.to_string(),
                title: memory.title,
                superseded_id: None,
                similarity: None,
            }))
        }
    }
}

async fn list_memories(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<TimelineEntry>>, ApiError> {
    let query = TimelineQuery {
        limit: params.limit,
        ..Default::default()
    };

    let mut entries = state
        .storage
        .timeline(&query)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    entries.retain(|e| sharing::is_visible(e.privacy, &e.created_by, &state.user_id));

    if let Some(ref kind_str) = params.kind {
        if let Ok(k) = kind_str.parse::<MemoryKind>() {
            entries.retain(|e| e.kind == k);
        }
    }

    // Filter by status if provided (requires fetching full memories)
    if let Some(ref status_str) = params.status {
        if let Ok(status) = serde_json::from_str::<MemoryStatus>(&format!("\"{status_str}\"")) {
            let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
            if let Ok(memories) = state.storage.get_memories(&ids).await {
                let matching_ids: std::collections::HashSet<Uuid> = memories
                    .into_iter()
                    .filter(|m| m.status == status)
                    .map(|m| m.id)
                    .collect();
                entries.retain(|e| matching_ids.contains(&e.id));
            }
        }
    }

    Ok(Json(entries))
}

async fn get_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryResponse>, ApiError> {
    let memory = state
        .storage
        .get_memory(id)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?;

    let relations = state.storage.get_relations(id).await.unwrap_or_default();

    Ok(Json(MemoryResponse { memory, relations }))
}

async fn update_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateMemoryRequest>,
) -> Result<Json<Memory>, ApiError> {
    let old_memory = state
        .storage
        .get_memory(id)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?;

    let status = input
        .status
        .map(|s| {
            serde_json::from_str::<MemoryStatus>(&format!("\"{s}\""))
                .map_err(|_| ApiError::bad_request(format!("invalid status: {s}")))
        })
        .transpose()?;

    let privacy = input.privacy.and_then(|s| s.parse().ok());

    let update = UpdateMemoryInput {
        title: input.title,
        content: input.content,
        tags: input.tags,
        importance: input.importance,
        status,
        kind: None,
        privacy,
        verification: None,
    };

    shabka_core::model::validate_update_input(&update)?;

    let memory = state
        .storage
        .update_memory(id, &update)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let changes = shabka_core::history::diff_update(&old_memory, &update);
    state.history.log(
        &MemoryEvent::new(id, EventAction::Updated, state.user_id.clone())
            .with_title(&memory.title)
            .with_changes(changes),
    );

    Ok(Json(memory))
}

async fn delete_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let title = state.storage.get_memory(id).await.ok().map(|m| m.title);

    state
        .storage
        .delete_memory(id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut event = MemoryEvent::new(id, EventAction::Deleted, state.user_id.clone());
    if let Some(t) = title {
        event = event.with_title(t);
    }
    state.history.log(&event);

    Ok(Json(serde_json::json!({ "deleted": id.to_string() })))
}

async fn add_relation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<AddRelationRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let target_id = Uuid::parse_str(&input.target_id)
        .map_err(|e| ApiError::bad_request(format!("invalid target UUID: {e}")))?;
    let relation_type: RelationType = input
        .relation_type
        .parse()
        .map_err(|e: String| ApiError::bad_request(e))?;

    let relation = MemoryRelation {
        source_id: id,
        target_id,
        relation_type,
        strength: input.strength.clamp(0.0, 1.0),
    };

    state
        .storage
        .add_relation(&relation)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "source_id": id.to_string(),
        "target_id": target_id.to_string(),
        "relation_type": relation.relation_type.to_string(),
        "strength": relation.strength,
    })))
}

async fn get_relations(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<MemoryRelation>>, ApiError> {
    let relations = state
        .storage
        .get_relations(id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(relations))
}

async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<shabka_core::history::MemoryEvent>>, ApiError> {
    let events = state.history.history_for(id);
    Ok(Json(events))
}

async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<MemoryIndex>>, ApiError> {
    let embedding = state
        .embedding
        .embed(&params.q)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let fetch_limit = params.limit * 3;
    let mut results = state
        .storage
        .vector_search(&embedding, fetch_limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    sharing::filter_search_results(&mut results, &state.user_id);

    let tag_filter: Vec<String> = params
        .tag
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let filtered: Vec<(Memory, f32)> = results
        .into_iter()
        .filter(|(m, _)| {
            if let Some(ref kind_str) = params.kind {
                if let Ok(k) = kind_str.parse::<MemoryKind>() {
                    if m.kind != k {
                        return false;
                    }
                }
            }
            if !tag_filter.is_empty() && !tag_filter.iter().any(|t| m.tags.contains(t)) {
                return false;
            }
            true
        })
        .collect();

    let memory_ids: Vec<Uuid> = filtered.iter().map(|(m, _)| m.id).collect();
    let counts = state
        .storage
        .count_relations(&memory_ids)
        .await
        .unwrap_or_default();
    let count_map: std::collections::HashMap<Uuid, usize> = counts.into_iter().collect();

    let candidates: Vec<RankCandidate> = filtered
        .into_iter()
        .map(|(memory, vector_score)| {
            let kw_score = ranking::keyword_score(&params.q, &memory);
            RankCandidate {
                relation_count: count_map.get(&memory.id).copied().unwrap_or(0),
                keyword_score: kw_score,
                memory,
                vector_score,
            }
        })
        .collect();

    let ranked = ranking::rank(candidates, &RankingWeights::default());
    let top: Vec<MemoryIndex> = ranked
        .into_iter()
        .take(params.limit)
        .map(|r| MemoryIndex::from((&r.memory, r.score)))
        .collect();

    Ok(Json(top))
}

async fn timeline(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TimelineParams>,
) -> Result<Json<Vec<TimelineEntry>>, ApiError> {
    let query = TimelineQuery {
        limit: params.limit,
        session_id: params.session_id.and_then(|s| Uuid::parse_str(&s).ok()),
        ..Default::default()
    };

    let mut entries = state
        .storage
        .timeline(&query)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    entries.retain(|e| sharing::is_visible(e.privacy, &e.created_by, &state.user_id));

    Ok(Json(entries))
}

async fn stats(State(state): State<Arc<AppState>>) -> Result<Json<StatsResponse>, ApiError> {
    let entries = state
        .storage
        .timeline(&TimelineQuery {
            limit: 10000,
            ..Default::default()
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
    let memories = if ids.is_empty() {
        vec![]
    } else {
        state.storage.get_memories(&ids).await.unwrap_or_default()
    };

    // Count by kind
    let mut kind_counts = std::collections::HashMap::new();
    let mut active = 0usize;
    let mut archived = 0usize;
    let mut superseded = 0usize;

    for m in &memories {
        *kind_counts.entry(m.kind.to_string()).or_insert(0usize) += 1;
        match m.status {
            MemoryStatus::Active => active += 1,
            MemoryStatus::Archived => archived += 1,
            MemoryStatus::Superseded => superseded += 1,
        }
    }

    let mut by_kind: Vec<KindCount> = kind_counts
        .into_iter()
        .map(|(kind, count)| KindCount { kind, count })
        .collect();
    by_kind.sort_by(|a, b| b.count.cmp(&a.count));

    // Count total relations
    let mut total_relations = 0usize;
    for m in &memories {
        if let Ok(rels) = state.storage.get_relations(m.id).await {
            total_relations += rels.len();
        }
    }
    total_relations /= 2; // Each relation is counted twice (from both ends)

    Ok(Json(StatsResponse {
        total_memories: memories.len(),
        by_kind,
        by_status: StatusCounts {
            active,
            archived,
            superseded,
        },
        total_relations,
        embedding_provider: state.embedding.provider_name().to_string(),
        embedding_model: state.embedding.model_id().to_string(),
        embedding_dimensions: state.embedding.dimensions(),
    }))
}

async fn bulk_archive(
    State(state): State<Arc<AppState>>,
    Json(input): Json<BulkIdsRequest>,
) -> Result<Json<BulkResult>, ApiError> {
    let mut processed = 0usize;
    let mut errors = 0usize;

    for id_str in &input.ids {
        let id = match Uuid::parse_str(id_str) {
            Ok(id) => id,
            Err(_) => {
                errors += 1;
                continue;
            }
        };

        let update = UpdateMemoryInput {
            status: Some(MemoryStatus::Archived),
            ..Default::default()
        };

        match state.storage.update_memory(id, &update).await {
            Ok(m) => {
                processed += 1;
                state.history.log(
                    &MemoryEvent::new(id, EventAction::Archived, state.user_id.clone())
                        .with_title(&m.title),
                );
            }
            Err(_) => errors += 1,
        }
    }

    Ok(Json(BulkResult { processed, errors }))
}

async fn bulk_delete(
    State(state): State<Arc<AppState>>,
    Json(input): Json<BulkIdsRequest>,
) -> Result<Json<BulkResult>, ApiError> {
    let mut processed = 0usize;
    let mut errors = 0usize;

    for id_str in &input.ids {
        let id = match Uuid::parse_str(id_str) {
            Ok(id) => id,
            Err(_) => {
                errors += 1;
                continue;
            }
        };

        let title = state.storage.get_memory(id).await.ok().map(|m| m.title);

        match state.storage.delete_memory(id).await {
            Ok(()) => {
                processed += 1;
                let mut event = MemoryEvent::new(id, EventAction::Deleted, state.user_id.clone());
                if let Some(t) = title {
                    event = event.with_title(t);
                }
                state.history.log(&event);
            }
            Err(_) => errors += 1,
        }
    }

    Ok(Json(BulkResult { processed, errors }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_request_serde() {
        let json = r#"{
            "title": "Test",
            "content": "Body",
            "kind": "observation",
            "tags": ["a", "b"]
        }"#;
        let req: CreateMemoryRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.title, "Test");
        assert_eq!(req.tags, vec!["a", "b"]);
        assert!((req.importance - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_stats_response_serde() {
        let stats = StatsResponse {
            total_memories: 42,
            by_kind: vec![KindCount {
                kind: "observation".to_string(),
                count: 20,
            }],
            by_status: StatusCounts {
                active: 30,
                archived: 10,
                superseded: 2,
            },
            total_relations: 15,
            embedding_provider: "hash".to_string(),
            embedding_model: "hash-128d".to_string(),
            embedding_dimensions: 128,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"total_memories\":42"));
    }
}
