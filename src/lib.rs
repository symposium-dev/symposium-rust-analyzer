mod rust_analyzer_mcp;

use anyhow::Result;
pub use rust_analyzer_mcp::build_server;
use sacp::ProxyToConductor;
use sacp::component::Component;

#[derive(Default)]
pub struct RustAnalyzerProxy {
    pub workspace_path: Option<String>,
}

impl Component for RustAnalyzerProxy {
    async fn serve(self, client: impl Component) -> Result<(), sacp::Error> {
        ProxyToConductor::builder()
            .name("rust-analyzer-proxy")
            .with_mcp_server(build_server(self.workspace_path).await?)
            .serve(client)
            .await
    }
}
