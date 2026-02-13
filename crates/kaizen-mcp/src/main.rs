use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

mod server;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("Starting Kaizen MCP server");

    let service = server::KaizenServer::new()?;
    let running = service.serve(stdio()).await?;
    running.waiting().await?;

    Ok(())
}
