use std::path::PathBuf;

use anyhow::Result;
use expect_test::expect;
use sacp::DynComponent;
use sacp_conductor::Conductor;
use symposium_rust_analyzer::RustAnalyzerProxy;

fn get_test_project_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test-project")
}

#[tokio::test]
async fn test_rust_analyzer_proxy_creation() -> Result<()> {
    let test_project = get_test_project_path();
    let proxy = RustAnalyzerProxy {
        workspace_path: Some(test_project.display().to_string()),
    };

    let result = yopo::prompt(
        Conductor::new(
            "test-conductor".to_string(),
            vec![
                DynComponent::new(proxy),
                DynComponent::new(elizacp::ElizaAgent::new()),
            ],
            Default::default(),
        ),
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
async fn test_analyzer_workspace_diagnostics() -> Result<()> {
    let test_project = get_test_project_path();
    let proxy = RustAnalyzerProxy {
        workspace_path: Some(test_project.display().to_string()),
    };

    let conductor = Conductor::new(
        "test-conductor".to_string(),
        vec![
            DynComponent::new(proxy),
            DynComponent::new(elizacp::ElizaAgent::new()),
        ],
        Default::default(),
    );
    let result = yopo::prompt(
        conductor,
        &format!(r#"Use tool rust-analyzer-mcp::rust_analyzer_workspace_diagnostics with {{}}"#,),
    )
    .await?;

    expect![[r#"OK: CallToolResult { content: [Annotated { raw: Text(RawTextContent { text: "\"[]\"", meta: None }), annotations: None }], structured_content: Some(String("[]")), is_error: Some(false), meta: None }"#]].assert_eq(&result);

    Ok(())
}
