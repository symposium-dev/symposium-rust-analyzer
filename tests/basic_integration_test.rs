use std::path::PathBuf;

use anyhow::Result;
use expect_test::expect;
use sacp::DynComponent;
use sacp_conductor::Conductor;
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

async fn create_conductor() -> Conductor {
    init_tracing();
    let test_project = get_test_project_path();
    let proxy = RustAnalyzerProxy {
        workspace_path: Some(test_project.display().to_string()),
    };

    Conductor::new(
        "test-conductor".to_string(),
        vec![
            DynComponent::new(proxy),
            DynComponent::new(elizacp::ElizaAgent::new()),
        ],
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

    expect![[r#"OK: CallToolResult { content: [Annotated { raw: Text(RawTextContent { text: "\"Workspace set successfully\"", meta: None }), annotations: None }], structured_content: Some(String("Workspace set successfully")), is_error: Some(false), meta: None }"#]].assert_eq(&result);
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_hover() -> Result<()> {
    tracing::debug!("Starting test_rust_analyzer_hover");
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    // Test hovering over the Person struct name
    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_hover with {{ "file_path": "{}", "line": 120, "character": 11 }}"#,
            file_path
        ),
    )
    .await?;

    dbg!(&result);
    assert!(result.contains("CallToolResult"));
    assert!(false);
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_definition() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    // Test going to definition of HashMap
    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_definition with {{ "file_path": "{}", "line": 0, "character": 25 }}"#,
            file_path
        ),
    )
    .await?;

    dbg!(&result);
    assert!(result.contains("CallToolResult"));
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

    dbg!(&result);
    assert!(result.contains("CallToolResult"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_completion() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    // Test completion after "user." in main function
    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_completion with {{ "file_path": "{}", "line": 85, "character": 30 }}"#,
            file_path
        ),
    )
    .await?;

    dbg!(&result);
    assert!(result.contains("CallToolResult"));
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

    dbg!(&result);
    assert!(result.contains("CallToolResult"));
    assert!(false);
    Ok(())
}

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

    dbg!(&result);
    assert!(result.contains("CallToolResult"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_code_actions() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    // Test code actions on the error_function() call
    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_code_actions with {{ "file_path": "{}", "line": 104, "character": 4, "end_line": 104, "end_character": 20 }}"#,
            file_path
        ),
    )
    .await?;

    dbg!(&result);
    assert!(result.contains("CallToolResult"));
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
    assert!(result.contains("CallToolResult"));
    // Should contain diagnostics about the undefined error_function
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_failed_obligations() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    // Test failed obligations on the error_function call
    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_failed_obligations with {{ "file_path": "{}", "line": 104, "character": 4 }}"#,
            file_path
        ),
    )
    .await?;

    dbg!(&result);
    assert!(result.contains("CallToolResult"));
    Ok(())
}

#[tokio::test]
async fn test_direct_bridge_hover() -> Result<()> {
    use lsp_types::Position;
    use std::sync::Arc;
    use symposium_rust_analyzer::{BridgeState, BridgeType, SERVER_ID, with_bridge_and_document};
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
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let position = Position::new(3, 11); // Person struct
            let hover_result = lsp
                .get_hover(SERVER_ID, &uri, position)
                .await
                .map_err(|e| anyhow::anyhow!("Hover request failed: {}", e))?;
            dbg!(&hover_result);
            Ok(serde_json::to_string(&hover_result)?)
        },
    )
    .await?;

    dbg!(&result);
    assert!(!result.is_empty());
    Ok(())
}
