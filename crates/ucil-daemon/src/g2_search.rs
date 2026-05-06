//! G2 search providers for the `search_code` MCP tool.
//!
//! Master-plan §5.2 lines 447-461 defines the G2 search layer as a
//! parallel fan-out over Probe / ripgrep / `LanceDB` (Phase-2) plus
//! Zoekt / codedb (Phase-3) whose ranked outputs feed weighted RRF.
//! Master-plan §15.2 lines 1515-1518 names the span hierarchy
//! `ucil.group.search` (parent) → `ucil.tool.<engine>.search` /
//! `ucil.tool.lancedb.vector_search` (children).
//!
//! This module owns the UCIL-internal seam — the [`G2SourceProvider`]
//! async trait — plus three Phase-2 implementations:
//!
//! * [`RipgrepProvider`] — wraps the in-process `text_search` substrate
//!   from `DEC-0009` / WO-0035 (no subprocess; reuses the daemon's own
//!   regex engine).
//! * [`ProbeProvider`] — drives the stdio MCP plugin manifest at
//!   `plugins/search/probe/plugin.toml` via the
//!   [`crate::PluginManager::run_tools_call`] extension landed in this
//!   same WO per `DEC-0015` D2.
//! * [`LancedbProvider`] — filesystem-existence check on
//!   `StorageLayout::branch_vectors_dir(...)/code_chunks.lance/` per
//!   `DEC-0015` D3; returns empty results until P2-W8-F04 lands the
//!   indexing pipeline.  No subprocess and no `lancedb` crate dep —
//!   `tokio::fs` only.
//!
//! The [`G2SourceFactory`] keeps `McpServer` `Clone` cheap — it holds
//! two `PathBuf`s and `build()` returns three boxed providers in
//! `[Probe, Ripgrep, Lancedb]` order on every call.
//!
//! # Architectural references
//!
//! * `DEC-0008` §4 — UCIL-owned trait boundaries.  [`G2SourceProvider`]
//!   is the seam between the daemon and the per-engine subprocess
//!   clients; local-impl substitutes inside `#[cfg(test)]` are NOT
//!   critical-dep substitutes per WO-0048 lessons.
//! * `DEC-0009` — in-process ripgrep substrate ([`RipgrepProvider`]
//!   reuses [`crate::text_search::text_search`]).
//! * `DEC-0015` — three architectural decisions: D1 additive
//!   `_meta.g2_fused`, D2 `PluginManager::run_tools_call`, D3
//!   `LancedbProvider` empty-until-P2-W8-F04.
//! * `DEC-0016` — orphan-branch carve-out.  This module does NOT depend
//!   on `crate::branch_manager` (which lives only on the orphan branch
//!   `feat/WO-0053-lancedb-per-branch` per `DEC-0016`) —
//!   [`LancedbProvider`] constructs its `vectors_dir` field from
//!   [`crate::StorageLayout::branch_vectors_dir`]
//!   (`crates/ucil-daemon/src/storage.rs:230` on main).  When
//!   `DEC-0016` is closed via human merge of the orphan branch and
//!   P2-W8-F04 lands the `LanceDB` indexer, the [`LancedbProvider`]
//!   body is augmented to actually open the `lancedb::connect` handle —
//!   the trait signature is unchanged at that time.
//! * WO-0056 — the canonical RRF math via [`ucil_core::fuse_g2_rrf`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use ucil_core::{G2Hit, G2Source, G2SourceResults};

