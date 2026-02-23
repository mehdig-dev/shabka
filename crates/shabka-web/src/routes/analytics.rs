use std::collections::HashMap;
use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::Utc;
use shabka_core::assess::{self, AssessConfig, AssessmentResult, IssueCounts};
use shabka_core::config::EmbeddingState;
use shabka_core::history::{EventAction, MemoryEvent};
use shabka_core::model::*;
use shabka_core::storage::StorageBackend;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/analytics", get(analytics_page))
        .route("/analytics/archive-stale", post(archive_stale))
}

#[derive(Template)]
#[template(path = "analytics.html")]
struct AnalyticsTemplate {
    total_memories: usize,
    active_count: usize,
    archived_count: usize,
    superseded_count: usize,
    total_relations: usize,
    kind_labels_json: String,
    kind_counts_json: String,
    trend_labels_json: String,
    trend_counts_json: String,
    most_accessed: Vec<AccessedEntry>,
    stale_count: usize,
    embedding_provider: String,
    embedding_model: String,
    embedding_dimensions: usize,
    migration_warning: Option<String>,
    quality_score: u32,
    quality_counts: IssueCounts,
    quality_top_issues: Vec<QualityTopIssue>,
    contradiction_count: usize,
}

struct QualityTopIssue {
    id: Uuid,
    short_id: String,
    title: String,
    labels: String,
}

struct AccessedEntry {
    id: Uuid,
    title: String,
    kind: String,
    importance: f32,
    days_inactive: i64,
}

async fn analytics_page(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    let entries = state
        .storage
        .timeline(&TimelineQuery {
            limit: 10000,
            ..Default::default()
        })
        .await?;

    let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
    let memories = if ids.is_empty() {
        vec![]
    } else {
        state.storage.get_memories(&ids).await.unwrap_or_default()
    };

    // Count by kind
    let mut kind_counts = std::collections::HashMap::new();
    let mut active_count = 0usize;
    let mut archived_count = 0usize;
    let mut superseded_count = 0usize;

    for m in &memories {
        *kind_counts.entry(m.kind.to_string()).or_insert(0usize) += 1;
        match m.status {
            MemoryStatus::Active => active_count += 1,
            MemoryStatus::Archived => archived_count += 1,
            MemoryStatus::Superseded => superseded_count += 1,
        }
    }

    let mut kind_items: Vec<(String, usize)> = kind_counts.into_iter().collect();
    kind_items.sort_by(|a, b| b.1.cmp(&a.1));
    let kind_labels: Vec<String> = kind_items
        .iter()
        .map(|(k, _)| format!("\"{}\"", k))
        .collect();
    let kind_vals: Vec<String> = kind_items.iter().map(|(_, c)| c.to_string()).collect();

    // Creation trend by month (last 12 months)
    let now = Utc::now();
    let mut trend: Vec<(String, usize)> = Vec::new();
    for i in (0..12).rev() {
        let month = now - chrono::Duration::days(i * 30);
        let label = month.format("%Y-%m").to_string();
        let count = memories
            .iter()
            .filter(|m| m.created_at.format("%Y-%m").to_string() == label)
            .count();
        trend.push((label, count));
    }

    let trend_labels: Vec<String> = trend.iter().map(|(l, _)| format!("\"{}\"", l)).collect();
    let trend_vals: Vec<String> = trend.iter().map(|(_, c)| c.to_string()).collect();

    // Count total relations and track per-memory counts + contradiction count
    let mut total_relations = 0usize;
    let mut contradiction_count = 0usize;
    let mut relation_count_map: HashMap<Uuid, usize> = HashMap::new();
    for m in &memories {
        if let Ok(rels) = state.storage.get_relations(m.id).await {
            relation_count_map.insert(m.id, rels.len());
            total_relations += rels.len();
            for r in &rels {
                if r.relation_type == RelationType::Contradicts {
                    contradiction_count += 1;
                }
            }
        }
    }
    total_relations /= 2;
    contradiction_count /= 2; // Each contradiction counted from both sides

    // Stale count
    let stale_threshold = state.config.graph.stale_days as i64;
    let stale_count = memories
        .iter()
        .filter(|m| (now - m.accessed_at).num_days() >= stale_threshold)
        .count();

    // Most recently accessed (top 10)
    let mut sorted = memories.clone();
    sorted.sort_by(|a, b| b.accessed_at.cmp(&a.accessed_at));
    let most_accessed: Vec<AccessedEntry> = sorted
        .into_iter()
        .take(10)
        .map(|m| {
            let days_inactive = (now - m.accessed_at).num_days();
            AccessedEntry {
                id: m.id,
                title: m.title,
                kind: m.kind.to_string(),
                importance: m.importance,
                days_inactive,
            }
        })
        .collect();

    // Quality assessment
    let assess_config = AssessConfig {
        stale_days: state.config.graph.stale_days,
        ..AssessConfig::default()
    };
    let mut quality_results: Vec<AssessmentResult> = memories
        .iter()
        .filter_map(|m| {
            let rel_count = relation_count_map.get(&m.id).copied().unwrap_or(0);
            let issues = assess::analyze_memory(m, &assess_config, rel_count);
            if issues.is_empty() {
                None
            } else {
                Some(AssessmentResult {
                    memory_id: m.id,
                    title: m.title.clone(),
                    issues,
                })
            }
        })
        .collect();
    quality_results.sort_by(|a, b| b.issues.len().cmp(&a.issues.len()));

    let quality_score = assess::quality_score(&quality_results, memories.len());
    let quality_counts = IssueCounts::from_results(&quality_results);
    let quality_top_issues: Vec<QualityTopIssue> = quality_results
        .iter()
        .take(5)
        .map(|r| {
            let labels: Vec<&str> = r.issues.iter().map(|i| i.label()).collect();
            QualityTopIssue {
                id: r.memory_id,
                short_id: r.memory_id.to_string()[..8].to_string(),
                title: r.title.clone(),
                labels: labels.join(", "),
            }
        })
        .collect();

    let migration_warning = EmbeddingState::migration_warning(
        state.embedding.provider_name(),
        state.embedding.model_id(),
        state.embedding.dimensions(),
    );

    let tmpl = AnalyticsTemplate {
        total_memories: memories.len(),
        active_count,
        archived_count,
        superseded_count,
        total_relations,
        kind_labels_json: format!("[{}]", kind_labels.join(",")),
        kind_counts_json: format!("[{}]", kind_vals.join(",")),
        trend_labels_json: format!("[{}]", trend_labels.join(",")),
        trend_counts_json: format!("[{}]", trend_vals.join(",")),
        most_accessed,
        stale_count,
        embedding_provider: state.embedding.provider_name().to_string(),
        embedding_model: state.embedding.model_id().to_string(),
        embedding_dimensions: state.embedding.dimensions(),
        migration_warning,
        quality_score,
        quality_counts,
        quality_top_issues,
        contradiction_count,
    };

    Ok(Html(tmpl.render()?))
}

