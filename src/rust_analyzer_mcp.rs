use anyhow::anyhow;
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
use crate::lsp_client::LspClient;

pub type Result<T> = std::result::Result<T, sacp::Error>;

pub struct BridgeState {
    client: Option<LspClient>,
    opened_documents: HashSet<String>,
    document_versions: HashMap<String, i32>,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            client: None,
            opened_documents: HashSet::new(),
            document_versions: HashMap::new(),
        }
    }
}

pub type BridgeType = Arc<Mutex<BridgeState>>;

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct FilePositionInputs {
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

pub(crate) async fn ensure_bridge(bridge: &BridgeType, workspace_path: Option<&str>) -> Result<()> {
    let mut bridge_guard = bridge.lock().await;
    if bridge_guard.client.is_none() || workspace_path.is_some() {
        let workspace = workspace_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        tracing::debug!(?workspace);

        let root_uri = Uri::from_str(&format!("file://{}", workspace.display()))
            .map_err(|e| anyhow!("Invalid workspace path: {}", e))?;

        tracing::debug!(?root_uri);

        let client = LspClient::new("rust-analyzer", &[], root_uri)
            .await
            .map_err(|e| anyhow!("Failed to start rust-analyzer: {}", e))?;

        tracing::debug!(?client);

        bridge_guard.client = Some(client);
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
    F: for<'a> AsyncFnOnce(&'a LspClient) -> Result<R>,
{
    ensure_bridge(bridge, workspace_path).await?;
    let bridge_guard = bridge.lock().await;
    f(bridge_guard.client.as_ref().unwrap()).await
}

pub async fn with_bridge_and_document<F, R>(
    bridge: &BridgeType,
    workspace_path: Option<&str>,
    file_path: &str,
    f: F,
) -> Result<R>
where
    F: for<'a> AsyncFnOnce(&'a LspClient, Uri) -> Result<R>,
{
    ensure_bridge(bridge, workspace_path).await?;
    let mut bridge_guard = bridge.lock().await;
    let uri = ensure_document_open(&mut bridge_guard, file_path).await?;
    f(bridge_guard.client.as_ref().unwrap(), uri).await
}

fn file_path_to_uri(file_path: &str) -> anyhow::Result<Uri> {
    if file_path.starts_with("file://") {
        Uri::from_str(file_path).map_err(|e| anyhow!("Invalid URI: {}", e))
    } else {
        Uri::from_str(&format!("file://{}", file_path))
            .map_err(|e| anyhow!("Invalid file path: {}", e))
    }
}

async fn ensure_document_open(bridge_state: &mut BridgeState, file_path: &str) -> Result<Uri> {
    let uri = file_path_to_uri(file_path)?;
    let uri_str = uri.to_string();

    // Only open if not already opened
    if !bridge_state.opened_documents.contains(&uri_str) {
        if let Ok(content) = std::fs::read_to_string(file_path) {
            if let Some(client) = &bridge_state.client {
                let version = bridge_state
                    .document_versions
                    .get(&uri_str)
                    .copied()
                    .unwrap_or(1);
                client
                    .did_open(uri.clone(), "rust".to_string(), version, content)
                    .await
                    .map_err(|e| anyhow!("Failed to open document: {}", e))?;
                bridge_state.opened_documents.insert(uri_str.clone());
                bridge_state.document_versions.insert(uri_str, version);
            }
        }
    }

    Ok(uri)
}

pub async fn build_server(
    workspace_path: Option<String>,
) -> Result<McpServer<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>>> {
    let bridge: BridgeType = Arc::new(Mutex::new(BridgeState::new()));
    with_bridge(&bridge, workspace_path.as_deref(), async |_client| Ok(())).await?;

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
                    with_bridge_and_document(
                        &bridge,
                        None,
                        &input.file_path,
                        async move |client, uri| {
                            let position = Position::new(input.line, input.character);
                            let result = client
                                .hover(uri, position)
                                .await
                                .map_err(|e| anyhow!("Hover request failed: {}", e))?;
                            Ok(serde_json::to_string(&result)?)
                        },
                    )
                    .await
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
                    with_bridge_and_document(
                        &bridge,
                        None,
                        &input.file_path,
                        async move |client, uri| {
                            let position = Position::new(input.line, input.character);
                            let result = client
                                .goto_definition(uri, position)
                                .await
                                .map_err(|e| anyhow!("Definition request failed: {}", e))?;
                            Ok(serde_json::to_string(&result)?)
                        },
                    )
                    .await
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
                    with_bridge_and_document(
                        &bridge,
                        None,
                        &input.file_path,
                        async move |client, uri| {
                            let position = Position::new(input.line, input.character);
                            let result = client
                                .find_references(uri, position, true)
                                .await
                                .map_err(|e| anyhow!("References request failed: {}", e))?;
                            Ok(serde_json::to_string(&result)?)
                        },
                    )
                    .await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_completion",
            "Get code completions at a specific position",
            {
                let bridge = bridge.clone();
                async move |input: FilePositionInputs, _mcp_cx| {
                    with_bridge_and_document(
                        &bridge,
                        None,
                        &input.file_path,
                        async move |client, uri| {
                            let position = Position::new(input.line, input.character);
                            let result = client
                                .completion(uri, position)
                                .await
                                .map_err(|e| anyhow!("Completion request failed: {}", e))?;
                            Ok(serde_json::to_string(&result)?)
                        },
                    )
                    .await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_symbols",
            "Get document symbols for a Rust file",
            {
                let bridge = bridge.clone();
                async move |input: FileOnlyInputs, _mcp_cx| {
                    with_bridge_and_document(
                        &bridge,
                        None,
                        &input.file_path,
                        async move |client, uri| {
                            let result = client
                                .document_symbols(uri)
                                .await
                                .map_err(|e| anyhow!("Document symbols request failed: {}", e))?;
                            Ok(serde_json::to_string(&result)?)
                        },
                    )
                    .await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_format",
            "Format a Rust document",
            {
                let bridge = bridge.clone();
                async move |input: FileOnlyInputs, _mcp_cx| {
                    with_bridge_and_document(
                        &bridge,
                        None,
                        &input.file_path,
                        async move |client, uri| {
                            let result = client
                                .format_document(uri)
                                .await
                                .map_err(|e| anyhow!("Format request failed: {}", e))?;
                            Ok(serde_json::to_string(&result)?)
                        },
                    )
                    .await
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
                    with_bridge_and_document(
                        &bridge,
                        None,
                        &input.file_path,
                        async move |client, uri| {
                            let range = Range::new(
                                Position::new(input.line, input.character),
                                Position::new(input.end_line, input.end_character),
                            );
                            let context = CodeActionContext {
                                diagnostics: vec![],
                                only: None,
                                trigger_kind: None,
                            };
                            let result = client
                                .code_actions(uri, range, context)
                                .await
                                .map_err(|e| anyhow!("Code actions request failed: {}", e))?;
                            Ok(serde_json::to_string(&result)?)
                        },
                    )
                    .await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_set_workspace",
            "Set the workspace root for rust-analyzer",
            {
                let bridge = bridge.clone();
                async move |input: WorkspaceInputs, _mcp_cx| {
                    with_bridge(&bridge, Some(&input.workspace_path), async move |_client| {
                        Ok("Workspace set successfully".to_string())
                    })
                    .await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_diagnostics",
            "Get diagnostics for a Rust file",
            {
                let bridge = bridge.clone();
                async move |input: FileOnlyInputs, _mcp_cx| {
                    with_bridge_and_document(
                        &bridge,
                        None,
                        &input.file_path,
                        async move |client, uri| {
                            let result = client
                                .diagnostics(uri)
                                .await
                                .map_err(|e| anyhow!("Diagnostics request failed: {}", e))?;
                            Ok(serde_json::to_string(&result)?)
                        },
                    )
                    .await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_failed_obligations",
            "Get failed trait obligations for debugging (rust-analyzer specific)",
            {
                let bridge = bridge.clone();
                let state = failed_obligations_state.clone();
                async move |input: FilePositionInputs, _mcp_cx| {
                    let mut bridge_guard = bridge.lock().await;
                    let uri = ensure_document_open(&mut bridge_guard, &input.file_path).await?;
                    let doc = TextDocumentIdentifier { uri };
                    let position = Position::new(input.line, input.character);

                    let args = TextDocumentPositionParams {
                        text_document: doc,
                        position,
                    };

                    let mut state = state.lock().await;
                    use std::ops::DerefMut;
                    let state = state.deref_mut();
                    let result = handle_failed_obligations(
                        bridge_guard.client.as_ref().unwrap(),
                        state,
                        args,
                    )
                    .await?;

                    Ok(serde_json::to_string(&result).map_err(|e| anyhow::Error::new(e))?)
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_failed_obligations_goal",
            "Explore nested goals in failed trait obligations (rust-analyzer specific)",
            {
                let bridge = bridge.clone();
                let state = failed_obligations_state.clone();
                async move |input: GoalIndexInputs, _mcp_cx| {
                    let bridge_guard = bridge.lock().await;
                    let mut state = state.lock().await;
                    use std::ops::DerefMut;
                    let state = state.deref_mut();
                    let result = handle_failed_obligations_goal(
                        bridge_guard.client.as_ref().unwrap(),
                        state,
                        input,
                    )
                    .await?;

                    Ok(serde_json::to_string(&result).map_err(|e| anyhow::Error::new(e))?)
                }
            },
            sacp::tool_fn_mut!(),
        )
        .build();

    Ok(server)
}
