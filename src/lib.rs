mod rust_analyzer_proxy;

use anyhow::Result;
use sacp::ProxyToConductor;
use sacp::component::Component;

/// Run the proxy as a standalone binary connected to stdio
pub async fn run() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting rust-analyzer-proxy");

    RustAnalyzerProxy.serve(sacp_tokio::Stdio::new()).await?;

    Ok(())
}

pub struct RustAnalyzerProxy;

impl Component for RustAnalyzerProxy {
    async fn serve(self, client: impl Component) -> Result<(), sacp::Error> {
        ProxyToConductor::builder()
            .name("rust-analyzer-proxy")
            .with_mcp_server(rust_analyzer_proxy::build_server())
            .serve(client)
            .await
    }
}