use crate::plugin_manager::{PluginError, PluginManager, PluginManifest};
use crate::text_search::{self, TextSearchError};

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors produced by [`G2SourceProvider::execute`].
///
/// One variant per provider lane plus a generic [`G2SearchError::Timeout`]
/// for the per-source deadline that the caller wraps around the future.
/// The outer caller (`handle_search_code`) drops `Err` and `Timeout`
/// silently — partial-results semantics matching `fuse_g1` from WO-0048.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum G2SearchError {
    /// The in-process `text_search` substrate failed.  Wraps
    /// [`crate::text_search::TextSearchError`] verbatim so the caller can
    /// log the underlying regex / IO / walker reason.
    #[error("g2_search: ripgrep provider failed: {0}")]
    Ripgrep(#[source] TextSearchError),
    /// The Probe stdio MCP plugin failed.  Wraps
    /// [`crate::plugin_manager::PluginError`] verbatim.
    #[error("g2_search: probe provider failed: {0}")]
    Probe(#[source] PluginError),
    /// A filesystem read against the `LanceDB` table directory failed.
    /// `LanceDB` queries are filesystem-only until P2-W8-F04 per
    /// `DEC-0015` D3, so the only IO surface is `tokio::fs::try_exists`
    /// / `read_dir`.
    #[error("g2_search: lancedb provider filesystem error: {0}")]
    Lancedb(#[source] std::io::Error),
    /// The per-source deadline expired (the caller wraps each provider
    /// in `tokio::time::timeout(G2_PER_SOURCE_DEADLINE, ...)`).
    #[error("g2_search: provider exceeded per-source deadline")]
    Timeout,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// UCIL-internal seam between the search-code handler and a single G2
/// engine implementation.
///
/// Each impl returns its own already-ranked [`G2SourceResults`] — the
/// caller (`handle_search_code`) collects them into a `Vec` and feeds
/// the vec to [`ucil_core::fuse_g2_rrf`].  Per `DEC-0008` §4, the trait
/// is owned by UCIL, not by the upstream engine — so substituting a
/// local impl in `#[cfg(test)]` is NOT a critical-dep substitute.
#[async_trait]
pub trait G2SourceProvider: Send + Sync {
    /// Identify the engine this provider drives.  Used by
    /// [`G2SourceResults::source`] so the fusion layer's
    /// `contributing_sources` and `per_source_ranks` stay accurate.
    fn source(&self) -> G2Source;

    /// Run the engine over `query` against `root`, returning at most
    /// `max_results` already-ranked hits in `hits[0]`-is-rank-1 order.
    ///
    /// # Errors
    ///
    /// Returns the engine-specific [`G2SearchError`] variant on failure;
    /// the caller drops errors silently (partial-results semantics).
    async fn execute(
        &self,
        query: &str,
        root: &Path,
        max_results: usize,
    ) -> Result<G2SourceResults, G2SearchError>;
}

// ── Ripgrep provider ─────────────────────────────────────────────────────────

/// In-process ripgrep G2 source per `DEC-0009`.
///
/// Stateless — wraps [`crate::text_search::text_search`] and projects
/// each [`crate::text_search::TextMatch`] into a [`G2Hit`].
///
/// `start_line == end_line` because ripgrep returns line-granular hits;
/// the snippet is the matched line content with terminators stripped.
#[derive(Debug, Default, Clone)]
pub struct RipgrepProvider;

impl RipgrepProvider {
    /// Construct a new stateless ripgrep provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl G2SourceProvider for RipgrepProvider {
    fn source(&self) -> G2Source {
        G2Source::Ripgrep
    }

    #[tracing::instrument(
        name = "ucil.tool.ripgrep.search",
        level = "debug",
        skip(self, root),
        fields(query_len = query.len(), max_results),
    )]
    async fn execute(
        &self,
        query: &str,
        root: &Path,
        max_results: usize,
    ) -> Result<G2SourceResults, G2SearchError> {
        let hits =
            text_search::text_search(root, query, max_results).map_err(G2SearchError::Ripgrep)?;
        let projected: Vec<G2Hit> = hits
            .into_iter()
            .map(|m| {
                let line = u32::try_from(m.line_number).unwrap_or(u32::MAX);
                G2Hit {
                    file_path: m.file_path,
                    start_line: line,
                    end_line: line,
                    snippet: m.line_text,
                    score: 1.0,
                }
            })
            .collect();
        Ok(G2SourceResults {
            source: G2Source::Ripgrep,
            hits: projected,
        })
    }
}

// ── Probe provider ───────────────────────────────────────────────────────────

/// Probe stdio MCP G2 source per `DEC-0015` D2.
///
/// Drives the manifest at `plugins/search/probe/plugin.toml` via
/// [`crate::PluginManager::run_tools_call`] (the new public method
/// landed in this WO).  Spawn-per-call shape: each [`Self::execute`]
/// invocation parses the manifest, spawns a fresh child, runs the MCP
/// `initialize` → `notifications/initialized` → `tools/call` sequence,
/// kills the child, and returns parsed hits.
///
/// # Probe `tools/call` schema (from upstream research at v0.6.0-rc315)
///
/// Required parameters per the MCP `tools/list` `inputSchema`:
///
/// * `path` (string) — absolute path to the directory to search.
/// * `query` (string) — `ElasticSearch`-style query.  The provider
///   passes the caller's verbatim query string; advanced syntax is the
///   caller's responsibility.
///
/// Optional parameters Probe advertises (`exact`, `strictElasticSyntax`,
/// `session`, `nextPage`, `lsp`) are NOT forwarded — the provider's
/// contract is the lowest-common-denominator search.
///
/// # Probe response shape
///
/// Probe returns its results as MCP `result.content[].text` blocks
/// containing markdown with embedded XML-style `<file path="...">`
/// blocks; each `<file>` block carries one or more `<lineno> <content>`
/// rows.  The provider parses these blocks into [`G2Hit`] rows in the
/// order Probe returned them — `hits[0]` is rank 1.
///
/// Note: Probe does NOT advertise a `maxResults` parameter; the
/// provider applies the caller's `max_results` cap by truncating the
/// parsed-hit vector after the parse pass.
///
/// # Forward-compat
///
/// If a future Probe release exposes structured output (`result.matches`)
/// or a `maxResults` input parameter, switch the parser to the
/// structured path and forward the cap natively — the trait signature
/// is unchanged at that time.
#[derive(Debug, Clone)]
pub struct ProbeProvider {
    /// Absolute path to the manifest file
    /// (`plugins/search/probe/plugin.toml`).
    manifest_path: PathBuf,
    /// Per-call timeout in milliseconds, threaded through to
    /// [`PluginManager::run_tools_call`].  Cold-cache `npx` fetch can
    /// take 30 s+ on first run, so the default mirrors the WO-0044
    /// `FIRST_RUN_TIMEOUT_MS` budget.
    timeout_ms: u64,
}

impl ProbeProvider {
    /// Construct a new provider that drives the manifest at
    /// `manifest_path` with the supplied per-call timeout.
    #[must_use]
    pub const fn new(manifest_path: PathBuf, timeout_ms: u64) -> Self {
        Self {
            manifest_path,
            timeout_ms,
        }
    }
}

#[async_trait]
impl G2SourceProvider for ProbeProvider {
    fn source(&self) -> G2Source {
        G2Source::Probe
    }

    #[tracing::instrument(
        name = "ucil.tool.probe.search",
        level = "debug",
        skip(self, root),
        fields(query_len = query.len(), max_results),
    )]
    async fn execute(
        &self,
        query: &str,
        root: &Path,
        max_results: usize,
    ) -> Result<G2SourceResults, G2SearchError> {
        let manifest =
            PluginManifest::from_path(&self.manifest_path).map_err(G2SearchError::Probe)?;
        let arguments = serde_json::json!({
            "query": query,
            "path": root.display().to_string(),
        });
        let result =
            PluginManager::run_tools_call(&manifest, "search_code", &arguments, self.timeout_ms)
                .await
                .map_err(G2SearchError::Probe)?;
        let hits = parse_probe_response(&result, max_results);
        Ok(G2SourceResults {
            source: G2Source::Probe,
            hits,
        })
    }
}

