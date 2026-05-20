use agent_client_protocol::{ConnectTo, Stdio};
use anyhow::Result;
use symposium_rust_analyzer::build_server;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .init();

    let mcp = build_server(None).await?;
    mcp.connect_to(Stdio::new()).await?;

    Ok(())
}
