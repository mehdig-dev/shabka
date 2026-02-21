mod error;
mod routes;

use std::sync::Arc;

use anyhow::Result;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::StreamableHttpService;
use shabka_core::config::{self, ShabkaConfig};
use shabka_core::embedding::EmbeddingService;
use shabka_core::history::HistoryLogger;
use shabka_core::llm::LlmService;
use shabka_core::storage::{create_backend, Storage};
use shabka_mcp::ShabkaServer;
use tokio_util::sync::CancellationToken;

pub struct AppState {
    pub storage: Storage,
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

    let storage = create_backend(&config)?;

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

    // Build MCP HTTP service
    let ct = CancellationToken::new();
    let session_manager = Arc::new(LocalSessionManager::default());
    let mcp_config = StreamableHttpServerConfig {
        sse_keep_alive: Some(std::time::Duration::from_secs(30)),
        sse_retry: Some(std::time::Duration::from_secs(3)),
        stateful_mode: true,
        cancellation_token: ct,
    };
    let mcp_service = StreamableHttpService::new(
        || ShabkaServer::new().map_err(std::io::Error::other),
        session_manager,
        mcp_config,
    );

    let app = routes::router()
        .with_state(state)
        .nest_service("/mcp", mcp_service)
        .layer(tower_http::cors::CorsLayer::permissive());

    let addr = format!("{}:{}", config.web.host, config.web.port);
    tracing::info!("shabka-web listening on http://{addr}");
    tracing::info!("MCP endpoint available at http://{addr}/mcp");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
