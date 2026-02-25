use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

use shabka_mcp::ShabkaServer;

#[derive(Parser)]
#[command(name = "shabka-mcp", about = "Shabka MCP server", version)]
struct Cli {
    /// Start in HTTP mode instead of stdio (default port: 8080)
    #[arg(long, num_args = 0..=1, default_missing_value = "8080", value_name = "PORT")]
    http: Option<u16>,

    /// Bind address for HTTP mode (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1", value_name = "ADDR")]
    bind: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // Spawn auto-consolidation if configured
    maybe_auto_consolidate();

    match cli.http {
        Some(port) => run_http(port, &cli.bind).await,
        None => run_stdio().await,
    }
}

/// Check if auto-consolidation is due and spawn it in the background.
/// Never blocks startup or propagates errors.
fn maybe_auto_consolidate() {
    use shabka_core::config::{ConsolidateState, ShabkaConfig};

    let config = ShabkaConfig::load(Some(&std::env::current_dir().unwrap_or_default()))
        .unwrap_or_else(|_| ShabkaConfig::default_config());

    if !config.consolidate.auto {
        return;
    }

    let state = ConsolidateState::load();
    if !state.is_due(&config.consolidate.interval) {
        tracing::debug!("auto-consolidation not due (last run: {})", state.last_run);
        return;
    }

    tracing::info!("auto-consolidation is due, spawning background task");
    tokio::spawn(async move {
        if let Err(e) = run_auto_consolidate(config).await {
            tracing::warn!("auto-consolidation failed: {e}");
        }
    });
}

async fn run_auto_consolidate(config: shabka_core::config::ShabkaConfig) -> Result<()> {
    use shabka_core::config::ConsolidateState;
    use shabka_core::consolidate;
    use shabka_core::embedding::EmbeddingService;
    use shabka_core::history::HistoryLogger;
    use shabka_core::storage::create_backend;

    let storage = create_backend(&config)?;
    let embedder = EmbeddingService::from_config(&config.embedding)?;
    let llm = shabka_core::llm::LlmService::from_config(&config.llm)?;
    let history = HistoryLogger::new(config.history.enabled);
    let user_id = shabka_core::config::resolve_user_id(&config.sharing);

    let result = consolidate::consolidate(
        &storage,
        &embedder,
        &llm,
        &config.consolidate,
        &user_id,
        &history,
        false,
    )
    .await?;

    tracing::info!(
        "auto-consolidation complete: {} clusters consolidated, {} memories superseded, {} new memories",
        result.clusters_consolidated,
        result.memories_superseded,
        result.memories_created,
    );

    // Update state
    let state = ConsolidateState {
        last_run: chrono::Utc::now().to_rfc3339(),
        memories_consolidated: result.memories_superseded,
    };
    let _ = state.save();

    Ok(())
}

async fn run_stdio() -> Result<()> {
    tracing::info!("Starting Shabka MCP server (stdio)");
    let service = ShabkaServer::new()?;
    let running = service.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

async fn run_http(port: u16, bind: &str) -> Result<()> {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
    use rmcp::transport::StreamableHttpService;
    use tokio_util::sync::CancellationToken;

    tracing::info!("Starting Shabka MCP server (HTTP on {bind}:{port})");

    let ct = CancellationToken::new();
    let session_manager = Arc::new(LocalSessionManager::default());

    let config = StreamableHttpServerConfig {
        sse_keep_alive: Some(std::time::Duration::from_secs(30)),
        sse_retry: Some(std::time::Duration::from_secs(3)),
        stateful_mode: true,
        cancellation_token: ct.clone(),
    };

    let mcp_service = StreamableHttpService::new(
        || ShabkaServer::new().map_err(std::io::Error::other),
        session_manager,
        config,
    );

    let app = axum::Router::new().nest_service("/mcp", mcp_service);

    let addr = format!("{bind}:{port}");
    tracing::info!("Listening on http://{addr}/mcp");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await?;

    Ok(())
}
