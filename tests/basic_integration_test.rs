use std::path::PathBuf;

use anyhow::Result;
use expect_test::expect;
use sacp::DynComponent;
use sacp_conductor::Conductor;
use symposium_rust_analyzer::RustAnalyzerProxy;

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
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_hover with {{ "file_path": "{}", "line": 0, "character": 0 }}"#,
            file_path
        ),
    )
    .await?;

    // Should return some hover result (even if null)
    assert!(result.contains("CallToolResult"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_definition() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_definition with {{ "file_path": "{}", "line": 0, "character": 3 }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("CallToolResult"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_references() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_references with {{ "file_path": "{}", "line": 0, "character": 3 }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("CallToolResult"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_completion() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_completion with {{ "file_path": "{}", "line": 1, "character": 4 }}"#,
            file_path
        ),
    )
    .await?;

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

    assert!(result.contains("CallToolResult"));
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

    assert!(result.contains("CallToolResult"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_code_actions() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_code_actions with {{ "file_path": "{}", "line": 1, "character": 4, "end_line": 1, "end_character": 9 }}"#,
            file_path
        ),
    )
    .await?;

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

    assert!(result.contains("CallToolResult"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_workspace_diagnostics() -> Result<()> {
    let conductor = create_conductor().await;

    let result = yopo::prompt(
        conductor,
        r#"Use tool rust-analyzer-mcp::rust_analyzer_workspace_diagnostics with {}"#,
    )
    .await?;

    assert!(result.contains("CallToolResult"));
    assert!(result.contains("workspace"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_failed_obligations() -> Result<()> {
    let conductor = create_conductor().await;
    let file_path = get_test_file_path();

    let result = yopo::prompt(
        conductor,
        &format!(
            r#"Use tool rust-analyzer-mcp::rust_analyzer_failed_obligations with {{ "file_path": "{}", "line": 1, "character": 4 }}"#,
            file_path
        ),
    )
    .await?;

    assert!(result.contains("CallToolResult"));
    assert!(result.contains("debug_info"));
    Ok(())
}

#[tokio::test]
async fn test_rust_analyzer_failed_obligations_goal() -> Result<()> {
    let conductor = create_conductor().await;

    let result = yopo::prompt(
        conductor,
        r#"Use tool rust-analyzer-mcp::rust_analyzer_failed_obligations_goal with { "goal_index": "test_goal" }"#,
    )
    .await?;

    assert!(result.contains("CallToolResult"));
    assert!(result.contains("error"));
    Ok(())
}
