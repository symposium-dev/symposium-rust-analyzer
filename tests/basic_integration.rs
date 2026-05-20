use std::path::PathBuf;

use anyhow::Result;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .compact()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .try_init();
}

fn get_test_project_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test-project")
}

fn get_test_file_path() -> String {
    get_test_project_path()
        .join("src/main.rs")
        .display()
        .to_string()
}

/// Spin up the MCP server and connect an rmcp client to it.
/// Returns the running client whose `.peer()` can call tools.
async fn create_mcp_client() -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let test_project = get_test_project_path();
    let server = symposium_rust_analyzer::build_server::<agent_client_protocol::role::mcp::Client>(
        Some(test_project.display().to_string()),
    )
    .await
    .expect("failed to build MCP server");

    let (server_stream, client_stream) = tokio::io::duplex(8192);
    let (server_read, server_write) = tokio::io::split(server_stream);
    let (client_read, client_write) = tokio::io::split(client_stream);

    // Spawn the MCP server in the background
    tokio::spawn(async move {
        use agent_client_protocol::{ByteStreams, ConnectTo};
        let transport = ByteStreams::new(server_write.compat_write(), server_read.compat());
        server.connect_to(transport).await
    });

    // Connect the rmcp client
    ().serve((client_read, client_write))
        .await
        .expect("failed to connect rmcp client")
}

/// Call a tool and return the text content from the result.
async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args: serde_json::Value,
) -> String {
    let params = CallToolRequestParams::new(name.to_string())
        .with_arguments(args.as_object().unwrap().clone());

    let result = client.peer().call_tool(params).await.unwrap();

    result
        .content
        .iter()
        .filter_map(|c| c.raw.as_text().map(|t| t.text.as_str()))
        .collect::<Vec<_>>()
        .join("")
}

#[tokio::test]
async fn test_rust_analyzer_set_workspace() -> Result<()> {
    init_tracing();
    let client = create_mcp_client().await;
    let test_project = get_test_project_path();

    let result = call_tool(
        &client,
        "rust_analyzer_set_workspace",
        serde_json::json!({ "workspace_path": test_project.display().to_string() }),
    )
    .await;

    assert!(result.contains("Workspace set successfully"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_hover() -> Result<()> {
    init_tracing();
    let client = create_mcp_client().await;
    let file_path = get_test_file_path();

    let result = call_tool(
        &client,
        "rust_analyzer_hover",
        serde_json::json!({ "file_path": file_path, "line": 3, "character": 11 }),
    )
    .await;

    assert!(result.contains("name: String"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_definition() -> Result<()> {
    init_tracing();
    let client = create_mcp_client().await;
    let file_path = get_test_file_path();

    let result = call_tool(
        &client,
        "rust_analyzer_definition",
        serde_json::json!({ "file_path": file_path, "line": 0, "character": 25 }),
    )
    .await;

    assert!(result.contains("src/collections/hash/map.rs"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_references() -> Result<()> {
    init_tracing();
    let client = create_mcp_client().await;
    let file_path = get_test_file_path();

    let result = call_tool(
        &client,
        "rust_analyzer_references",
        serde_json::json!({ "file_path": file_path, "line": 3, "character": 11 }),
    )
    .await;

    assert!(result.contains("test-project/src/main.rs"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_completion() -> Result<()> {
    init_tracing();
    let client = create_mcp_client().await;
    let file_path = get_test_file_path();

    let result = call_tool(
        &client,
        "rust_analyzer_completion",
        serde_json::json!({ "file_path": file_path, "line": 99, "character": 29 }),
    )
    .await;

    assert!(result.contains("greet"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_symbols() -> Result<()> {
    init_tracing();
    let client = create_mcp_client().await;
    let file_path = get_test_file_path();

    let result = call_tool(
        &client,
        "rust_analyzer_symbols",
        serde_json::json!({ "file_path": file_path }),
    )
    .await;

    assert!(result.contains("Person"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_lsp_call_notification() -> Result<()> {
    init_tracing();
    let client = create_mcp_client().await;
    let test_project = get_test_project_path();

    let result = call_tool(
        &client,
        "rust_analyzer_lsp_call",
        serde_json::json!({
            "method": "window/logMessage",
            "params": { "type": 1, "message": "hello from test" },
            "is_notification": true,
            "workspace_path": test_project.display().to_string()
        }),
    )
    .await;

    assert!(result.contains("Notification sent"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_failed_obligations() -> Result<()> {
    init_tracing();
    let client = create_mcp_client().await;
    let file_path = get_test_file_path();

    let result = call_tool(
        &client,
        "rust_analyzer_failed_obligations",
        serde_json::json!({ "file_path": file_path, "line": 45, "character": 5 }),
    )
    .await;

    assert!(result.contains("goal_index"));
    Ok(())
}
