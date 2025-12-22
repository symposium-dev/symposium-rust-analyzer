use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    symposium_rust_analyzer::run().await
}
