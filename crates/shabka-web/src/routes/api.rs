use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
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
        .route(
            "/api/v1/memories/{id}",
            get(get_memory)
                .put(update_memory)
                .patch(update_memory)
                .delete(delete_memory),
        )
        .route("/api/v1/memories/{id}/edit-field", get(edit_field))
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
    pub kind: Option<String>,
    pub importance: Option<f32>,
    pub status: Option<String>,
    pub privacy: Option<String>,
    pub verification: Option<String>,
}

/// Flat form version where tags is a comma-separated string (from HTMX form inputs).
#[derive(Debug, Deserialize)]
struct UpdateMemoryForm {
    title: Option<String>,
    content: Option<String>,
    tags: Option<String>,
    kind: Option<String>,
    importance: Option<f32>,
    status: Option<String>,
    privacy: Option<String>,
    verification: Option<String>,
}

impl From<UpdateMemoryForm> for UpdateMemoryRequest {
    fn from(form: UpdateMemoryForm) -> Self {
        let tags = form.tags.map(|csv| {
            csv.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        });
        Self {
            title: form.title,
            content: form.content,
            tags,
            kind: form.kind,
            importance: form.importance,
            status: form.status,
            privacy: form.privacy,
            verification: form.verification,
        }
    }
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

// -- Helpers --

fn is_htmx(headers: &HeaderMap) -> bool {
    headers.get("hx-request").is_some()
}

const KIND_OPTIONS: &[&str] = &[
    "observation",
    "decision",
    "pattern",
    "error",
    "fix",
    "preference",
    "fact",
    "lesson",
    "todo",
];

/// Render an HTML fragment for inline editing a single field.
fn render_edit_field(id: &Uuid, field: &str, memory: &Memory) -> String {
    let input_style = "background:var(--surface2);border:1px solid var(--accent);\
        color:var(--text);padding:0.35rem 0.6rem;border-radius:4px;\
        font-size:inherit;font-family:inherit;width:100%";

    match field {
        "title" => {
            let val = html_escape(&memory.title);
            format!(
                r#"<input type="text" name="title" value="{val}" style="{input_style};font-size:1.5rem;font-weight:700"
                    hx-patch="/api/v1/memories/{id}" hx-trigger="blur, keyup[key=='Enter']"
                    hx-target="closest h1" hx-swap="outerHTML"
                    hx-include="this" autofocus
                    onfocus="this.select()">"#,
            )
        }
        "tags" => {
            let val = html_escape(&memory.tags.join(", "));
            format!(
                r#"<div class="inline-edit-wrap">
                    <input type="text" name="tags" value="{val}" style="{input_style};font-size:0.85rem"
                        hx-patch="/api/v1/memories/{id}" hx-trigger="blur, keyup[key=='Enter']"
                        hx-target="closest .editable-tags" hx-swap="outerHTML"
                        hx-include="this" autofocus placeholder="tag1, tag2, ..."
                        onfocus="this.select()">
                </div>"#,
            )
        }
        "kind" => {
            let current = memory.kind.to_string();
            let mut options = String::new();
            for kind in KIND_OPTIONS {
                let selected = if *kind == current { " selected" } else { "" };
                options.push_str(&format!(
                    r#"<option value="{kind}"{selected}>{kind}</option>"#
                ));
            }
            format!(
                r#"<select name="kind" style="{input_style};font-size:0.75rem;width:auto"
                    hx-patch="/api/v1/memories/{id}" hx-trigger="change"
                    hx-target="closest .editable-kind" hx-swap="outerHTML"
                    hx-include="this">{options}</select>"#,
            )
        }
        "importance" => {
            let pct = (memory.importance * 100.0) as u32;
            format!(
                r#"<span class="inline-edit-wrap" style="display:inline-flex;align-items:center;gap:0.5rem">
                    <input type="range" name="importance" min="0" max="1" step="0.05" value="{imp}"
                        style="width:100px"
                        hx-patch="/api/v1/memories/{id}" hx-trigger="change"
                        hx-target="closest .editable-importance" hx-swap="outerHTML"
                        hx-include="this"
                        oninput="this.nextElementSibling.textContent=Math.round(this.value*100)+'%'">
                    <span style="font-size:0.85rem;min-width:2.5em">{pct}%</span>
                </span>"#,
                imp = memory.importance,
            )
        }
        "content" => {
            let val = html_escape(&memory.content);
            format!(
                r#"<div style="margin-bottom:1.5rem">
                    <textarea name="content" style="{input_style};min-height:200px;resize:vertical;font-family:'JetBrains Mono','Fira Code',monospace;font-size:0.85rem;line-height:1.7"
                        hx-patch="/api/v1/memories/{id}" hx-trigger="blur"
                        hx-target="closest .editable-content" hx-swap="outerHTML"
                        hx-include="this">{val}</textarea>
                    <div style="margin-top:0.5rem;display:flex;gap:0.5rem">
                        <button class="btn btn-primary" style="font-size:0.8rem"
                            hx-patch="/api/v1/memories/{id}"
                            hx-target="closest .editable-content" hx-swap="outerHTML"
                            hx-include="closest .editable-content"
                            >Save</button>
                        <button class="btn btn-outline" style="font-size:0.8rem"
                            onclick="location.reload()">Cancel</button>
                    </div>
                </div>"#,
            )
        }
        _ => "<span>Unknown field</span>".to_string(),
    }
}

/// Render the display-mode HTML fragment for a field (returned after HTMX PATCH).
fn render_display_field(id: &Uuid, field: &str, memory: &Memory) -> String {
    match field {
        "title" => {
            let val = html_escape(&memory.title);
            format!(
                r#"<h1 hx-get="/api/v1/memories/{id}/edit-field?field=title" hx-trigger="click" hx-target="this" hx-swap="innerHTML" style="cursor:pointer" title="Click to edit">{val}</h1>"#,
            )
        }
        "tags" => {
            if memory.tags.is_empty() {
                format!(
                    r#"<div class="editable-tags" hx-get="/api/v1/memories/{id}/edit-field?field=tags" hx-trigger="click" hx-target="this" hx-swap="innerHTML" style="cursor:pointer;margin-bottom:1rem" title="Click to edit tags"><span style="font-size:0.82rem;color:var(--text-dim)">Click to add tags</span></div>"#,
                )
            } else {
                let tags_html: String = memory
                    .tags
                    .iter()
                    .map(|t| format!(r#"<span class="tag">{}</span>"#, html_escape(t)))
                    .collect::<Vec<_>>()
                    .join("\n    ");
                format!(
                    r#"<div class="editable-tags" hx-get="/api/v1/memories/{id}/edit-field?field=tags" hx-trigger="click" hx-target="this" hx-swap="innerHTML" style="cursor:pointer;margin-bottom:1rem" title="Click to edit tags">
    {tags_html}
</div>"#,
                )
            }
        }
        "kind" => {
            let val = html_escape(&memory.kind.to_string());
            format!(
                r#"<span class="badge badge-kind editable-kind" hx-get="/api/v1/memories/{id}/edit-field?field=kind" hx-trigger="click" hx-target="this" hx-swap="outerHTML" style="cursor:pointer" title="Click to edit">{val}</span>"#,
            )
        }
        "importance" => {
            let pct = memory.importance * 100.0;
            format!(
                r#"<span class="editable-importance" hx-get="/api/v1/memories/{id}/edit-field?field=importance" hx-trigger="click" hx-target="this" hx-swap="innerHTML" style="cursor:pointer" title="Click to edit">Importance: <span class="importance-bar" style="width:80px"><span class="fill" style="width:{pct:.0}%"></span></span> {pct:.0}%</span>"#,
            )
        }
        "content" => {
            let val = html_escape(&memory.content);
            // Note: The script re-renders markdown client-side after the swap.
            format!(
                r##"<div class="editable-content" style="margin-bottom:1.5rem">
    <div class="content-body markdown-rendered" id="memory-content">{val}</div>
    <button class="btn btn-outline" style="font-size:0.75rem;margin-top:0.5rem"
        hx-get="/api/v1/memories/{id}/edit-field?field=content"
        hx-target="closest .editable-content" hx-swap="innerHTML">Edit content</button>
    <script>
    (function(){{
        var el = document.getElementById('memory-content');
        if (el && typeof marked !== 'undefined') {{ el.innerHTML = marked.parse(el.textContent); }}
    }})();
    </script>
</div>"##,
            )
        }
        _ => String::new(),
    }
}

/// Render the verification badge + buttons fragment for HTMX responses.
fn render_verification_fragment(id: &Uuid, memory: &Memory) -> String {
    let verification_class = match memory.verification {
        VerificationStatus::Verified => "verified",
        VerificationStatus::Unverified => "unverified",
        VerificationStatus::Disputed => "disputed",
        VerificationStatus::Outdated => "outdated",
    };
    let badge = format!(
        r#"<span class="badge badge-verification-{verification_class}">{}</span>"#,
        memory.verification,
    );

    format!(
        r#"{badge}
<div class="verify-actions" style="margin-left:auto;display:flex;gap:0.35rem">
    <button hx-put="/api/v1/memories/{id}" hx-headers='{{"Content-Type":"application/json"}}' hx-vals='{{"verification":"verified"}}' hx-target="closest .verify-group" hx-swap="innerHTML" hx-confirm="Mark this memory as verified?" class="btn btn-outline" style="font-size:0.72rem;padding:0.2rem 0.5rem;border-color:#22c55e;color:#22c55e">Verify</button>
    <button hx-put="/api/v1/memories/{id}" hx-headers='{{"Content-Type":"application/json"}}' hx-vals='{{"verification":"disputed"}}' hx-target="closest .verify-group" hx-swap="innerHTML" hx-confirm="Mark this memory as disputed?" class="btn btn-outline" style="font-size:0.72rem;padding:0.2rem 0.5rem;border-color:#f59e0b;color:#f59e0b">Dispute</button>
    <button hx-put="/api/v1/memories/{id}" hx-headers='{{"Content-Type":"application/json"}}' hx-vals='{{"verification":"outdated"}}' hx-target="closest .verify-group" hx-swap="innerHTML" hx-confirm="Mark this memory as outdated?" class="btn btn-outline" style="font-size:0.72rem;padding:0.2rem 0.5rem;border-color:#ef4444;color:#ef4444">Outdated</button>
</div>"#,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// -- Handlers --

#[derive(Debug, Deserialize)]
pub struct EditFieldParams {
    field: String,
}

async fn edit_field(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(params): Query<EditFieldParams>,
) -> Result<Html<String>, ApiError> {
    let memory = state
        .storage
        .get_memory(id)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?;

    let html = render_edit_field(&id, &params.field, &memory);
    Ok(Html(html))
}

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
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let input: UpdateMemoryRequest = if content_type.contains("application/json") {
        serde_json::from_slice(&body)
            .map_err(|e| ApiError::bad_request(format!("invalid JSON: {e}")))?
    } else {
        let form: UpdateMemoryForm = serde_urlencoded::from_bytes(&body)
            .map_err(|e| ApiError::bad_request(format!("invalid form data: {e}")))?;
        form.into()
    };

    // Capture field-presence flags before consuming input fields.
    let has_title = input.title.is_some();
    let has_content = input.content.is_some();
    let has_tags = input.tags.is_some();
    let has_kind = input.kind.is_some();
    let has_importance = input.importance.is_some();
    let has_verification = input.verification.is_some();

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

    let kind = input
        .kind
        .map(|s| s.parse::<MemoryKind>().map_err(ApiError::bad_request))
        .transpose()?;

    let verification = input
        .verification
        .map(|s| {
            s.parse::<VerificationStatus>()
                .map_err(ApiError::bad_request)
        })
        .transpose()?;

    let update = UpdateMemoryInput {
        title: input.title,
        content: input.content,
        tags: input.tags,
        importance: input.importance,
        status,
        kind,
        privacy,
        verification,
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

    if is_htmx(&headers) {
        // Determine which field was updated and return the appropriate display fragment.
        let field = if has_title {
            "title"
        } else if has_content {
            "content"
        } else if has_tags {
            "tags"
        } else if has_kind {
            "kind"
        } else if has_importance {
            "importance"
        } else if has_verification {
            let html = render_verification_fragment(&id, &memory);
            let mut resp_headers = HeaderMap::new();
            resp_headers.insert("hx-trigger", "showToast".parse().unwrap());
            return Ok((StatusCode::OK, resp_headers, Html(html)).into_response());
        } else {
            "title" // fallback
        };

        let html = render_display_field(&id, field, &memory);
        let mut resp_headers = HeaderMap::new();
        resp_headers.insert("hx-trigger", "showToast".parse().unwrap());
        return Ok((StatusCode::OK, resp_headers, Html(html)).into_response());
    }

    Ok(Json(memory).into_response())
}

async fn delete_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
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

    if is_htmx(&headers) {
        let mut resp_headers = HeaderMap::new();
        resp_headers.insert("hx-redirect", "/?toast=Memory%20deleted".parse().unwrap());
        return Ok((StatusCode::OK, resp_headers, Html(String::new())).into_response());
    }

    Ok(Json(serde_json::json!({ "deleted": id.to_string() })).into_response())
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

    let contradiction_counts = state
        .storage
        .count_contradictions(&memory_ids)
        .await
        .unwrap_or_default();
    let contradiction_map: std::collections::HashMap<Uuid, usize> =
        contradiction_counts.into_iter().collect();

    let candidates: Vec<RankCandidate> = filtered
        .into_iter()
        .map(|(memory, vector_score)| {
            let kw_score = ranking::keyword_score(&params.q, &memory);
            RankCandidate {
                relation_count: count_map.get(&memory.id).copied().unwrap_or(0),
                keyword_score: kw_score,
                contradiction_count: contradiction_map.get(&memory.id).copied().unwrap_or(0),
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
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use shabka_core::config::ShabkaConfig;
    use shabka_core::embedding::EmbeddingService;
    use shabka_core::history::HistoryLogger;
    use shabka_core::storage::{SqliteStorage, Storage};
    use tower::ServiceExt;

    fn test_app_state() -> Arc<AppState> {
        let storage = Storage::Sqlite(SqliteStorage::open_in_memory().unwrap());
        let config = ShabkaConfig::default_config();
        let embedding = EmbeddingService::from_config(&config.embedding).unwrap();
        Arc::new(AppState {
            storage,
            embedding,
            config,
            user_id: "test-user".to_string(),
            history: HistoryLogger::new(false),
            llm: None,
        })
    }

    fn test_router() -> axum::Router {
        crate::routes::router().with_state(test_app_state())
    }

    async fn body_json(body: Body) -> serde_json::Value {
        let bytes = body.collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

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

    // ── API integration tests ────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_memories_empty() {
        let app = test_router();
        let req = Request::builder()
            .uri("/api/v1/memories")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_create_and_get_memory() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state);

        // Create
        let create_body = serde_json::json!({
            "title": "Test memory",
            "content": "Some content",
            "kind": "observation"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(create_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        let id = json["id"].as_str().unwrap().to_string();

        // Get
        let req = Request::builder()
            .uri(format!("/api/v1/memories/{id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["memory"]["title"], "Test memory");
    }

    #[tokio::test]
    async fn test_update_memory() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state);

        // Create
        let create_body = serde_json::json!({
            "title": "Original title",
            "content": "Original content",
            "kind": "fact"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(create_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        let id = json["id"].as_str().unwrap().to_string();

        // Update
        let update_body = serde_json::json!({ "title": "Updated title" });
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/api/v1/memories/{id}"))
            .header("content-type", "application/json")
            .body(Body::from(update_body.to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["title"], "Updated title");
    }

    #[tokio::test]
    async fn test_delete_memory() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state);

        // Create
        let create_body = serde_json::json!({
            "title": "To delete",
            "content": "Will be removed",
            "kind": "observation"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(create_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        let id = json["id"].as_str().unwrap().to_string();

        // Delete
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/memories/{id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Get should return 404
        let req = Request::builder()
            .uri(format!("/api/v1/memories/{id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_search() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state);

        // Create a memory first
        let create_body = serde_json::json!({
            "title": "Rust borrowing rules",
            "content": "The borrow checker enforces ownership",
            "kind": "lesson"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(create_body.to_string()))
            .unwrap();
        app.clone().oneshot(req).await.unwrap();

        // Search
        let req = Request::builder()
            .uri("/api/v1/search?q=rust")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert!(json.is_array());
    }

    #[tokio::test]
    async fn test_timeline() {
        let app = test_router();
        let req = Request::builder()
            .uri("/api/v1/timeline")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert!(json.is_array());
    }

    #[tokio::test]
    async fn test_stats() {
        let app = test_router();
        let req = Request::builder()
            .uri("/api/v1/stats")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["total_memories"], 0);
        assert_eq!(json["embedding_provider"], "hash");
    }

    #[tokio::test]
    async fn test_bulk_delete() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state);

        // Create two memories (different content to avoid dedup)
        for (title, content) in &[
            ("Bulk A", "First unique content about topic alpha"),
            ("Bulk B", "Second unique content about topic beta"),
        ] {
            let body = serde_json::json!({
                "title": title,
                "content": content,
                "kind": "fact"
            });
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            app.clone().oneshot(req).await.unwrap();
        }

        // List to get IDs
        let req = Request::builder()
            .uri("/api/v1/memories")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        let ids: Vec<String> = json
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids.len(), 2);

        // Bulk delete
        let bulk_body = serde_json::json!({ "ids": ids });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories/bulk/delete")
            .header("content-type", "application/json")
            .body(Body::from(bulk_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["processed"], 2);
        assert_eq!(json["errors"], 0);

        // Verify empty
        let req = Request::builder()
            .uri("/api/v1/memories")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_bulk_delete_partial() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state);

        // Create 3 memories with distinct content
        let mut ids = Vec::new();
        for (title, content) in &[
            ("Delete X", "First unique delete content about x-ray topic"),
            (
                "Delete Y",
                "Second unique delete content about yellow topic",
            ),
            ("Keep Z", "Third unique delete content about zebra topic"),
        ] {
            let body = serde_json::json!({
                "title": title,
                "content": content,
                "kind": "fact"
            });
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let json = body_json(resp.into_body()).await;
            ids.push(json["id"].as_str().unwrap().to_string());
        }
        assert_eq!(ids.len(), 3);

        // Bulk delete first 2
        let delete_ids = vec![ids[0].clone(), ids[1].clone()];
        let bulk_body = serde_json::json!({ "ids": delete_ids });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories/bulk/delete")
            .header("content-type", "application/json")
            .body(Body::from(bulk_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["processed"], 2);
        assert_eq!(json["errors"], 0);

        // Verify deleted memories return 404
        for deleted_id in &delete_ids {
            let req = Request::builder()
                .uri(format!("/api/v1/memories/{deleted_id}"))
                .body(Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "Memory {} should be deleted",
                deleted_id
            );
        }

        // Verify 3rd memory still exists
        let req = Request::builder()
            .uri(format!("/api/v1/memories/{}", ids[2]))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["memory"]["title"], "Keep Z");
    }

    #[tokio::test]
    async fn test_bulk_delete_invalid_ids() {
        let app = test_router();
        let body = serde_json::json!({ "ids": ["not-a-uuid", "also-bad"] });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories/bulk/delete")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["processed"], 0);
        assert_eq!(json["errors"], 2);
    }

    #[tokio::test]
    async fn test_create_validation_empty_title() {
        let app = test_router();
        let body = serde_json::json!({
            "title": "",
            "content": "content",
            "kind": "fact"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── Page handler tests ────────────────────────────────────────────

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = test_router();
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_not_found_handler() {
        let app = test_router();
        let req = Request::builder()
            .uri("/definitely-not-a-real-route")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_memories_page() {
        let app = test_router();
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_new_memory_form() {
        let app = test_router();
        let req = Request::builder()
            .uri("/memories/new")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_search_page() {
        let app = test_router();
        let req = Request::builder()
            .uri("/search?q=test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_timeline_page() {
        let app = test_router();
        let req = Request::builder()
            .uri("/timeline")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_graph_page() {
        let app = test_router();
        let req = Request::builder()
            .uri("/graph")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_analytics_page() {
        let app = test_router();
        let req = Request::builder()
            .uri("/analytics")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── Page/API tests with data ────────────────────────────────────────

    #[tokio::test]
    async fn test_show_memory_page() {
        let state = test_app_state();
        let mem = shabka_core::model::Memory::new(
            "Test page memory".to_string(),
            "Unique page content for detail view".to_string(),
            shabka_core::model::MemoryKind::Observation,
            "test-user".to_string(),
        );
        let id = mem.id;
        state.storage.save_memory(&mem, None).await.unwrap();

        let app = crate::routes::router().with_state(state);
        let req = Request::builder()
            .uri(format!("/memories/{id}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_graph_data_json() {
        let app = test_router();
        let req = Request::builder()
            .uri("/graph/data")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert!(json["nodes"].is_array());
        assert!(json["edges"].is_array());
    }

    #[tokio::test]
    async fn test_memory_chain_api() {
        let state = test_app_state();
        let mem = shabka_core::model::Memory::new(
            "Chain root".to_string(),
            "Unique chain root content".to_string(),
            shabka_core::model::MemoryKind::Observation,
            "test-user".to_string(),
        );
        let id = mem.id;
        state.storage.save_memory(&mem, None).await.unwrap();

        let app = crate::routes::router().with_state(state);
        let req = Request::builder()
            .uri(format!("/api/memories/{id}/chain"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_get_relations() {
        let state = test_app_state();
        let mem = shabka_core::model::Memory::new(
            "Relations target".to_string(),
            "Unique relations target content".to_string(),
            shabka_core::model::MemoryKind::Fact,
            "test-user".to_string(),
        );
        let id = mem.id;
        state.storage.save_memory(&mem, None).await.unwrap();

        let app = crate::routes::router().with_state(state);
        let req = Request::builder()
            .uri(format!("/api/v1/memories/{id}/relations"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert!(json.is_array());
    }

    #[tokio::test]
    async fn test_api_get_history() {
        let state = test_app_state();
        let mem = shabka_core::model::Memory::new(
            "History target".to_string(),
            "Unique history target content".to_string(),
            shabka_core::model::MemoryKind::Lesson,
            "test-user".to_string(),
        );
        let id = mem.id;
        state.storage.save_memory(&mem, None).await.unwrap();

        let app = crate::routes::router().with_state(state);
        let req = Request::builder()
            .uri(format!("/api/v1/memories/{id}/history"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert!(json.is_array());
    }

    #[tokio::test]
    async fn test_bulk_archive_empty() {
        let app = test_router();
        let body = serde_json::json!({ "ids": [] });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories/bulk/archive")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["processed"], 0);
        assert_eq!(json["errors"], 0);
    }

    #[tokio::test]
    async fn test_bulk_archive_with_memories() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state.clone());

        // Create 3 memories with distinct content to avoid dedup
        let mut ids = Vec::new();
        for (title, content) in &[
            (
                "Archive A",
                "First unique archive content about alpha topic",
            ),
            (
                "Archive B",
                "Second unique archive content about beta topic",
            ),
            (
                "Archive C",
                "Third unique archive content about gamma topic",
            ),
        ] {
            let body = serde_json::json!({
                "title": title,
                "content": content,
                "kind": "observation"
            });
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let json = body_json(resp.into_body()).await;
            ids.push(json["id"].as_str().unwrap().to_string());
        }
        assert_eq!(ids.len(), 3);

        // Bulk archive first 2
        let archive_ids = vec![ids[0].clone(), ids[1].clone()];
        let bulk_body = serde_json::json!({ "ids": archive_ids });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories/bulk/archive")
            .header("content-type", "application/json")
            .body(Body::from(bulk_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["processed"], 2);
        assert_eq!(json["errors"], 0);

        // Verify the 2 archived memories have status "archived"
        for archived_id in &archive_ids {
            let req = Request::builder()
                .uri(format!("/api/v1/memories/{archived_id}"))
                .body(Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let json = body_json(resp.into_body()).await;
            assert_eq!(
                json["memory"]["status"], "archived",
                "Memory {} should be archived",
                archived_id
            );
        }

        // Verify the 3rd memory is still active
        let req = Request::builder()
            .uri(format!("/api/v1/memories/{}", ids[2]))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(
            json["memory"]["status"], "active",
            "Third memory should remain active"
        );
    }

    #[tokio::test]
    async fn test_bulk_archive_invalid_ids() {
        let app = test_router();
        let body = serde_json::json!({ "ids": ["not-a-uuid", "also-invalid"] });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories/bulk/archive")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["processed"], 0);
        assert_eq!(json["errors"], 2);
    }

    // ── Existing relation test ──────────────────────────────────────────

    #[tokio::test]
    async fn test_add_relation() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state);

        // Create two memories
        let mut ids = Vec::new();
        for title in &["Source", "Target"] {
            let body = serde_json::json!({
                "title": title,
                "content": "content",
                "kind": "fact"
            });
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            let json = body_json(resp.into_body()).await;
            ids.push(json["id"].as_str().unwrap().to_string());
        }

        // Add relation
        let rel_body = serde_json::json!({
            "target_id": ids[1],
            "relation_type": "related",
            "strength": 0.7
        });
        let req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/memories/{}/relate", ids[0]))
            .header("content-type", "application/json")
            .body(Body::from(rel_body.to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["source_id"], ids[0]);
        assert_eq!(json["target_id"], ids[1]);
    }

    // ── HTMX / new endpoint tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_search_partial_htmx() {
        let app = test_router();
        let req = Request::builder()
            .uri("/search?q=test")
            .header("hx-request", "true")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let html = String::from_utf8_lossy(&bytes);
        // HTMX partial should NOT contain doctype (it's a fragment)
        assert!(
            !html.contains("<!doctype html>"),
            "Expected partial, got full page"
        );
    }

    #[tokio::test]
    async fn test_search_full_page() {
        let app = test_router();
        let req = Request::builder()
            .uri("/search?q=test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let html = String::from_utf8_lossy(&bytes);
        // Full page should contain doctype
        assert!(html.contains("<!doctype html>"), "Expected full page");
    }

    #[tokio::test]
    async fn test_patch_memory() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state);

        // Create
        let body = serde_json::json!({
            "title": "Patch test memory",
            "content": "Content for patch test",
            "kind": "fact"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        let id = json["id"].as_str().unwrap().to_string();

        // PATCH
        let patch_body = serde_json::json!({ "title": "Patched title" });
        let req = Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1/memories/{id}"))
            .header("content-type", "application/json")
            .body(Body::from(patch_body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["title"], "Patched title");
    }

    #[tokio::test]
    async fn test_edit_field_endpoint() {
        let state = test_app_state();
        let mem = shabka_core::model::Memory::new(
            "Edit field test".to_string(),
            "Content for edit field".to_string(),
            shabka_core::model::MemoryKind::Observation,
            "test-user".to_string(),
        );
        let id = mem.id;
        state.storage.save_memory(&mem, None).await.unwrap();

        let app = crate::routes::router().with_state(state);
        let req = Request::builder()
            .uri(format!("/api/v1/memories/{id}/edit-field?field=title"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let html = String::from_utf8_lossy(&bytes);
        assert!(
            html.contains("<input"),
            "Expected an input element for inline editing"
        );
    }

    #[tokio::test]
    async fn test_archive_stale_endpoint() {
        let app = test_router();
        let req = Request::builder()
            .method("POST")
            .uri("/analytics/archive-stale")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_htmx_delete_redirect() {
        let state = test_app_state();
        let app = crate::routes::router().with_state(state);

        // Create
        let body = serde_json::json!({
            "title": "HTMX delete test",
            "content": "Will be deleted via HTMX",
            "kind": "observation"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/memories")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        let id = json["id"].as_str().unwrap().to_string();

        // DELETE with HX-Request
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/memories/{id}"))
            .header("hx-request", "true")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let redirect = resp
            .headers()
            .get("hx-redirect")
            .expect("Expected hx-redirect header");
        assert!(redirect.to_str().unwrap().contains("toast=Memory"));
    }
}
