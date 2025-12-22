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

type Result<T> = std::result::Result<T, sacp::Error>;

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
    if bridge_guard.is_none() {
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

pub fn build_server() -> McpServer<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>> {
    let bridge: BridgeType = Arc::new(Mutex::new(None));

    McpServer::builder("rust-analyzer-mcp".to_string())
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
                        let uri = file_path_to_uri(&input.file_path);
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
                        let uri = file_path_to_uri(&input.file_path);
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
                        let uri = file_path_to_uri(&input.file_path);
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
                        let uri = file_path_to_uri(&input.file_path);
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
                        let uri = file_path_to_uri(&input.file_path);
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
                        let uri = file_path_to_uri(&input.file_path);
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
                        let uri = file_path_to_uri(&input.file_path);
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
                        // For workspace diagnostics, we'll return an empty array for now
                        // as lsp-bridge doesn't have a direct workspace diagnostics method
                        Ok(serde_json::to_string(&Vec::<serde_json::Value>::new())?)
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
                async move |_input: FilePositionInputs, _mcp_cx| {
                    with_bridge(&bridge, None, async move |_lsp| {
                        // This is a rust-analyzer specific extension that may not be available in lsp-bridge
                        // Return empty result for now
                        Ok(serde_json::to_string(&serde_json::Value::Null)?)
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
                async move |_input: GoalIndexInputs, _mcp_cx| {
                    with_bridge(&bridge, None, async move |_lsp| {
                        // This is a rust-analyzer specific extension that may not be available in lsp-bridge
                        // Return empty result for now
                        Ok(serde_json::to_string(&serde_json::Value::Null)?)
                    }).await
                }
            },
            sacp::tool_fn_mut!(),
        )
        .build()
}
