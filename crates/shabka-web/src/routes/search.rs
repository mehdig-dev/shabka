use std::collections::HashMap;
use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use chrono::Utc;
use serde::Deserialize;
use shabka_core::model::Memory;
use shabka_core::ranking::{self, RankCandidate, RankingWeights};
use shabka_core::storage::StorageBackend;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/search", get(search))
}

#[derive(Template)]
#[template(path = "search.html")]
struct SearchTemplate {
    query: String,
    project: String,
    results: Vec<SearchResult>,
}

struct SearchResult {
    memory: Memory,
    score: f32,
    days_inactive: i64,
    is_stale: bool,
    relation_count: usize,
}

#[derive(Deserialize)]
pub struct SearchParams {
    q: Option<String>,
    limit: Option<usize>,
    project: Option<String>,
}

async fn search(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<SearchParams>,
) -> Result<Html<String>, AppError> {
    let query = params.q.unwrap_or_default();
    let limit = params.limit.unwrap_or(20);

    let results = if query.is_empty() {
        vec![]
    } else {
        let embedding = state.embedding.embed(&query).await?;
        let mut raw = state.storage.vector_search(&embedding, limit * 3).await?;
        shabka_core::sharing::filter_search_results(&mut raw, &state.user_id);

        // Filter by project
        if let Some(ref p) = params.project {
            raw.retain(|(m, _)| m.project_id.as_deref() == Some(p.as_str()));
        }

        // Get relation counts for ranking
        let memory_ids: Vec<Uuid> = raw.iter().map(|(m, _)| m.id).collect();
        let counts = state
            .storage
            .count_relations(&memory_ids)
            .await
            .unwrap_or_default();
        let count_map: HashMap<Uuid, usize> = counts.into_iter().collect();

        // Build rank candidates with keyword scoring
        let candidates: Vec<RankCandidate> = raw
            .into_iter()
            .map(|(memory, vector_score)| {
                let kw_score = ranking::keyword_score(&query, &memory);
                RankCandidate {
                    relation_count: count_map.get(&memory.id).copied().unwrap_or(0),
                    keyword_score: kw_score,
                    memory,
                    vector_score,
                    contradiction_count: 0,
                }
            })
            .collect();

        let ranked = ranking::rank(candidates, &RankingWeights::default());
        let now = Utc::now();
        let stale_threshold = state.config.graph.stale_days as i64;

        ranked
            .into_iter()
            .take(limit)
            .map(|r| {
                let days_inactive = (now - r.memory.accessed_at).num_days();
                let is_stale = days_inactive >= stale_threshold;
                let relation_count = count_map.get(&r.memory.id).copied().unwrap_or(0);
                SearchResult {
                    memory: r.memory,
                    score: r.score,
                    days_inactive,
                    is_stale,
                    relation_count,
                }
            })
            .collect()
    };

    let project = params.project.unwrap_or_default();
    let tmpl = SearchTemplate {
        query,
        project,
        results,
    };
    Ok(Html(tmpl.render()?))
}
