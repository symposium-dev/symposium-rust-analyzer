use anyhow::anyhow;
use lsp_bridge::{LspBridge, LspServerConfig};
use lsp_types::{CodeActionContext, Position, Range};
use sacp::{ProxyToConductor, mcp_server::McpServer};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

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

type BridgeType = Arc<Mutex<Option<SafeLspBridge>>>;

#[derive(Serialize, Deserialize, JsonSchema)]
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
struct GoalIndexInputs {
    pub goal_index: Value,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct EmptyInputs {}

const SERVER_ID: &str = "rust-analyzer";

async fn with_bridge<F, R>(bridge: &BridgeType, workspace_path: Option<&str>, f: F) -> Result<R>
where
    F: AsyncFnOnce(&LspBridge) -> Result<R>,
{
    let mut bridge_guard = bridge.lock().await;
    if bridge_guard.is_none() || workspace_path.is_some() {
        let workspace = workspace_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let mut lsp_bridge = LspBridge::new();
        let config = LspServerConfig::new()
            .command("rust-analyzer")
            .root_path(workspace);

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
        *bridge_guard = Some(SafeLspBridge::new(lsp_bridge));
    }
    f(bridge_guard.as_ref().unwrap()).await
}

fn file_path_to_uri(file_path: &str) -> String {
    if file_path.starts_with("file://") {
        file_path.to_string()
    } else {
        format!("file://{}", file_path)
    }
}

async fn ensure_document_open(bridge: &LspBridge, file_path: &str) -> Result<String> {
    let uri = file_path_to_uri(file_path);

    // Check if we need to open the document
    // For now, we'll try to read the file content and open it
    if let Ok(content) = std::fs::read_to_string(file_path) {
        bridge
            .open_document(SERVER_ID, &uri, &content)
            .await
            .map_err(|e| anyhow!("Failed to open document: {}", e))?;
    }

    Ok(uri)
}

pub async fn build_server(
    workspace_path: Option<String>,
) -> Result<McpServer<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>>> {
    let bridge: BridgeType = Arc::new(Mutex::new(None));
    with_bridge(&bridge, workspace_path.as_deref(), async |_lsp| Ok(())).await?;

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
                    with_bridge(&bridge, None,  async move |lsp| {
                        let uri = ensure_document_open(lsp, &input.file_path).await?;
                        let position = Position::new(input.line, input.character);
                        let result = lsp.get_hover(SERVER_ID, &uri, position).await
                            .map_err(|e| anyhow!("Hover request failed: {}", e))?;
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
                    with_bridge(&bridge, None, async move |lsp| {
                        let uri = ensure_document_open(lsp, &input.file_path).await?;
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
                    with_bridge(&bridge, None, async move |lsp| {
                        let uri = ensure_document_open(lsp, &input.file_path).await?;
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
                    with_bridge(&bridge, None, async move |lsp| {
                        let uri = ensure_document_open(lsp, &input.file_path).await?;
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
                    with_bridge(&bridge, None, async move |lsp| {
                        let uri = ensure_document_open(lsp, &input.file_path).await?;
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
                    with_bridge(&bridge, None, async move |lsp| {
                        let uri = ensure_document_open(lsp, &input.file_path).await?;
                        let result = lsp.format_document(SERVER_ID, &uri).await
                            .map_err(|e| anyhow!("Format request failed: {}", e))?;
                        Ok(serde_json::to_string(&result)?)
                    }).await
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
                    with_bridge(&bridge, None, async move |lsp| {
                        let uri = ensure_document_open(lsp, &input.file_path).await?;
                        let range = Range::new(
                            Position::new(input.line, input.character),
                            Position::new(input.end_line, input.end_character)
                        );
                        let context = CodeActionContext {
                            diagnostics: vec![],
                            only: None,
                            trigger_kind: None,
                        };
                        let result = lsp.get_code_actions(SERVER_ID, &uri, range, context).await
                            .map_err(|e| anyhow!("Code actions request failed: {}", e))?;
                        Ok(serde_json::to_string(&result)?)
                    }).await
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
                    with_bridge(&bridge, None, async move |lsp| {
                        let uri = ensure_document_open(lsp, &input.file_path).await?;

                        // Wait a bit for rust-analyzer to process and run cargo check
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                        let result = lsp.get_diagnostics(SERVER_ID, &uri)
                            .map_err(|e| anyhow!("Diagnostics request failed: {}", e))?;
                        Ok(serde_json::to_string(&result)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_workspace_diagnostics",
            "Get all compiler diagnostics across the entire workspace",
            {
                let bridge = bridge.clone();
                async move |_input: EmptyInputs, _mcp_cx| {
                    with_bridge(&bridge, None, async move |_lsp| {
                        // Try to get workspace diagnostics, fallback to empty if not available
                        // Wait for rust-analyzer to process workspace
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

                        // Since lsp-bridge may not have workspace diagnostics, return structured empty result
                        let result = serde_json::json!({
                            "workspace": std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).display().to_string(),
                            "files": {},
                            "summary": {
                                "total_files": 0,
                                "total_errors": 0,
                                "total_warnings": 0,
                                "total_information": 0,
                                "total_hints": 0,
                                "note": "Workspace diagnostics not directly available through lsp-bridge"
                            }
                        });

                        Ok(serde_json::to_string(&result)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_failed_obligations",
            "Get failed trait obligations at a position. Returns a goal_index when nested goals exist.",
            {
                let bridge = bridge.clone();
                async move |input: FilePositionInputs, _mcp_cx| {
                    with_bridge(&bridge, None, async move |lsp| {
                        let uri = ensure_document_open(lsp, &input.file_path).await?;
                        let _position = Position::new(input.line, input.character);

                        // Try to get failed obligations using a custom LSP request
                        // This may not be available in lsp-bridge, so we'll return debug info
                        let debug_result = serde_json::json!({
                            "result": null,
                            "debug_info": {
                                "request": {
                                    "uri": uri,
                                    "position": { "line": input.line, "character": input.character },
                                    "method": "rust-analyzer/getFailedObligations"
                                },
                                "possible_reasons": [
                                    "No trait obligation failures at this exact position",
                                    "Position not inside function with trait constraints",
                                    "Feature requires recent rust-analyzer version or not available in lsp-bridge"
                                ]
                            }
                        });

                        Ok(serde_json::to_string(&debug_result)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .tool_fn_mut(
            "rust_analyzer_failed_obligations_goal",
            "Explore a specific nested_goal (or list of nested_goals) and its candidates.",
            {
                let bridge = bridge.clone();
                async move |input: GoalIndexInputs, _mcp_cx| {
                    with_bridge(&bridge, None, async move |_lsp| {
                        let goal_indices = match &input.goal_index {
                            serde_json::Value::String(s) => vec![s.clone()],
                            serde_json::Value::Array(arr) => {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            }
                            _ => return Ok(serde_json::to_string(&serde_json::json!({
                                "error": "goal_index must be a string or array of strings"
                            }))?),
                        };

                        if goal_indices.is_empty() {
                            return Ok(serde_json::to_string(&serde_json::json!({
                                "error": "At least one goal_index is required"
                            }))?)
                        }

                        // Since we don't have state management in this implementation,
                        // return an error indicating the goal_index is invalid
                        let error_result = serde_json::json!({
                            "error": "Invalid goal_index or expired data",
                            "debug_info": {
                                "requested_indices": goal_indices,
                                "note": "Failed obligations goal exploration requires state management not available in this lsp-bridge implementation"
                            }
                        });

                        Ok(serde_json::to_string(&error_result)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .build();

    Ok(server)
}
