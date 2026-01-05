use anyhow::anyhow;
use lsp_bridge::{LspBridge, LspClientCapabilities, LspError, LspServerConfig};
use lsp_types::{
    CodeActionContext, Position, Range, TextDocumentIdentifier, TextDocumentPositionParams, Uri,
};
use sacp::{ProxyToConductor, mcp_server::McpServer};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::failed_obligations::{
    FailedObligationsState, handle_failed_obligations, handle_failed_obligations_goal,
};

pub type Result<T> = std::result::Result<T, sacp::Error>;

struct SafeLspBridge(Option<LspBridge>);

impl SafeLspBridge {
    fn new(bridge: LspBridge) -> Self {
        Self(Some(bridge))
    }
}

impl std::ops::Deref for SafeLspBridge {
    type Target = LspBridge;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for SafeLspBridge {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().unwrap()
    }
}

impl Drop for SafeLspBridge {
    fn drop(&mut self) {
        // The Drop impl of LspBridge uses futures's block_on, but this doesn't work with the tokio runtime.
        // So, given that this should only be dropped at the end of the program, just mem forget.
        // I hate this, btw. It means that we need an `Option` and the `unwrap`s.
        if let Some(bridge) = self.0.take() {
            std::mem::forget(bridge);
        }
    }
}

pub struct BridgeState {
    bridge: Option<SafeLspBridge>,
    opened_documents: HashSet<String>,
    document_versions: HashMap<String, i32>,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            bridge: None,
            opened_documents: HashSet::new(),
            document_versions: HashMap::new(),
        }
    }
}