/// Parse a Probe `tools/call` response envelope (`result` field) into
/// a vector of [`G2Hit`] rows in the order Probe returned them.
///
/// Probe's response format is `result.content[].text` markdown with
/// embedded XML-style `<file path="...">` blocks.  Each `<file>` block
/// contains lines of the form `<spaces><line_no> <content>`.  The
/// parser is permissive: malformed lines and missing fields are skipped
/// so a single bad row does not abort the whole parse.  The cap
/// `max_results` is applied via truncation after the parse pass — Probe
/// does not advertise a native `maxResults` input parameter as of
/// v0.6.0-rc315.
fn parse_probe_response(result: &serde_json::Value, max_results: usize) -> Vec<G2Hit> {
    let Some(content) = result.get("content").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut hits: Vec<G2Hit> = Vec::new();
    for entry in content {
        let Some(text) = entry.get("text").and_then(|v| v.as_str()) else {
            continue;
        };
        parse_probe_text_block(text, &mut hits, max_results);
        if hits.len() >= max_results {
            break;
        }
    }
    hits.truncate(max_results);
    hits
}

/// Parse a single Probe `<text>` block, pushing each detected line hit
/// onto `out`.  Stops appending once `out.len() >= max_results`.
///
/// Format (verified against Probe v0.6.0-rc315):
///
/// ```text
/// <file path="/abs/path/util.rs">
///
///    1 fn foo() { let x = 42; }
///   23 fn bar(...)
/// </file>
/// ```
///
/// The parser walks every line, tracks the most recent `<file>` opener,
/// and emits one [`G2Hit`] per `<lineno> <content>` row inside an open
/// block.  `<file>` closers (`</file>`) clear the current path so the
/// next block can re-open with a different path.
fn parse_probe_text_block(text: &str, out: &mut Vec<G2Hit>, max_results: usize) {
    let mut current_path: Option<PathBuf> = None;
    for line in text.lines() {
        if out.len() >= max_results {
            break;
        }
        let trimmed = line.trim_start();
        if let Some(path) = parse_file_open_tag(trimmed) {
            current_path = Some(path);
            continue;
        }
        if trimmed.starts_with("</file>") {
            current_path = None;
            continue;
        }
        let Some(path) = current_path.as_ref() else {
            continue;
        };
        if let Some((line_no, content)) = parse_numbered_line(line) {
            out.push(G2Hit {
                file_path: path.clone(),
                start_line: line_no,
                end_line: line_no,
                snippet: content.to_owned(),
                score: 1.0,
            });
        }
    }
}

