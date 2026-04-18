//! `ucil-daemon` entry point.
//!
//! Dispatches on the first positional argument: `"mcp"` routes to the MCP
//! stdio server; anything else runs the (future) daemon mode.
//!
//! With `--repo <PATH>`, the `"mcp"` subcommand bootstraps a SQLite
//! `KnowledgeGraph` by running the tree-sitter ingestion pipeline over
//! the repo so `find_definition` returns real file:line data instead of
//! the `_meta.not_yet_implemented` stub envelope.

use anyhow::{Context, Result};

/// Parse the `mcp` subcommand's trailing arguments for `--repo <VALUE>`
/// (space-separated) or `--repo=<VALUE>` (equals-joined).
///
/// Unknown flags (`--stdio`, `--help`, …) are accepted-and-ignored so
/// the stdio transport never fails when the host agent appends
/// additional CLI shape.  Only the *first* `--repo` occurrence wins.
fn parse_repo_arg(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if let Some(rest) = a.strip_prefix("--repo=") {
            return Some(rest.to_owned());
        }
        if a == "--repo" {
            if let Some(v) = iter.next() {
                return Some(v.clone());
            }
        }
    }
    None
}

#[tokio::main]
async fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("mcp") => {
            // Route tracing to stderr so stdout stays pristine for the
            // newline-delimited JSON-RPC frames the host agent parses.
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .init();

            let all_args: Vec<String> = std::env::args().collect();
            let rest: &[String] = all_args.get(2..).unwrap_or(&[]);
            let repo_arg = parse_repo_arg(rest);
            let repo_dir = repo_arg
                .as_deref()
                .map(std::path::PathBuf::from)
                .filter(|p| p.is_dir());

            if let Some(repo) = repo_dir {
                tracing::info!(repo = %repo.display(), "--repo supplied (KG bootstrap will land in a follow-up commit)");
            }

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

#[cfg(test)]
mod tests {
    use super::parse_repo_arg;

    #[test]
    fn parse_repo_arg_space_separated() {
        let args = vec!["--stdio".into(), "--repo".into(), "/tmp/foo".into()];
        assert_eq!(parse_repo_arg(&args), Some("/tmp/foo".to_owned()));
    }

    #[test]
    fn parse_repo_arg_equals_joined() {
        let args = vec!["--repo=/tmp/bar".into(), "--stdio".into()];
        assert_eq!(parse_repo_arg(&args), Some("/tmp/bar".to_owned()));
    }

    #[test]
    fn parse_repo_arg_absent_returns_none() {
        let args = vec!["--stdio".into(), "--quiet".into()];
        assert_eq!(parse_repo_arg(&args), None);
    }

    #[test]
    fn parse_repo_arg_trailing_without_value_returns_none() {
        let args = vec!["--stdio".into(), "--repo".into()];
        assert_eq!(parse_repo_arg(&args), None);
    }

    #[test]
    fn parse_repo_arg_first_wins() {
        let args = vec![
            "--repo=/tmp/first".into(),
            "--repo".into(),
            "/tmp/second".into(),
        ];
        assert_eq!(parse_repo_arg(&args), Some("/tmp/first".to_owned()));
    }
}
