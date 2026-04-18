//! In-process `ripgrep` text-search helper — backs the text half of the
//! `search_code` MCP tool (`P1-W5-F09`, master-plan §3.2 row 4 / §18
//! Phase 1 Week 5 line 1765).
//!
//! `DEC-0009-search-code-in-process-ripgrep` picks the `ignore` +
//! `grep-searcher` + `grep-regex` + `grep-matcher` crates over shelling
//! out to the `rg` binary: no system-binary dependency (phase-1
//! invariant #2 spirit), the anti-laziness contract's "no mocked
//! critical deps" rule is satisfied by composition, and the libraries
//! *are* the libraries `rg` itself composes, so behaviour is identical
//! by construction.
//!
//! Public surface (all `pub`, but the module is declared
//! `pub(crate) mod text_search;` in `lib.rs` so the effective
//! visibility is still crate-local — the `pub` inside the module
//! avoids the `clippy::redundant_pub_crate` lint):
//!
//! * [`TextMatch`] — a `{file_path, line_number, line_text}` record.
//! * [`TextSearchError`] — regex-build, I/O, and walker errors.
//! * [`text_search`] — walks `root` respecting `.gitignore`, streams
//!   per-file matches via [`grep_searcher::Searcher::search_path`], and
//!   returns up to `max_results` rows.

use std::{
    path::{Path, PathBuf},
    str,
};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;
use thiserror::Error;

/// One match hit emitted by [`text_search`].
///
/// `line_number` is 1-indexed to match `ripgrep` and `grep -n` output
/// (the [`SearcherBuilder::line_number`] flag is enabled in
/// [`text_search`]).  `line_text` is the matching line with its
/// terminator stripped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMatch {
    /// Absolute path to the file that contained the match.
    pub file_path: PathBuf,
    /// 1-indexed line number at which the match was found.
    pub line_number: u64,
    /// The matching line, trimmed of any trailing `\r` / `\n` bytes.
    pub line_text: String,
}

/// Errors produced by [`text_search`].
///
/// * [`TextSearchError::BuildMatcher`] — the caller's query failed to
///   compile as a regex.  Propagated from [`RegexMatcherBuilder::build`].
/// * [`TextSearchError::Io`] — a file I/O failure surfaced by the
///   grep searcher's sink or the walker.
/// * [`TextSearchError::Walk`] — the [`ignore::Walk`] iterator yielded a
///   non-I/O error (permission denied on a directory, broken symlink
///   loop, etc.).  Stringified so the caller does not need to depend on
///   [`ignore::Error`] directly.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TextSearchError {
    /// Compiling the caller-supplied query as a regex failed.
    #[error("text_search: failed to build regex matcher: {0}")]
    BuildMatcher(#[from] grep_regex::Error),
    /// A filesystem read (walker or sink) surfaced an I/O error.
    #[error("text_search: i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The `ignore::Walk` iterator reported a non-I/O failure.
    #[error("text_search: walker error: {0}")]
    Walk(String),
}

/// Hard cap on [`TextMatch::line_text`] length (bytes).
///
/// Long minified-JS / minified-JSON lines would otherwise pin an
/// entire megabyte+ line into the response payload.  The cap is far
/// beyond any realistic source-code line (1024 bytes ≈ 15 normal
/// 80-col lines worth) and keeps memory bounded when a regex happens
/// to hit a pathological line.
const MAX_LINE_LEN: usize = 1024;

