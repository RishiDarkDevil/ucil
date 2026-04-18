//! `ucil-daemon` entry point.
//!
//! Dispatches on the first positional argument: `"mcp"` routes to the MCP
//! stdio server; anything else runs the (future) daemon mode.
//!
//! With `--repo <PATH>`, the `"mcp"` subcommand bootstraps a SQLite
//! `KnowledgeGraph` by running the tree-sitter ingestion pipeline over
//! the repo so `find_definition` returns real file:line data instead of
//! the `_meta.not_yet_implemented` stub envelope.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use ucil_core::KnowledgeGraph;
use ucil_daemon::executor::IngestPipeline;

/// Extensions fed to `IngestPipeline::ingest_file` for the one-shot
/// `--repo` bootstrap.  Mirrors `ucil_treesitter`'s language coverage
/// from `P1-W2-F01` (see `executor.rs::language_from_extension`).
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "go", "java", "c", "cpp", "cc", "h", "hpp",
];

/// Directory basenames skipped at any depth during repo discovery.
///
/// Matching is case-sensitive and keyed on the directory's own file
/// name, not its full path — so a nested `foo/target/bar.rs` is still
/// excluded.  Dotfiles at the repo root are filtered separately.
const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    ".ucil",
    "dist",
    "build",
    "venv",
    ".venv",
    "__pycache__",
];

/// Hard cap on files returned by [`walk_supported_source_files`].
///
/// Defensive guard so a gigantic monorepo does not hang the stdio
/// transport's cold-start; once reached, the walker stops recursing.
const WALK_FILE_CAP: usize = 50_000;

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

/// Recursively enumerate source files under `root` that
/// `IngestPipeline::ingest_file` can process.
///
/// * Skips directories whose basename matches [`SKIP_DIRS`] at any depth.
/// * Skips entries whose basename starts with `.` (dotfiles / dotdirs)
///   so hidden VCS or editor caches do not pollute the KG.
/// * Filters to extensions in [`SUPPORTED_EXTENSIONS`] (lowercased).
/// * Output is sorted by `PathBuf` for deterministic ingest order —
///   downstream KG upserts use `ON CONFLICT` so stable ordering keeps
///   test fixtures reproducible.
/// * Capped at [`WALK_FILE_CAP`] files; once the cap is reached the
///   walker returns what it has and logs a `warn!`.
fn walk_supported_source_files(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut count: usize = 0;
    visit(root, &mut out, &mut count);
    out.sort();
    out
}

fn visit(dir: &Path, out: &mut Vec<PathBuf>, count: &mut usize) {
    if *count >= WALK_FILE_CAP {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(e) => {
            tracing::warn!(dir = %dir.display(), error = %e, "read_dir failed — skipping");
            return;
        }
    };
    for entry in entries.flatten() {
        if *count >= WALK_FILE_CAP {
            tracing::warn!(
                cap = WALK_FILE_CAP,
                "file discovery capped — repo too large for one-shot ingest"
            );
            return;
        }
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            visit(&path, out, count);
        } else if file_type.is_file() {
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase);
            if let Some(ext) = ext {
                if SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
                    out.push(path);
                    *count += 1;
                }
            }
        }
    }
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

            // `_tmp` is bound to the match arm so the SQLite file +
            // WAL + lock survive for the full `serve` loop.
            let (server, _tmp) = if let Some(repo) = repo_dir.as_deref() {
                let tmp = tempfile::TempDir::new()
                    .context("ucil-daemon mcp --stdio: failed to create temp KG dir")?;
                let kg_path = tmp.path().join("knowledge.db");
                let mut kg = KnowledgeGraph::open(&kg_path)
                    .context("ucil-daemon mcp --stdio: failed to open KnowledgeGraph")?;
                let mut pipeline = IngestPipeline::new();
                let files = walk_supported_source_files(repo);
                let mut ingested: usize = 0;
                for file in &files {
                    match pipeline.ingest_file(&mut kg, file) {
                        Ok(_) => ingested += 1,
                        Err(e) => {
                            tracing::warn!(
                                file = %file.display(),
                                error = %e,
                                "ingest_file failed — skipping",
                            );
                        }
                    }
                }
                tracing::info!(
                    repo = %repo.display(),
                    discovered = files.len(),
                    ingested,
                    "ucil-daemon mcp --stdio bootstrap complete",
                );
                let kg_arc = Arc::new(Mutex::new(kg));
                (
                    ucil_daemon::server::McpServer::with_knowledge_graph(kg_arc),
                    Some(tmp),
                )
            } else {
                (ucil_daemon::server::McpServer::new(), None)
            };

            server
                .serve(tokio::io::stdin(), tokio::io::stdout())
                .await
                .context("ucil-daemon mcp --stdio: serve loop terminated with error")?;
            drop(_tmp);
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
    use std::fs;

    use tempfile::tempdir;

    use super::{parse_repo_arg, walk_supported_source_files, SKIP_DIRS, SUPPORTED_EXTENSIONS};

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

    #[test]
    fn walk_supported_source_files_filters_extensions_and_skips_dirs() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();

        // Supported files at various depths.
        fs::write(root.join("a.rs"), "pub fn a() {}").unwrap();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/b.py"), "def b(): pass").unwrap();
        fs::write(root.join("src/nested/c.ts"), "export const c = 1;").unwrap();

        // Skipped directory at nested depth.
        fs::create_dir_all(root.join("src/target")).unwrap();
        fs::write(root.join("src/target/bad.rs"), "pub fn bad() {}").unwrap();

        // Skipped dotdir.
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/config"), "...").unwrap();

        // Skipped dotfile at root.
        fs::write(root.join(".hidden.rs"), "pub fn hidden() {}").unwrap();

        // Unsupported extension.
        fs::write(root.join("readme.md"), "# readme").unwrap();

        let got = walk_supported_source_files(root);
        let names: Vec<_> = got
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect();

        // Output is sorted and contains only the expected entries.
        assert_eq!(got.len(), 3, "unexpected files: {names:?}");
        assert!(names.iter().any(|n| n.ends_with("a.rs")));
        assert!(names.iter().any(|n| n.ends_with("b.py")));
        assert!(names.iter().any(|n| n.ends_with("c.ts")));

        // Determinism — sort is stable.
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(got, sorted);
    }

    #[test]
    fn walk_supported_source_files_empty_on_missing_root() {
        let dir = tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist");
        let got = walk_supported_source_files(&missing);
        assert!(got.is_empty());
    }

    #[test]
    fn supported_extensions_not_empty_and_match_executor_coverage() {
        // Sanity: these extensions are the intersection of what the
        // walker surfaces and what executor.rs::language_from_extension
        // accepts.  If the table ever shrinks to an empty slice, the
        // --repo code path becomes a no-op without any test noticing.
        assert!(!SUPPORTED_EXTENSIONS.is_empty());
        for ext in ["rs", "py", "ts", "tsx", "js", "jsx", "go", "java"] {
            assert!(
                SUPPORTED_EXTENSIONS.contains(&ext),
                "extension table missing {ext:?}"
            );
        }
    }

    #[test]
    fn skip_dirs_covers_expected_ignores() {
        // Sanity: the vendor / build / VCS dirs every monorepo ships.
        for d in ["target", "node_modules", ".git", ".ucil"] {
            assert!(SKIP_DIRS.contains(&d), "skip-list missing {d:?}");
        }
    }
}