async fn archive_stale(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let entries = state
        .storage
        .timeline(&TimelineQuery {
            limit: 10000,
            ..Default::default()
        })
        .await?;

    let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
    let memories = if ids.is_empty() {
        vec![]
    } else {
        state.storage.get_memories(&ids).await.unwrap_or_default()
    };

    let now = Utc::now();
    let stale_threshold = state.config.graph.stale_days as i64;

    let stale_memories: Vec<&Memory> = memories
        .iter()
        .filter(|m| {
            m.status == MemoryStatus::Active && (now - m.accessed_at).num_days() >= stale_threshold
        })
        .collect();

    let mut archived = 0usize;
    for m in &stale_memories {
        let input = UpdateMemoryInput {
            status: Some(MemoryStatus::Archived),
            ..Default::default()
        };
        if state.storage.update_memory(m.id, &input).await.is_ok() {
            state.history.log(
                &MemoryEvent::new(m.id, EventAction::Archived, state.user_id.clone())
                    .with_title(&m.title),
            );
            archived += 1;
        }
    }

    let remaining = stale_memories.len() - archived;
    let color = if remaining == 0 {
        "var(--success)"
    } else {
        "var(--warning)"
    };
    let html = format!(
        "<div style=\"font-size:2rem;font-weight:700;color:{color}\">{remaining}</div>\
         <div style=\"color:var(--text-dim);font-size:0.85rem\">Stale</div>\
         <div style=\"font-size:0.75rem;color:var(--success);margin-top:0.25rem\">Archived {archived} memories</div>"
    );

    let mut headers = HeaderMap::new();
    headers.insert("HX-Trigger", "showToast".parse().unwrap());

    Ok((StatusCode::OK, headers, Html(html)).into_response())
}
