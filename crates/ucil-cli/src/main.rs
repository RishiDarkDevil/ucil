//! `ucil` CLI entry point.
//!
//! Phase 0 skeleton — parses nothing yet, exits cleanly.
//! Full command implementation begins in Phase 0 Week 1 (F03–F06).

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!(version = ucil_core::VERSION, "ucil CLI starting");
    Ok(())
}
