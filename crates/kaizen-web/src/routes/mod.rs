pub mod analytics;
pub mod api;
pub mod graph;
pub mod memories;
pub mod search;
pub mod timeline;

use std::sync::Arc;

use axum::extract::State;
use axum::response::{Html, Json};
use axum::routing::get;
use axum::Router;
use kaizen_core::storage::StorageBackend;

use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health))
        .merge(memories::routes())
        .merge(search::routes())
        .merge(timeline::routes())
        .merge(graph::routes())
        .merge(api::routes())
        .merge(analytics::routes())
        .fallback(not_found)
}

async fn health(
    State(state): State<Arc<AppState>>,
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    use kaizen_core::model::TimelineQuery;
    let db_ok = state
        .storage
        .timeline(&TimelineQuery {
            limit: 1,
            ..Default::default()
        })
        .await
        .is_ok();

    let status = if db_ok {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(serde_json::json!({
            "status": if db_ok { "ok" } else { "degraded" },
            "helix_db": if db_ok { "connected" } else { "unavailable" },
            "embedding_provider": state.embedding.provider_name(),
        })),
    )
}

async fn not_found() -> (axum::http::StatusCode, Html<String>) {
    let body = r#"<!doctype html>
<html><head><title>404 — Kaizen</title>
<style>body{font-family:system-ui;background:#0f0f1a;color:#e0e0e0;display:flex;justify-content:center;align-items:center;height:100vh;margin:0}
.box{text-align:center}
h1{font-size:4rem;color:#6c63ff;margin:0}
p{color:#888;margin:0.5rem 0 1.5rem}
a{color:#6c63ff;text-decoration:none;padding:0.5rem 1rem;border:1px solid #2a2a4a;border-radius:8px}
a:hover{border-color:#6c63ff;background:rgba(108,99,255,0.1)}</style>
</head><body><div class="box"><h1>404</h1><p>This page doesn't exist.</p><a href="/">Back to memories</a></div></body></html>"#;
    (axum::http::StatusCode::NOT_FOUND, Html(body.to_string()))
}