/// Extract `path=...` from a `<file path="...">` opener, or `None` if
/// the line is not a `<file ...>` tag.
fn parse_file_open_tag(line: &str) -> Option<PathBuf> {
    let inner = line.strip_prefix("<file ")?;
    let inner = inner.trim_end_matches('>');
    let attr = inner.strip_prefix("path=")?;
    let path = attr.trim_matches('"');
    Some(PathBuf::from(path))
}

/// Parse `   <number> <content>` into `(number, content)`.  Returns
/// `None` for any line that does not start with optional whitespace
/// followed by a parseable `u32`.
fn parse_numbered_line(line: &str) -> Option<(u32, &str)> {
    let trimmed = line.trim_start();
    let mut split = trimmed.splitn(2, char::is_whitespace);
    let number_str = split.next()?;
    let line_no: u32 = number_str.parse().ok()?;
    let content = split.next().unwrap_or("");
    Some((line_no, content))
}

// ── LanceDB provider ─────────────────────────────────────────────────────────

/// `LanceDB` G2 source per `DEC-0015` D3.
///
/// Until P2-W8-F02 (`CodeRankEmbed` inference) and P2-W8-F04 (background
/// indexing) land, the table directory is empty — but the directory
/// layout is real (created by `StorageLayout::branch_vectors_dir`), and
/// the provider runs a real `tokio::fs::try_exists` / `read_dir` check
/// on every call.  Returns `Ok(G2SourceResults { hits: vec![] })` when
/// either (a) the table directory does not exist, or (b) the directory
/// exists but contains zero entries.
///
/// # `DEC-0016` cross-reference
///
/// `vectors_dir` is constructed from
/// [`crate::StorageLayout::branch_vectors_dir`]
/// (`crates/ucil-daemon/src/storage.rs:230` on main) — NOT through the
/// orphan `BranchManager` (`crates/ucil-daemon/src/branch_manager.rs`
/// does not exist on main per `DEC-0016`).  This provider does not
/// import or reference `crate::branch_manager`, so the orphan-branch
/// state has no bearing on F06.
///
/// # Forward-compat
///
/// When P2-W8-F04 lands the indexing pipeline, this body is augmented
/// to open `lancedb::connect(&self.vectors_dir)` and run the actual
/// vector query — the trait signature is unchanged so the call site at
/// `handle_search_code` does not need updating.  No `lancedb` crate dep
/// is added in this WO; `tokio::fs` is the only IO surface.
#[derive(Debug, Clone)]
pub struct LancedbProvider {
    /// Absolute path to the per-branch vectors directory, typically
    /// `<ucil-base>/branches/<branch>/vectors/` from
    /// [`crate::StorageLayout::branch_vectors_dir`] on main per
    /// `DEC-0016`.
    vectors_dir: PathBuf,
}

impl LancedbProvider {
    /// Construct a new `LanceDB` provider rooted at `vectors_dir`.
    #[must_use]
    pub const fn new(vectors_dir: PathBuf) -> Self {
        Self { vectors_dir }
    }
}

#[async_trait]
impl G2SourceProvider for LancedbProvider {
    fn source(&self) -> G2Source {
        G2Source::Lancedb
    }

