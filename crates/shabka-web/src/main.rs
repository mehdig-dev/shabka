mod error;
mod routes;

use std::sync::Arc;

use anyhow::Result;
use shabka_core::config::{self, ShabkaConfig};
use shabka_core::embedding::EmbeddingService;
use shabka_core::history::HistoryLogger;
use shabka_core::llm::LlmService;
use shabka_core::storage::HelixStorage;

pub struct AppState {
    pub storage: HelixStorage,
    pub embedding: EmbeddingService,
    pub config: ShabkaConfig,
    pub user_id: String,
    pub history: HistoryLogger,
    pub llm: Option<LlmService>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "shabka_web=info".parse().unwrap()),
        )
        .init();

    let config = ShabkaConfig::load(None).unwrap_or_else(|_| ShabkaConfig::default_config());

    let storage = HelixStorage::new(
        Some(&config.helix.url),
        Some(config.helix.port),
        config.helix.api_key.as_deref(),
    );

    let embedding = EmbeddingService::from_config(&config.embedding)?;

    let user_id = config::resolve_user_id(&config.sharing);
    let history = HistoryLogger::new(config.history.enabled);

    let llm = if config.llm.enabled {
        LlmService::from_config(&config.llm).ok()
    } else {
        None
    };

    let state = Arc::new(AppState {
        storage,
        embedding,
        config: config.clone(),
        user_id,
        history,
        llm,
    });

    let app = routes::router()
        .with_state(state)
        .layer(tower_http::cors::CorsLayer::permissive());

    let addr = format!("{}:{}", config.web.host, config.web.port);
    tracing::info!("shabka-web listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
