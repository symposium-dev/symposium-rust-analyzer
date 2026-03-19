use anyhow::Result;
use pico_args::Arguments;
use sacp::{ByteStreams, ConnectTo};
use symposium_rust_analyzer::{RustAnalyzerProxy, build_server};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Run the proxy as a standalone binary connected to stdio
async fn run_proxy() -> Result<()> {
    RustAnalyzerProxy::default()
        .connect_to(sacp_tokio::Stdio::new())
        .await?;

    Ok(())
}

pub async fn run_mcp() -> Result<()> {
    let mcp = build_server(None).await?;
    let stido = ByteStreams::new(
        tokio::io::stdout().compat_write(),
        tokio::io::stdin().compat(),
    );
    mcp.connect_to(stido).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .init();

    let mut args = Arguments::from_env();
    let mcp = args.contains("--mcp");
    let proxy = args.contains("--proxy");

    match (mcp, proxy) {
        (true, false) => run_mcp().await,
        (false, true) => run_proxy().await,
        _ => {
            eprintln!("Usage: symposium-cargo [--mcp | --proxy]");
            std::process::exit(1);
        }
    }
}