pub type BridgeType = Arc<Mutex<BridgeState>>;

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
struct FilePositionInputs {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct FileOnlyInputs {
    pub file_path: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct RangeInputs {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct WorkspaceInputs {
    pub workspace_path: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GoalIndexInputs {
    pub goal_index: Value,
}

pub const SERVER_ID: &str = "rust-analyzer";

fn initialization_options() -> Value {
    serde_json::json!({
        "cargo": {
            "buildScripts": {
                "enable": true
            }
        },
        "checkOnSave": {
            "enable": true,
            "command": "check",
            "allTargets": true
        },
        "diagnostics": {
            "enable": true,
            "experimental": {
                "enable": false
            }
        },
        "procMacro": {
            "enable": true
        }
    })
}

fn capabilities() -> LspClientCapabilities {
    let capabilities = LspClientCapabilities {
        text_document: lsp_bridge::config::TextDocumentClientCapabilities {
            hover: Some(lsp_bridge::config::HoverClientCapabilities),
            completion: Some(lsp_bridge::config::CompletionClientCapabilities),
            definition: Some(lsp_bridge::config::GotoCapability),
            ..Default::default()
        },
        workspace: lsp_bridge::config::WorkspaceClientCapabilities {
            did_change_configuration: None,
            ..Default::default()
        },
        window: lsp_bridge::config::WindowClientCapabilities {
            work_done_progress: Some(true),
            ..Default::default()
        },
        experimental: Some(serde_json::json!({
            "serverStatusNotification": true,
        })),
        ..Default::default()
    };

    capabilities
}

pub(crate) async fn ensure_bridge(bridge: &BridgeType, workspace_path: Option<&str>) -> Result<()> {
    let mut bridge_guard = bridge.lock().await;
    if bridge_guard.bridge.is_none() || workspace_path.is_some() {
        let workspace = workspace_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        tracing::debug!(?workspace);

        let mut lsp_bridge = LspBridge::new();
        let config = LspServerConfig::new()
            .command("rust-analyzer")
            .trace(lsp_bridge::config::TraceLevel::Verbose)
            .env("RA_LOG", "base_db,rust_analyzer")
            .client_capabilities(capabilities())
            .initialization_options(initialization_options())
            .root_path(workspace.clone())
            .workspace_folder(workspace);

        lsp_bridge
            .register_server(SERVER_ID, config)
            .await
            .map_err(|e| anyhow!("Failed to register server: {}", e))?;
        lsp_bridge
            .start_server(SERVER_ID)
            .await
            .map_err(|e| anyhow!("Failed to start server: {}", e))?;
        lsp_bridge
            .wait_server_ready(SERVER_ID)
            .await
            .map_err(|e| anyhow!("Server failed to become ready: {}", e))?;
        bridge_guard.bridge = Some(SafeLspBridge::new(lsp_bridge));
        bridge_guard.opened_documents.clear();
        bridge_guard.document_versions.clear();
    }
    Ok(())
}

pub(crate) async fn with_bridge<F, R>(
    bridge: &BridgeType,
    workspace_path: Option<&str>,
    f: F,
) -> Result<R>
where
    F: AsyncFnOnce(&LspBridge) -> Result<R>,
{
    ensure_bridge(bridge, workspace_path).await?;
    let bridge_guard = bridge.lock().await;
    f(bridge_guard.bridge.as_ref().unwrap()).await
}

pub async fn with_bridge_and_document<F, R>(
    bridge: &BridgeType,
    workspace_path: Option<&str>,
    file_path: &str,
    f: F,
) -> Result<R>
where
    F: AsyncFnOnce(&LspBridge, String) -> Result<R>,
{
    ensure_bridge(bridge, workspace_path).await?;
    let mut bridge_guard = bridge.lock().await;
    let uri = ensure_document_open(&mut bridge_guard, file_path).await?;
    f(bridge_guard.bridge.as_ref().unwrap(), uri).await
}

fn file_path_to_uri(file_path: &str) -> String {
    if file_path.starts_with("file://") {
        file_path.to_string()
    } else {
        format!("file://{}", file_path)
    }
}

async fn ensure_document_open(bridge_state: &mut BridgeState, file_path: &str) -> Result<String> {
    let uri = file_path_to_uri(file_path);

    // Only open if not already opened
    if !bridge_state.opened_documents.contains(&uri) {
        if let Ok(content) = std::fs::read_to_string(file_path) {
            if let Some(bridge) = &bridge_state.bridge {
                bridge
                    .open_document(SERVER_ID, &uri, &content)
                    .await
                    .map_err(|e| anyhow!("Failed to open document: {}", e))?;
                bridge_state.opened_documents.insert(uri.clone());
                bridge_state.document_versions.insert(uri.clone(), 1);
            }
        }
    }

    Ok(uri)
}

pub async fn build_server(
    workspace_path: Option<String>,
) -> Result<McpServer<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>>> {
    let bridge: BridgeType = Arc::new(Mutex::new(BridgeState::new()));
    with_bridge(&bridge, workspace_path.as_deref(), async |_lsp| Ok(())).await?;

    let failed_obligations_state = Arc::new(Mutex::new(FailedObligationsState::new()));
    let server = McpServer::builder("rust-analyzer-mcp".to_string())
        .instructions(indoc::indoc! {"
            Rust analyzer LSP integration for code analysis, navigation, and diagnostics.
        "})
        .tool_fn_mut(
            "rust_analyzer_hover",
            "Get hover information for a symbol at a specific position in a Rust file",
            {
                let bridge = bridge.clone();
                async move |input: FilePositionInputs, _mcp_cx| {
                    with_bridge_and_document(&bridge, None, &input.file_path, async move |lsp, uri| {
                        let position = Position::new(input.line, input.character);
                        let result = lsp.get_hover(SERVER_ID, &uri, position).await
                            .map_err(|e| anyhow!("Hover request failed: {}", e))?;
                        dbg!(&result);
                        Ok(serde_json::to_string(&result)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_definition",
            "Go to definition of a symbol at a specific position",
            {
                let bridge = bridge.clone();
                async move |input: FilePositionInputs, _mcp_cx| {
                    with_bridge_and_document(&bridge, None, &input.file_path, async move |lsp, uri| {
                        let position = Position::new(input.line, input.character);
                        let result = lsp.go_to_definition(SERVER_ID, &uri, position).await
                            .map_err(|e| anyhow!("Definition request failed: {}", e))?;
                        Ok(serde_json::to_string(&result)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_references",
            "Find all references to a symbol at a specific position",
            {
                let bridge = bridge.clone();
                async move |input: FilePositionInputs, _mcp_cx| {
                    with_bridge_and_document(&bridge, None, &input.file_path, async move |lsp, uri| {
                        let position = Position::new(input.line, input.character);
                        let result = lsp.find_references(SERVER_ID, &uri, position).await
                            .map_err(|e| anyhow!("References request failed: {}", e))?;
                        Ok(serde_json::to_string(&result)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_completion",
            "Get code completion suggestions at a specific position",
            {
                let bridge = bridge.clone();
                async move |input: FilePositionInputs, _mcp_cx| {
                    with_bridge_and_document(&bridge, None, &input.file_path, async move |lsp, uri| {
                        let position = Position::new(input.line, input.character);
                        let result = lsp.get_completions(SERVER_ID, &uri, position).await
                            .map_err(|e| anyhow!("Completion request failed: {}", e))?;
                        Ok(serde_json::to_string(&result)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_symbols",
            "Get document symbols (functions, structs, etc.) for a Rust file",
            {
                let bridge = bridge.clone();
                async move |input: FileOnlyInputs, _mcp_cx| {
                    with_bridge_and_document(&bridge, None, &input.file_path, async move |lsp, uri| {
                        let result = lsp.get_document_symbols(SERVER_ID, &uri).await
                            .map_err(|e| anyhow!("Document symbols request failed: {}", e))?;
                        Ok(serde_json::to_string(&result)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_format",
            "Format a Rust file using rust-analyzer",
            {
                let bridge = bridge.clone();
                async move |input: FileOnlyInputs, _mcp_cx| {
                    let mut bridge_guard = bridge.lock().await;
                    let uri = ensure_document_open(&mut bridge_guard, &input.file_path).await?;
                    let result = bridge_guard.bridge.as_ref().unwrap().format_document(SERVER_ID, &uri).await
                        .map_err(|e| anyhow!("Format request failed: {}", e))?;
                    Ok(serde_json::to_string(&result)?)
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_code_actions",
            "Get available code actions for a range in a Rust file",
            {
                let bridge = bridge.clone();
                async move |input: RangeInputs, _mcp_cx| {
                    let mut bridge_guard = bridge.lock().await;
                    let uri = ensure_document_open(&mut bridge_guard, &input.file_path).await?;
                    let range = Range::new(
                        Position::new(input.line, input.character),
                        Position::new(input.end_line, input.end_character)
                    );
                    let context = CodeActionContext {
                        diagnostics: vec![],
                        only: None,
                        trigger_kind: None,
                    };
                    let result = bridge_guard.bridge.as_ref().unwrap().get_code_actions(SERVER_ID, &uri, range, context).await
                        .map_err(|e| anyhow!("Code actions request failed: {}", e))?;
                    Ok(serde_json::to_string(&result)?)
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_set_workspace",
            "Set the workspace root directory for rust-analyzer",
            {
                let bridge = bridge.clone();
                async move |input: WorkspaceInputs, _mcp_cx| {
                    with_bridge(&bridge, Some(&input.workspace_path), async move |_lsp| {
                        Ok("Workspace set successfully".to_string())
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_diagnostics",
            "Get compiler diagnostics (errors, warnings, hints) for a Rust file",
            {
                let bridge = bridge.clone();
                async move |input: FileOnlyInputs, _mcp_cx| {
                    let mut bridge_guard = bridge.lock().await;
                    let uri = ensure_document_open(&mut bridge_guard, &input.file_path).await?;

                    // Wait a bit for rust-analyzer to process
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                    let result = bridge_guard.bridge.as_ref().unwrap().get_diagnostics(SERVER_ID, &uri)
                        .map_err(|e| anyhow!("Diagnostics request failed: {}", e))?;
                    Ok(serde_json::to_string(&result)?)
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_failed_obligations",
            "Get failed trait obligations at a position. Returns a goal_index when nested goals exist.",
            {
                let bridge = bridge.clone();
                let state = failed_obligations_state.clone();
                async move |input: FilePositionInputs, _mcp_cx| {
                    let mut bridge_guard = bridge.lock().await;
                    let uri = ensure_document_open(&mut bridge_guard, &input.file_path).await?;
                    let doc = TextDocumentIdentifier {
                        uri: Uri::from_str(&uri).map_err(|_| LspError::invalid_uri(uri)).map_err(|e| anyhow::Error::new(e))?,
                    };
                    let position = Position::new(input.line, input.character);

                    let args = TextDocumentPositionParams {
                        text_document: doc,
                        position,
                    };

                    let mut state = state.lock().await;
                    use std::ops::DerefMut;
                    let state = state.deref_mut();
                    let result = handle_failed_obligations(bridge_guard.bridge.as_ref().unwrap(), state, args).await?;

                    Ok(serde_json::to_string(&result).map_err(|e| anyhow::Error::new(e))?)
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_failed_obligations_goal",
            "Explore a specific nested_goal (or list of nested_goals) and its candidates.",
            {
                let bridge = bridge.clone();
                let state = failed_obligations_state.clone();
                async move |input: GoalIndexInputs, _mcp_cx| {
                    let bridge_guard = bridge.lock().await;
                    let mut state = state.lock().await;
                    use std::ops::DerefMut;
                    let state = state.deref_mut();
                    let result = handle_failed_obligations_goal(bridge_guard.bridge.as_ref().unwrap(), state, input).await?;

                    Ok(serde_json::to_string(&result).map_err(|e| anyhow::Error::new(e))?)
                }
            },
            sacp::tool_fn_mut!(),
        )
        .build();

    Ok(server)
}