    #[tracing::instrument(
        name = "ucil.tool.lancedb.vector_search",
        level = "debug",
        skip(self, _root),
        fields(query_len = query.len(), max_results),
    )]
    async fn execute(
        &self,
        query: &str,
        _root: &Path,
        max_results: usize,
    ) -> Result<G2SourceResults, G2SearchError> {
        let _ = (query, max_results);
        let table_dir = self.vectors_dir.join("code_chunks.lance");
        let exists = tokio::fs::try_exists(&table_dir)
            .await
            .map_err(G2SearchError::Lancedb)?;
        if !exists {
            tracing::debug!(
                table_dir = %table_dir.display(),
                "LancedbProvider: table directory absent; returning empty hits per DEC-0015 D3",
            );
            return Ok(G2SourceResults {
                source: G2Source::Lancedb,
                hits: Vec::new(),
            });
        }
        // Directory exists — count entries.  Zero entries == zero rows
        // until the indexer (P2-W8-F04) populates it.
        let mut entries = tokio::fs::read_dir(&table_dir)
            .await
            .map_err(G2SearchError::Lancedb)?;
        let mut count: usize = 0;
        while let Some(_entry) = entries.next_entry().await.map_err(G2SearchError::Lancedb)? {
            count += 1;
        }
        if count == 0 {
            tracing::debug!(
                table_dir = %table_dir.display(),
                "LancedbProvider: table directory empty; returning empty hits per DEC-0015 D3",
            );
        } else {
            tracing::debug!(
                table_dir = %table_dir.display(),
                count,
                "LancedbProvider: table directory populated but vector query path lands in P2-W8-F04",
            );
        }
        Ok(G2SourceResults {
            source: G2Source::Lancedb,
            hits: Vec::new(),
        })
    }
}

// ── Factory ──────────────────────────────────────────────────────────────────

/// Type alias for the builder closure stored inside [`G2SourceFactory`].
type BuilderFn =
    dyn Fn() -> Vec<Box<dyn G2SourceProvider + Send + Sync + 'static>> + Send + Sync + 'static;

/// Builds the three Phase-2 G2 providers in `[Probe, Ripgrep, Lancedb]`
/// order on every call.
///
/// Wraps a builder closure inside an `Arc` so the factory is cheap to
/// `Clone` — the [`crate::McpServer`] holding an `Arc<Self>` stays
/// `Clone` per WO-0049 lessons line 12.  The closure abstraction also
/// gives `#[cfg(test)]` callers a clean substitution seam via
/// [`Self::from_builder`] without touching the production codepath.
#[derive(Clone)]
pub struct G2SourceFactory {
    builder: Arc<BuilderFn>,
}

impl std::fmt::Debug for G2SourceFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("G2SourceFactory")
            .field(
                "builder",
                &"<Arc<dyn Fn() -> Vec<Box<dyn G2SourceProvider>>>>",
            )
            .finish()
    }
}

impl G2SourceFactory {
    /// Construct a new factory backed by the production
    /// `[Probe, Ripgrep, Lancedb]` provider trio.  `probe_timeout_ms`
    /// is the per-call budget threaded through to [`ProbeProvider::new`].
    #[must_use]
    pub fn new(probe_manifest_path: PathBuf, vectors_dir: PathBuf, probe_timeout_ms: u64) -> Self {
        Self {
            builder: Arc::new(move || {
                vec![
                    Box::new(ProbeProvider::new(
                        probe_manifest_path.clone(),
                        probe_timeout_ms,
                    )),
                    Box::new(RipgrepProvider::new()),
                    Box::new(LancedbProvider::new(vectors_dir.clone())),
                ]
            }),
        }
    }

    /// Construct a factory whose `build()` is fully delegated to a
    /// caller-supplied closure.  Crate-private substitution seam used
    /// by the frozen acceptance test in `server.rs` to inject
    /// `TestG2SourceProvider` impls — per `DEC-0008` §4 these are
    /// UCIL-owned trait substitutes, NOT critical-dep substitutes.
    #[must_use]
    pub fn from_builder<F>(builder: F) -> Self
    where
        F: Fn() -> Vec<Box<dyn G2SourceProvider + Send + Sync + 'static>> + Send + Sync + 'static,
    {
        Self {
            builder: Arc::new(builder),
        }
    }

