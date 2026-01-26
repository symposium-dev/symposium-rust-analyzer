use std::path::PathBuf;

use anyhow::Result;
use sacp::link::ConductorToClient;
use sacp_conductor::{Conductor, ProxiesAndAgent};
use symposium_rust_analyzer::RustAnalyzerProxy;

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

async fn create_conductor() -> Conductor<ConductorToClient> {
    init_tracing();
    let test_project = get_test_project_path();
    let proxy = RustAnalyzerProxy {
        workspace_path: Some(test_project.display().to_string()),
    };

    Conductor::new_agent(
        "test-conductor".to_string(),
        ProxiesAndAgent::new(elizacp::ElizaAgent::new()).proxy(proxy),
        Default::default(),
    )
}

#[tokio::test]
async fn test_rust_analyzer_set_workspace() -> Result<()> {
    let test_project = get_test_project_path();
    let conductor = create_conductor().await;

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_set_workspace with {{ "workspace_path": "{}" }}"#,
            test_project.display()
        ),
    )
    .await?;

    assert!(result.contains("Workspace set successfully"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_hover() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_hover with {{ "file_path": "{}", "line": 3, "character": 11 }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("name: String"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_definition() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_definition with {{ "file_path": "{}", "line": 0, "character": 25 }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("src/collections/hash/map.rs"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_references() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    // Test finding references to Person struct
    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_references with {{ "file_path": "{}", "line": 3, "character": 11 }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("test-project/src/main.rs"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_completion() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_completion with {{ "file_path": "{}", "line": 99, "character": 29 }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("greet"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_symbols() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_symbols with {{ "file_path": "{}" }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("Person"));
    Ok(())
}

/*
#[tokio::test]
async fn test_rust_analyzer_format() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_format with {{ "file_path": "{}" }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("is_error: Some(false)"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_code_actions() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_code_actions with {{ "file_path": "{}", "line": 104, "character": 4, "end_line": 104, "end_character": 20 }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("structured_content"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_diagnostics() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_diagnostics with {{ "file_path": "{}" }}"#,
            file_path
        ),
    )
    .await?;

    dbg!(&result);
    assert!(result.contains("structured_content"));
    Ok(())
}
*/

#[tokio::test]
async fn test_rust_analyzer_lsp_call_notification() -> Result<()> {
    let conductor = create_conductor().await;
    let test_project = get_test_project_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_lsp_call with {{ "method": "window/logMessage", "params": {{ "type": 1, "message": "hello from test" }}, "is_notification": true, "workspace_path": "{}" }}"#,
            test_project.display()
        ),
    )
    .await?;

    assert!(result.contains("Notification sent"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_failed_obligations() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_failed_obligations with {{ "file_path": "{}", "line": 45, "character": 5 }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("goal_index"));
    Ok(())
}

#[tokio::test]
async fn test_direct_bridge_hover() -> Result<()> {
    use lsp_types::Position;
    use std::sync::Arc;
    use symposium_rust_analyzer::{BridgeState, BridgeType, with_bridge_and_document};
    use tokio::sync::Mutex;

    init_tracing();
    let test_project = get_test_project_path();
    let file_path = get_test_file_path();

    let bridge: BridgeType = Arc::new(Mutex::new(BridgeState::new()));

    let result = with_bridge_and_document(
        &bridge,
        Some(&test_project.display().to_string()),
        &file_path,
        async move |lsp, uri| {
            let position = Position::new(3, 11); // Person struct
            let hover_result = lsp
                .hover(uri, position)
                .await
                .map_err(|e| anyhow::anyhow!("Hover request failed: {}", e))?;
            Ok(serde_json::to_string(&hover_result)?)
        },
    )
    .await?;

    assert!(result.contains("contents"));
    Ok(())
}
