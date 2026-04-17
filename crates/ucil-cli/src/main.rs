//! `ucil` CLI entry point.

mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ucil",
    about = "Universal Code Intelligence Layer — CLI",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialise a `.ucil/` directory in the current project.
    Init(commands::init::InitArgs),
    /// Plugin management: `install <name>`, etc.
    Plugin(commands::plugin::PluginArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Commands::Init(args) => commands::init::run(args).await,
        Commands::Plugin(args) => commands::plugin::run(args).await,
    }
}
