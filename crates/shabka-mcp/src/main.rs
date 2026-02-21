use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

use shabka_mcp::ShabkaServer;

#[derive(Parser)]
#[command(name = "shabka-mcp", about = "Shabka MCP server")]
struct Cli {
    /// Start in HTTP mode instead of stdio (default port: 8080)
    #[arg(long, num_args = 0..=1, default_missing_value = "8080", value_name = "PORT")]
    http: Option<u16>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    match cli.http {
        Some(port) => run_http(port).await,
        None => run_stdio().await,
    }
}

async fn run_stdio() -> Result<()> {
    tracing::info!("Starting Shabka MCP server (stdio)");
    let service = ShabkaServer::new()?;
    let running = service.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

async fn run_http(port: u16) -> Result<()> {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
    use rmcp::transport::StreamableHttpService;
    use tokio_util::sync::CancellationToken;

    tracing::info!("Starting Shabka MCP server (HTTP on port {port})");

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

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("Listening on http://{addr}/mcp");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await?;

    Ok(())
}