    /// Build three boxed providers in `[Probe, Ripgrep, Lancedb]`
    /// order.  Per WO-0049 lessons line 12, returning fresh boxes on
    /// every call avoids consuming the providers by-move on every
    /// `handle_search_code` invocation.
    #[must_use]
    pub fn build(&self) -> Vec<Box<dyn G2SourceProvider + Send + Sync + 'static>> {
        (self.builder)()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_open_tag_extracts_path() {
        assert_eq!(
            parse_file_open_tag(r#"<file path="/abs/util.rs">"#),
            Some(PathBuf::from("/abs/util.rs")),
        );
        assert_eq!(parse_file_open_tag("<other>"), None);
        assert_eq!(parse_file_open_tag("not a tag"), None);
    }

    #[test]
    fn parse_numbered_line_extracts_lineno_and_content() {
        assert_eq!(
            parse_numbered_line("   1 fn foo() { let x = 42; }"),
            Some((1, "fn foo() { let x = 42; }")),
        );
        assert_eq!(
            parse_numbered_line("  23 pub fn bar(y: u32) -> u32 { y * 2 }"),
            Some((23, "pub fn bar(y: u32) -> u32 { y * 2 }")),
        );
        assert_eq!(parse_numbered_line("not a line"), None);
        assert_eq!(parse_numbered_line(""), None);
    }

    #[test]
    fn parse_probe_response_emits_hits_in_order() {
        let response = serde_json::json!({
            "content": [{
                "type": "text",
                "text": "preamble text\n<matches>\n\n<file path=\"/abs/util.rs\">\n\n   1 fn foo()\n  23 fn bar()\n</file>\n<file path=\"/abs/other.rs\">\n   5 fn baz()\n</file>\n</matches>",
            }],
        });
        let hits = parse_probe_response(&response, 50);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].file_path, PathBuf::from("/abs/util.rs"));
        assert_eq!(hits[0].start_line, 1);
        assert_eq!(hits[0].snippet, "fn foo()");
        assert_eq!(hits[1].start_line, 23);
        assert_eq!(hits[2].file_path, PathBuf::from("/abs/other.rs"));
        assert_eq!(hits[2].start_line, 5);
    }

    #[test]
    fn parse_probe_response_respects_max_results() {
        let response = serde_json::json!({
            "content": [{
                "type": "text",
                "text": "<file path=\"/abs/a.rs\">\n   1 line one\n   2 line two\n   3 line three\n</file>",
            }],
        });
        let hits = parse_probe_response(&response, 2);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn parse_probe_response_handles_missing_content() {
        let response = serde_json::json!({});
        assert!(parse_probe_response(&response, 50).is_empty());
    }

    #[tokio::test]
    async fn ripgrep_provider_runs_text_search_against_real_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("util.rs");
        std::fs::write(&file, "pub fn rgp_canary() -> u32 { 42 }\n").expect("seed file write");
        let provider = RipgrepProvider::new();
        let results = provider
            .execute("rgp_canary", tmp.path(), 50)
            .await
            .expect("execute ok");
        assert_eq!(results.source, G2Source::Ripgrep);
        assert!(
            results
                .hits
                .iter()
                .any(|h| h.snippet.contains("rgp_canary")),
            "expected at least one hit mentioning the canary; got {:?}",
            results.hits,
        );
    }

    #[tokio::test]
    async fn lancedb_provider_returns_empty_when_dir_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let provider = LancedbProvider::new(tmp.path().join("nonexistent_vectors"));
        let results = provider
            .execute("anything", tmp.path(), 50)
            .await
            .expect("execute ok");
        assert_eq!(results.source, G2Source::Lancedb);
        assert!(results.hits.is_empty());
    }

    #[tokio::test]
    async fn lancedb_provider_returns_empty_when_table_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let vectors = tmp.path().join("vectors");
        std::fs::create_dir_all(&vectors).expect("vectors dir");
        let provider = LancedbProvider::new(vectors);
        let results = provider
            .execute("anything", tmp.path(), 50)
            .await
            .expect("execute ok");
        assert_eq!(results.source, G2Source::Lancedb);
        assert!(results.hits.is_empty());
    }

    #[tokio::test]
    async fn lancedb_provider_returns_empty_when_table_empty_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let table = tmp.path().join("vectors").join("code_chunks.lance");
        std::fs::create_dir_all(&table).expect("table dir");
        let provider = LancedbProvider::new(tmp.path().join("vectors"));
        let results = provider
            .execute("anything", tmp.path(), 50)
            .await
            .expect("execute ok");
        assert_eq!(results.source, G2Source::Lancedb);
        assert!(results.hits.is_empty());
    }

    #[test]
    fn factory_builds_three_providers_in_canonical_order() {
        let factory = G2SourceFactory::new(
            PathBuf::from("/manifest.toml"),
            PathBuf::from("/vectors"),
            5_000,
        );
        let providers = factory.build();
        assert_eq!(providers.len(), 3);
        assert_eq!(providers[0].source(), G2Source::Probe);
        assert_eq!(providers[1].source(), G2Source::Ripgrep);
        assert_eq!(providers[2].source(), G2Source::Lancedb);
    }
}