/// Walk `root` and return every matching line across every regular
/// file, respecting `.gitignore`, `.ignore`, and hidden-file rules.
///
/// See [`DEC-0009-search-code-in-process-ripgrep`] (in
/// `ucil-build/decisions/`) for the rationale behind the in-process
/// composition vs a `tokio::process::Command` spawn of `rg`.
///
/// # Behaviour
///
/// * The matcher is built with [`RegexMatcherBuilder::case_smart`]
///   enabled — uppercase in the query makes the match case-sensitive,
///   all-lowercase keeps it case-insensitive (matches `rg`'s
///   `--smart-case` default).
/// * The walker defaults respect `.gitignore`, skip hidden files, and
///   break symlink loops — identical to `rg`'s defaults.
/// * Binary files are detected via the NUL-byte heuristic
///   ([`BinaryDetection::quit`]) and skipped mid-search; non-UTF8
///   lines are replaced lossily so a stray invalid byte does not abort
///   the whole scan.
/// * Search halts as soon as `max_results` rows are collected — the
///   sink returns `Ok(false)` from `matched` to signal the searcher to
///   stop, per the [`Sink`] contract.
///
/// # Errors
///
/// * [`TextSearchError::BuildMatcher`] — `query` is not a valid regex.
/// * [`TextSearchError::Walk`] — the `ignore` walker yielded a fatal
///   error.  Per-file I/O failures are logged via `tracing::warn` and
///   skipped, so a single unreadable file does not abort the scan.
#[tracing::instrument(
    level = "debug",
    skip(root),
    fields(root = %root.display(), query_len = query.len(), max_results),
    name = "ucil.daemon.text_search",
)]
pub fn text_search(
    root: &Path,
    query: &str,
    max_results: usize,
) -> Result<Vec<TextMatch>, TextSearchError> {
    let matcher = RegexMatcherBuilder::new().case_smart(true).build(query)?;

    let mut out: Vec<TextMatch> = Vec::new();
    if max_results == 0 {
        return Ok(out);
    }

    if !root.exists() {
        return Err(TextSearchError::Walk(format!(
            "root does not exist: {}",
            root.display(),
        )));
    }
    if !root.is_dir() {
        return Err(TextSearchError::Walk(format!(
            "root is not a directory: {}",
            root.display(),
        )));
    }

    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(b'\0'))
        .build();

    // `require_git(false)` so `.gitignore` / `.ignore` files are
    // respected even when the target tree is not itself a git
    // checkout — matches `rg`'s behaviour when invoked from a random
    // subdirectory, and is what makes the unit tests (tempdir without
    // `.git/`) honour the fixture's `.gitignore`.
    let walker = WalkBuilder::new(root).require_git(false).build();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(walker_error = %e, "text_search: walker yielded error, skipping");
                continue;
            }
        };

        // Skip directories and anything that is not a regular file
        // (symlinks the walker already resolved, sockets, fifos, ...).
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let path = entry.path();

        let remaining = max_results - out.len();
        let mut sink = CollectSink {
            path: path.to_path_buf(),
            hits: &mut out,
            remaining,
        };
        if let Err(e) = searcher.search_path(&matcher, path, &mut sink) {
            // Per-file I/O failures are non-fatal: log + skip so a
            // single unreadable file (permissions, mid-walk unlink)
            // cannot abort the whole scan.
            tracing::warn!(path = %path.display(), search_error = %e, "text_search: per-file search failed, skipping");
            continue;
        }

        if out.len() >= max_results {
            break;
        }
    }

    Ok(out)
}

/// [`Sink`] that pushes [`TextMatch`] rows into a borrowed vector and
/// signals the searcher to stop once the per-call `remaining` quota
/// hits zero.
struct CollectSink<'v> {
    /// Absolute path of the file currently being searched — copied into
    /// each produced [`TextMatch`] so callers can group / dedupe
    /// results by file.
    path: PathBuf,
    /// Output buffer shared across every per-file sink invocation.
    hits: &'v mut Vec<TextMatch>,
    /// Rows this sink instance may still push before signalling stop.
    remaining: usize,
}

impl Sink for CollectSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.remaining == 0 {
            return Ok(false);
        }

        let line_number = mat.line_number().unwrap_or(0);
        let bytes = mat.bytes();
        let trimmed = trim_line_terminator(bytes);
        let capped_len = trimmed.len().min(MAX_LINE_LEN);
        let capped = &trimmed[..capped_len];
        let line_text = String::from_utf8_lossy(capped).into_owned();

        self.hits.push(TextMatch {
            file_path: self.path.clone(),
            line_number,
            line_text,
        });
        self.remaining -= 1;
        Ok(self.remaining > 0)
    }
}

