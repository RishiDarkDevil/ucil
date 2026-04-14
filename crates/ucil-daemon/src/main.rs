//! `ucil-daemon` entry point.
//!
//! Phase 0 skeleton — starts up and exits cleanly.
//! Full daemon implementation begins in Phase 1 Week 3.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!(version = ucil_core::VERSION, "ucil-daemon starting");
    Ok(())
}
