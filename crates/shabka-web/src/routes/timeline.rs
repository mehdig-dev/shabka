use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use shabka_core::model::{TimelineEntry, TimelineQuery};
use shabka_core::storage::StorageBackend;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/timeline", get(timeline))
}

#[derive(Template)]
#[template(path = "timeline.html")]
#[allow(dead_code)]
struct TimelineTemplate {
    entries: Vec<TimelineEntry>,
    session_filter: String,
}

#[derive(Deserialize)]
pub struct TimelineParams {
    limit: Option<usize>,
    session_id: Option<Uuid>,
}

async fn timeline(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<TimelineParams>,
) -> Result<Html<String>, AppError> {
    let query = TimelineQuery {
        limit: params.limit.unwrap_or(50),
        session_id: params.session_id,
        ..Default::default()
    };

    let mut entries = state.storage.timeline(&query).await?;
    entries.retain(|e| shabka_core::sharing::is_visible(e.privacy, &e.created_by, &state.user_id));
    let session_filter = params.session_id.map(|s| s.to_string()).unwrap_or_default();

    let tmpl = TimelineTemplate {
        entries,
        session_filter,
    };
    Ok(Html(tmpl.render()?))
}
