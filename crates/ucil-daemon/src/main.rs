//! `ucil-daemon` entry point.
//!
//! Dispatches on the first positional argument: `"mcp"` routes to the MCP
//! stdio server; anything else runs the (future) daemon mode.

use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("mcp") => {
            // Route tracing to stderr so stdout stays pristine for the
            // newline-delimited JSON-RPC frames the host agent parses.
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .init();
            ucil_daemon::server::McpServer::new()
                .serve(tokio::io::stdin(), tokio::io::stdout())
                .await
                .context("ucil-daemon mcp --stdio: serve loop terminated with error")?;
            Ok(())
        }
        _ => {
            tracing_subscriber::fmt::init();
            tracing::info!(version = ucil_core::VERSION, "ucil-daemon starting");
            Ok(())
        }
    }
}