/// Strip a single trailing `\n` or `\r\n` from a matching-line byte
/// slice.  `grep_searcher` yields lines *with* their terminator so
/// the downstream JSON envelope needs the raw text without newlines.
fn trim_line_terminator(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    if end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
        if end > 0 && bytes[end - 1] == b'\r' {
            end -= 1;
        }
    }
    &bytes[..end]
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use tempfile::TempDir;

    use super::{text_search, TextMatch};

    /// Write the two-file + .gitignore + ignored-file tree used by the
    /// happy-path acceptance tests.  Returns the tempdir so the caller
    /// controls the drop point.
    fn write_fixture_tree(needle: &str) -> TempDir {
        let tmp = TempDir::new().expect("tempdir must be creatable");
        let root = tmp.path();

        let a_rs = root.join("a.rs");
        fs::write(
            &a_rs,
            format!("fn {needle}() {{ /* marker */ }}\nfn other() {{}}\n"),
        )
        .expect("write a.rs");

        let b_txt = root.join("b.txt");
        fs::write(&b_txt, format!("foo {needle} bar\nanother line\n")).expect("write b.txt");

        let gi = root.join(".gitignore");
        fs::write(&gi, "skip.log\n").expect("write .gitignore");

        let skip = root.join("skip.log");
        fs::write(&skip, format!("{needle} should not match\n")).expect("write skip.log");

        tmp
    }

    #[test]
    fn respects_gitignore_and_finds_hits() {
        let tmp = write_fixture_tree("hello_world");
        let hits = text_search(tmp.path(), "hello_world", 50).expect("text_search must succeed");

        let matched_paths: Vec<String> = hits
            .iter()
            .map(|m| {
                m.file_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_owned()
            })
            .collect();

        assert!(
            matched_paths.iter().any(|p| p == "a.rs"),
            "a.rs must be in hits: {matched_paths:?}",
        );
        assert!(
            matched_paths.iter().any(|p| p == "b.txt"),
            "b.txt must be in hits: {matched_paths:?}",
        );
        assert!(
            !matched_paths.iter().any(|p| p == "skip.log"),
            ".gitignore-ignored skip.log must NOT be in hits: {matched_paths:?}",
        );
    }

    #[test]
    fn line_numbers_are_1_indexed() {
        let tmp = write_fixture_tree("hello_world");
        let hits = text_search(tmp.path(), "hello_world", 50).expect("text_search must succeed");

        let a_rs_hit: &TextMatch = hits
            .iter()
            .find(|m| m.file_path.file_name().and_then(|s| s.to_str()) == Some("a.rs"))
            .expect("a.rs must appear in hits");
        // The needle is on line 1 of a.rs.
        assert_eq!(a_rs_hit.line_number, 1, "a.rs match must be line 1");

        let b_txt_hit: &TextMatch = hits
            .iter()
            .find(|m| m.file_path.file_name().and_then(|s| s.to_str()) == Some("b.txt"))
            .expect("b.txt must appear in hits");
        // The needle is on line 1 of b.txt (first line).
        assert_eq!(b_txt_hit.line_number, 1, "b.txt match must be line 1");
    }

    #[test]
    fn max_results_caps_output() {
        let tmp = write_fixture_tree("hello_world");
        let hits = text_search(tmp.path(), "hello_world", 1).expect("text_search must succeed");
        assert_eq!(
            hits.len(),
            1,
            "max_results=1 must cap the scan to a single hit",
        );
    }

    #[test]
    fn zero_max_results_returns_empty() {
        let tmp = write_fixture_tree("hello_world");
        let hits = text_search(tmp.path(), "hello_world", 0).expect("text_search must succeed");
        assert!(hits.is_empty(), "max_results=0 must return empty");
    }

    #[test]
    fn invalid_regex_returns_build_matcher_error() {
        let tmp = TempDir::new().expect("tempdir must be creatable");
        let err = text_search(tmp.path(), "foo[", 10)
            .expect_err("unterminated character class must fail at build time");
        assert!(
            matches!(err, super::TextSearchError::BuildMatcher(_)),
            "must be BuildMatcher variant, got: {err:?}",
        );
    }

    #[test]
    fn empty_result_is_empty_vec_not_error() {
        let tmp = TempDir::new().expect("tempdir must be creatable");
        let only = tmp.path().join("only.rs");
        let mut f = fs::File::create(&only).expect("create only.rs");
        f.write_all(b"nothing matches here\n").expect("write");

        let hits = text_search(tmp.path(), "nonexistent_needle_xyz", 10)
            .expect("empty-result must succeed");
        assert!(hits.is_empty(), "no matches yields empty vec");
    }
}
