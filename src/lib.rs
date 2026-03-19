mod failed_obligations;
mod lsp_client;
mod rust_analyzer_mcp;

pub use rust_analyzer_mcp::{
    BridgeState, BridgeType, SERVER_ID, build_server, with_bridge_and_document,
};
use sacp::{Conductor, ConnectTo, Proxy};

#[derive(Default)]
pub struct RustAnalyzerProxy {
    pub workspace_path: Option<String>,
}

impl ConnectTo<Conductor> for RustAnalyzerProxy {
    async fn connect_to(
        self,
        client: impl ConnectTo<Proxy>,
    ) -> std::result::Result<(), sacp::Error> {
        Proxy
            .builder()
            .name("rust-analyzer-proxy")
            .with_mcp_server(build_server(self.workspace_path).await?)
            .connect_to(client)
            .await
    }
}
