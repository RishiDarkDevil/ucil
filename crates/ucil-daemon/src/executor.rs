//! Tree-sitter → knowledge-graph ingestion pipeline (Phase 1 Week 4).
//!
//! This module implements feature `P1-W4-F04` — master-plan §18 Phase 1
//! Week 4 line 1759 ("Wire tree-sitter extraction → knowledge graph
//! population").  Given a file path, [`IngestPipeline::ingest_file`]
//!
//! 1. reads the file from disk;
//! 2. infers a [`Language`] from the extension (`rs` → Rust, `py` →
//!    Python, `ts`/`tsx` → TypeScript, `js`/`jsx`/`mjs`/`cjs` → JavaScript,
//!    `go` → Go, `java` → Java, `c`/`h` → C, `cc`/`cpp`/`cxx`/`hpp`/`hh`/
//!    `hxx` → C++, `rb` → Ruby, `sh`/`bash` → Bash, `json` → JSON);
//! 3. parses it with [`ucil_treesitter::parser::Parser`];
//! 4. extracts symbols with [`ucil_treesitter::symbols::SymbolExtractor`];
//! 5. maps each [`ExtractedSymbol`] to a
//!    [`ucil_core::knowledge_graph::Entity`]-shaped row with
//!    `source_tool = "tree-sitter"`; and
//! 6. upserts the **entire batch** in a single
//!    [`KnowledgeGraph::execute_in_transaction`] call so every file's
//!    symbols land under one `BEGIN IMMEDIATE` WAL transaction — the
//!    chokepoint master-plan §11 line 1117 + phase-log invariant 8
//!    mandate for all knowledge-graph writes.
//!
//! The pipeline is synchronous (no `tokio::spawn` / `.await`) because
//! `ucil_treesitter` + `rusqlite` are both blocking — callers that need
//! concurrency wrap a whole `ingest_file` call in
//! [`tokio::task::spawn_blocking`].  That integration point lives in the
//! next work-order (wiring the `FileWatcher` loop), outside this feature's
//! scope (see work-order `scope_out`).
//!
//! # Idempotency
//!
//! Running [`IngestPipeline::ingest_file`] twice on an unchanged file is
//! a no-op in row-count terms: every upsert uses the existing
//! `ON CONFLICT(qualified_name, file_path, t_valid_from) DO UPDATE SET
//! t_last_verified = datetime('now'), access_count = access_count + 1`
//! branch that [`KnowledgeGraph::upsert_entity`] introduced in WO-0024.
//! To keep the three uniqueness columns non-NULL and stable across runs
//! — `SQLite`'s UNIQUE constraint treats NULL as distinct, so any
//! NULL column in the triple would silently defeat the conflict path —
//! the pipeline synthesises a deterministic `qualified_name` of the form
//! `<file_path>::<name>@<start_line>:<start_col>` and pins
//! `t_valid_from` to the epoch constant [`TREE_SITTER_VALID_FROM`].  The
//! `@<line>:<col>` suffix disambiguates name-colliding methods (e.g. the
//! three `fmt` impls on distinct types in the fixture
//! `tests/fixtures/rust-project/src/eval_ctx.rs`) so each symbol gets
//! its own row rather than overwriting a sibling.
//!
//! # Error policy
//!
//! Per phase-log invariants 1 + 7, no mocks of tree-sitter, `SQLite`, or
//! the knowledge graph are permitted.  Every failure surfaces through
//! [`ExecutorError`], a `thiserror`-backed `#[non_exhaustive]` enum that
//! preserves cause chains via `#[from]` / `#[source]`.
//!
//! [`ExtractedSymbol`]: ucil_treesitter::symbols::ExtractedSymbol
//! [`KnowledgeGraph`]: ucil_core::knowledge_graph::KnowledgeGraph
//! [`KnowledgeGraph::execute_in_transaction`]:
//!     ucil_core::knowledge_graph::KnowledgeGraph::execute_in_transaction
//! [`KnowledgeGraph::upsert_entity`]:
//!     ucil_core::knowledge_graph::KnowledgeGraph::upsert_entity
//! [`Language`]: ucil_treesitter::parser::Language

// The public API (`ExecutorError`, `IngestPipeline`) shares a name prefix
// with the module ("executor" → `ExecutorError`) — match the convention
// established by `plugin_manager::PluginManager`, `watcher::FileWatcher`,
// and friends.
#![allow(clippy::module_name_repetitions)]

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::future::Future;
use std::hash::{Hash as _, Hasher as _};
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use thiserror::Error;
use ucil_core::knowledge_graph::{KnowledgeGraph, KnowledgeGraphError};
use ucil_treesitter::parser::{Language, ParseError, Parser};
use ucil_treesitter::symbols::{ExtractedSymbol, SymbolExtractor, SymbolKind};

// ── Constants ─────────────────────────────────────────────────────────────

/// `entities.source_tool` value every row this pipeline inserts carries.
///
/// Downstream fusion code (P1-W5-F06 Serena → G1 structural fusion) pivots
/// on this tag to tell tree-sitter-provenance rows apart from LSP- and
/// Serena-provenance rows.  Exposed as a `pub const` so callers can assert
/// provenance without duplicating the literal.
pub const SOURCE_TOOL: &str = "tree-sitter";

/// `entities.t_valid_from` pinned to the Unix epoch (RFC-3339) for every
/// tree-sitter-extracted row.
///
/// The bi-temporal semantics of `t_valid_from` (§12.2) do not apply to
/// raw AST extraction — tree-sitter observes the file "as of now" without
/// a meaningful valid-time lower bound.  Pinning the column to a fixed
/// constant keeps the `UNIQUE(qualified_name, file_path, t_valid_from)`
/// triple stable across re-indexing runs so the `ON CONFLICT DO UPDATE`
/// branch fires (master-plan §12.1 line 1131).  A later work-order that
/// introduces a bi-temporal consolidator can migrate this column to a
/// genuine valid-time lower bound; doing so only requires re-indexing,
/// not a schema change.
pub const TREE_SITTER_VALID_FROM: &str = "1970-01-01T00:00:00+00:00";

// ── Errors ────────────────────────────────────────────────────────────────

/// Errors produced by [`IngestPipeline`].
///
/// Marked `#[non_exhaustive]` so downstream crates cannot rely on
/// exhaustive matching — new failure modes can be added without a
/// `SemVer` break as the pipeline gains behaviour in later
/// work-orders (watcher wiring, multi-file concurrency).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ExecutorError {
    /// Reading the source file from disk failed.
    #[error("i/o error reading {path:?}: {source}")]
    Io {
        /// The path whose `read_to_string` failed.
        path: PathBuf,
        /// The underlying `std::io::Error` the OS returned.
        #[source]
        source: io::Error,
    },

    /// The file's extension was not recognised as a supported language.
    ///
    /// The pipeline infers [`Language`] from the extension table
    /// documented at the module level; unknown extensions flow through
    /// this variant rather than silently producing an empty batch so
    /// upstream callers can log or re-route.
    #[error("unsupported file extension for tree-sitter extraction: {path:?}")]
    UnsupportedExtension {
        /// The path whose extension was rejected.
        path: PathBuf,
    },

    /// Tree-sitter parsing failed.
    ///
    /// Note that *syntax errors* inside the source do **not** surface as
    /// this variant — tree-sitter represents them as error nodes inside
    /// the returned tree.  This variant only fires on load / timeout /
    /// internal-parser failures (see [`ParseError`]).
    #[error("parse failed: {0}")]
    Parse(#[from] ParseError),

    /// Knowledge-graph write failed.
    #[error("knowledge_graph error: {0}")]
    KnowledgeGraph(#[from] KnowledgeGraphError),
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Infer a [`Language`] from `path`'s extension.
///
/// Returns `None` when the extension is absent, non-UTF-8, or not one of
/// the known mappings.  The table here is the single source of truth for
/// extension → language mapping inside the pipeline — extensions that
/// `ucil_treesitter` supports but this table omits (e.g. `.pyi`) cannot
/// be ingested yet; adding them is a one-line additive change.
fn language_from_extension(path: &Path) -> Option<Language> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Some(Language::Rust),
        "py" => Some(Language::Python),
        "ts" | "tsx" => Some(Language::TypeScript),
        "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
        "go" => Some(Language::Go),
        "java" => Some(Language::Java),
        "c" | "h" => Some(Language::C),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some(Language::Cpp),
        "rb" => Some(Language::Ruby),
        "sh" | "bash" => Some(Language::Bash),
        "json" => Some(Language::Json),
        _ => None,
    }
}

/// Map a [`SymbolKind`] variant to the lowercase string tag stored in
/// `entities.kind`.
///
/// Values mirror the `#[serde(rename_all = "snake_case")]` representation
/// [`SymbolKind`] derives — keeping this function in lockstep with
/// [`SymbolKind`] Serde so downstream tools that round-trip JSON see the
/// same tag a direct Serde serialize would emit.
const fn kind_tag(k: SymbolKind) -> &'static str {
    // `SymbolKind` is `#[non_exhaustive]` (owned by `ucil_treesitter`),
    // so external crates must include a wildcard arm.  New variants
    // flow through the `"unknown"` fallback until this table is
    // updated — `test_kind_tag_covers_all_variants` fails loudly in
    // that case so the mismatch cannot ship unnoticed.
    match k {
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Class => "class",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Interface => "interface",
        SymbolKind::TypeAlias => "type_alias",
        SymbolKind::Constant => "constant",
        SymbolKind::Module => "module",
        _ => "unknown",
    }
}

/// Map a [`Language`] variant to the lowercase string tag stored in
/// `entities.language`.
///
/// Values match the `language_serde` module in `ucil_treesitter::symbols`
/// so a tree-sitter-extracted row's `language` column matches the tag a
/// Serde round-trip of [`ExtractedSymbol::language`] would emit.
const fn language_tag(l: Language) -> &'static str {
    // `Language` is `#[non_exhaustive]` (owned by `ucil_treesitter`),
    // so external crates must include a wildcard arm.  The pipeline
    // only receives a `Language` after going through
    // `language_from_extension`, which returns only variants this
    // table covers — the wildcard arm is therefore unreachable in
    // practice but required by the compiler.
    match l {
        Language::Rust => "rust",
        Language::Python => "python",
        Language::TypeScript => "typescript",
        Language::JavaScript => "javascript",
        Language::Go => "go",
        Language::Java => "java",
        Language::C => "c",
        Language::Cpp => "cpp",
        Language::Ruby => "ruby",
        Language::Bash => "bash",
        Language::Json => "json",
        _ => "unknown",
    }
}

/// Compose the deterministic `qualified_name` every pipeline row carries.
///
/// Shape: `<file_path>::<symbol_name>@<start_line>:<start_col>`.  The
/// line/col suffix disambiguates same-name sibling methods inside one
/// file (e.g. the three `impl fmt::Display for Value / EvalError / Expr`
/// blocks in the fixture `rust-project`) so each one gets its own
/// `entities` row under the `UNIQUE(qualified_name, file_path,
/// t_valid_from)` constraint rather than silently aliasing to the first
/// writer.
fn build_qualified_name(file_path: &str, s: &ExtractedSymbol) -> String {
    format!(
        "{file_path}::{name}@{start_line}:{start_col}",
        name = s.name,
        start_line = s.start_line,
        start_col = s.start_col,
    )
}

/// Compute a deterministic hex-encoded `source_hash` for the symbol's
/// line span in `source`.
///
/// Feeds the symbol's 1-based line/col range and each source line in the
/// `[start_line, end_line]` range into a
/// [`std::collections::hash_map::DefaultHasher`] (fixed-key `SipHash-1-3`
/// internally, therefore deterministic across processes with the same
/// `rustc` build).  The returned 16-hex-char `u64` is sufficient for
/// staleness detection — not a cryptographic integrity check — which is
/// all `entities.source_hash` is used for today.
///
/// A later work-order may upgrade this to a proper `Blake3` / `SHA-256`
/// digest; callers that round-trip rows should therefore treat the
/// value as an opaque string, not a fixed-width integer.
fn compute_source_hash(source: &str, s: &ExtractedSymbol) -> String {
    let mut h = DefaultHasher::new();
    s.start_line.hash(&mut h);
    s.start_col.hash(&mut h);
    s.end_line.hash(&mut h);
    s.end_col.hash(&mut h);
    let lines: Vec<&str> = source.lines().collect();
    // `start_line` / `end_line` are 1-based; clamp to `lines.len()` so
    // malformed ranges (end past EOF, start == 0) never panic.
    let start = usize::try_from(s.start_line.saturating_sub(1))
        .unwrap_or(usize::MAX)
        .min(lines.len());
    let end = usize::try_from(s.end_line)
        .unwrap_or(usize::MAX)
        .min(lines.len())
        .max(start);
    for line in &lines[start..end] {
        line.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// Intermediate row struct bound to the `entities` table's column order.
///
/// Kept private because it has no purpose outside
/// [`IngestPipeline::ingest_file`] — callers who want a richer row type
/// work with [`ucil_core::knowledge_graph::Entity`] (the public shape
/// the CRUD helpers speak).
struct EntityRow {
    kind: String,
    name: String,
    qualified_name: String,
    file_path: String,
    start_line: i64,
    end_line: i64,
    signature: Option<String>,
    doc_comment: Option<String>,
    language: String,
    t_valid_from: String,
    importance: f64,
    source_tool: String,
    source_hash: String,
}

/// Build an [`EntityRow`] from an [`ExtractedSymbol`] plus the
/// extraction-time scalars the row also needs.
fn symbol_to_row(s: &ExtractedSymbol, source: &str, file_path: &str, lang: Language) -> EntityRow {
    EntityRow {
        kind: kind_tag(s.kind).to_owned(),
        name: s.name.clone(),
        qualified_name: build_qualified_name(file_path, s),
        file_path: file_path.to_owned(),
        start_line: i64::from(s.start_line),
        end_line: i64::from(s.end_line),
        signature: s.signature.clone(),
        doc_comment: s.doc_comment.clone(),
        language: language_tag(lang).to_owned(),
        t_valid_from: TREE_SITTER_VALID_FROM.to_owned(),
        importance: 0.5,
        source_tool: SOURCE_TOOL.to_owned(),
        source_hash: compute_source_hash(source, s),
    }
}

// ── Pipeline ──────────────────────────────────────────────────────────────

/// Tree-sitter → knowledge-graph extraction pipeline.
///
/// Holds a reusable [`Parser`] (tree-sitter's `Parser::set_language` reset
/// is cheap but not free) and a stateless [`SymbolExtractor`].  Callers
/// typically keep one [`IngestPipeline`] per worker thread and invoke
/// [`ingest_file`][Self::ingest_file] per event.
///
/// The pipeline does **not** own the [`KnowledgeGraph`]; callers pass a
/// `&mut KnowledgeGraph` per call so the ingest's `BEGIN IMMEDIATE`
/// transaction composes cleanly with other writers in the same process.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// use ucil_core::KnowledgeGraph;
/// use ucil_daemon::executor::IngestPipeline;
///
/// # fn demo(kg_path: &Path, file_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
/// let mut kg = KnowledgeGraph::open(kg_path)?;
/// let mut pipeline = IngestPipeline::new();
/// let n = pipeline.ingest_file(&mut kg, file_path)?;
/// assert!(n >= 0);
/// # Ok(())
/// # }
/// ```
pub struct IngestPipeline {
    parser: Parser,
    extractor: SymbolExtractor,
}

impl IngestPipeline {
    /// Construct a new pipeline.
    ///
    /// Equivalent to [`Self::default`]; kept as an explicit constructor
    /// for symmetry with [`Parser::new`] / [`SymbolExtractor::new`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            extractor: SymbolExtractor::new(),
        }
    }

    /// Parse `path`, extract its symbols, and upsert them into `kg`
    /// inside one `BEGIN IMMEDIATE` transaction.
    ///
    /// Returns the number of symbols upserted — 0 for a fallback-language
    /// file (Java / C / C++ / Ruby / Bash / JSON, which today yield
    /// `Vec::new()` from `SymbolExtractor::extract`) or any file whose
    /// tree-sitter pass produces no named symbols.
    ///
    /// # Transaction scope
    ///
    /// Every symbol for the file lands in a single call to
    /// [`KnowledgeGraph::execute_in_transaction`]
    /// (`TransactionBehavior::Immediate`) — the master-plan §11 line 1117
    /// chokepoint.  Partial batches are impossible: either every row
    /// upserts or the whole batch rolls back.
    ///
    /// # Idempotency
    ///
    /// Calling `ingest_file` twice on an unchanged file leaves the row
    /// count unchanged; each `entities` row's `access_count` bumps and
    /// `t_last_verified` refreshes via the existing
    /// [`KnowledgeGraph::upsert_entity`] `ON CONFLICT DO UPDATE` branch.
    /// See module-level docs "Idempotency" for the stable triple shape.
    ///
    /// # Errors
    ///
    /// * [`ExecutorError::Io`] — `std::fs::read_to_string(path)` failed.
    /// * [`ExecutorError::UnsupportedExtension`] — the file's extension
    ///   is not in the module-level extension table.
    /// * [`ExecutorError::Parse`] — `ucil_treesitter` returned a non-`OK`
    ///   [`ParseError`] (grammar load / timeout / internal).
    /// * [`ExecutorError::KnowledgeGraph`] — transaction begin, statement
    ///   prepare, parameter bind, or commit failed.
    #[tracing::instrument(
        level = "debug",
        name = "ucil.daemon.executor.ingest_file",
        skip(self, kg),
        fields(path = %path.display()),
    )]
    pub fn ingest_file(
        &mut self,
        kg: &mut KnowledgeGraph,
        path: &Path,
    ) -> Result<usize, ExecutorError> {
        let lang =
            language_from_extension(path).ok_or_else(|| ExecutorError::UnsupportedExtension {
                path: path.to_path_buf(),
            })?;

        let source = std::fs::read_to_string(path).map_err(|source| ExecutorError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        let tree = self.parser.parse(&source, lang)?;
        let symbols = self.extractor.extract(&tree, &source, path, lang);
        tracing::debug!(count = symbols.len(), "tree-sitter symbols extracted");

        if symbols.is_empty() {
            return Ok(0);
        }

        let file_path_str = path.display().to_string();
        let rows: Vec<EntityRow> = symbols
            .iter()
            .map(|s| symbol_to_row(s, &source, &file_path_str, lang))
            .collect();

        let inserted = kg.execute_in_transaction(|tx| -> Result<usize, rusqlite::Error> {
            let mut stmt = tx.prepare(
                "INSERT INTO entities (\
                    kind, name, qualified_name, file_path, start_line, end_line, \
                    signature, doc_comment, language, t_valid_from, t_valid_to, \
                    importance, source_tool, source_hash\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12, ?13) \
                 ON CONFLICT(qualified_name, file_path, t_valid_from) DO UPDATE SET \
                    t_last_verified = datetime('now'), \
                    access_count = access_count + 1;",
            )?;
            let mut count = 0usize;
            for row in &rows {
                stmt.execute(rusqlite::params![
                    row.kind,
                    row.name,
                    row.qualified_name,
                    row.file_path,
                    row.start_line,
                    row.end_line,
                    row.signature,
                    row.doc_comment,
                    row.language,
                    row.t_valid_from,
                    row.importance,
                    row.source_tool,
                    row.source_hash,
                ])?;
                count += 1;
            }
            Ok(count)
        })?;

        Ok(inserted)
    }
}

impl Default for IngestPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ── Serena G1 hover fusion ───────────────────────────────────────────────
//
// WO-0037 for `P1-W5-F02` (master-plan §18 Phase 1 Week 5 lines 1762-1770,
// "Serena integration → G1 structural fusion") adds a dependency-inversion
// seam around the Serena MCP channel's `textDocument/hover` response so the
// daemon's `find_definition` / `find_references` / `go_to_definition` tools
// can enrich their responses with signature + documentation context without
// coupling the core daemon to Serena's wire format.
//
// The seam has three pieces:
//
// 1. [`SerenaHoverClient`] — the trait a live implementation wires to the
//    Serena MCP channel ([`plugin_manager::PluginManager`] already owns
//    the stdio pipe; the glue WO lands after this one).  Per DEC-0008 §4
//    the trait is UCIL-owned, not a direct re-export of Serena's `tools/
//    call` payload shape, so the dependency direction is UCIL → Serena
//    (not the other way round).
// 2. [`enrich_find_definition`] — the pure async fusion function that
//    merges a [`ucil_core::knowledge_graph::SymbolResolution`] + its
//    [`Caller`] list + optional hover info from the trait into an
//    [`EnrichedFindDefinition`].  Errors from the client are suppressed
//    (logged at `warn!`) so a Serena outage never breaks the G1 response
//    — the master-plan §13.4 diagnostics-bridge best-effort contract
//    applies to hover fusion too.
// 3. `fake_serena_hover_client::ScriptedFakeSerenaHoverClient` — the
//    hand-written scripted fake that drives the fusion function under
//    test.  It is NOT a mock of Serena's MCP wire format (forbidden per
//    root `CLAUDE.md`) — it implements UCIL's own [`SerenaHoverClient`]
//    trait, the DEC-0008 canonical test seam also in use by
//    `ucil-lsp-diagnostics::{call_hierarchy,quality_pipeline}::
//    fake_serena_client`.
//
// Wiring into `server::McpServer::handle_find_definition` is
// deliberately out of scope for this WO — see the work-order's
// `scope_out` field for the reasoning (the P1-W4-F05 frozen acceptance
// selector asserts on the current `_meta` JSON shape and an ADR-gated
// envelope extension will land with the live-wiring follow-up WO).

/// Provenance of a [`HoverDoc`] — which upstream produced the markdown.
///
/// Master-plan §13.4 (diagnostics bridge sources) enumerates the three
/// provenance tiers UCIL's hover bus surfaces today.  Variants map as:
///
/// * [`HoverSource::Serena`] — hover fetched over the Serena MCP channel
///   (the live [`SerenaHoverClient`] impl landing in a follow-up WO).
/// * [`HoverSource::Lsp`] — hover fetched directly from an LSP server
///   (reserved for the LSP bridge in `ucil-lsp-diagnostics`; not produced
///   by this WO but included so the enum is forward-compatible without
///   a `SemVer` break — see DEC-0008 §3 "degraded mode when Serena is
///   unavailable but an LSP is").
/// * [`HoverSource::None`] — no upstream supplied hover text; callers
///   that want to assert "Serena tried and returned nothing" should
///   pair `HoverSource::None` with `Option<HoverDoc>::None` on the
///   [`EnrichedFindDefinition`] rather than building a sentinel doc.
///
/// The enum is `#[non_exhaustive]` so a later WO can add provenance
/// variants (e.g. `HoverSource::TreeSitter` for doc-comment fallback)
/// without a `SemVer` break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HoverSource {
    /// Hover fetched over the Serena MCP channel.
    Serena,
    /// Hover fetched directly from an LSP server.
    Lsp,
    /// No upstream supplied hover text.
    None,
}

/// A single hover document — markdown blob plus its provenance.
///
/// `markdown` is the **unprocessed** LSP hover text, which typically
/// includes Markdown headings (`## Signature`), fenced code blocks
/// (```` ``` ````), and cross-reference links.  The daemon does not
/// re-flow or sanitise the payload; the MCP response carries it
/// verbatim so the client (Claude Code / Codex / Cursor / …) can render
/// it with their own Markdown pipeline.
///
/// `source` tracks which upstream produced the payload so the MCP
/// response can populate `_meta.source` precisely and a consuming
/// adapter can decide whether to trust the markdown's `signature`
/// section as authoritative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverDoc {
    /// Unprocessed hover markdown produced by Serena / LSP.
    pub markdown: String,
    /// Provenance of [`Self::markdown`].
    pub source: HoverSource,
}

/// Errors [`SerenaHoverClient::hover`] can return.
///
/// The enum is intentionally `#[non_exhaustive]` so a later WO can add
/// transport-layer variants (e.g. `RateLimited`, `ProtocolVersion`,
/// `UnsupportedLanguage`) without a `SemVer` break.  Payloads are
/// `String` rather than concrete wrapped errors so this enum stays
/// cycle-free from `ucil-lsp-diagnostics` and MCP-client internals;
/// the live-wiring WO that implements the trait against a real MCP
/// client converts from its native errors via `.to_string()`.
///
/// All variants are treated equivalently by [`enrich_find_definition`]
/// — an `Err(_)` return means `hover = None` in the fused result, so
/// the specific variant is observed only by the logger.  Master-plan
/// §13.4 (diagnostics bridge best-effort contract) applies: a Serena
/// outage never breaks a G1 response.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HoverFetchError {
    /// Transport-level failure on the Serena MCP channel (closed pipe,
    /// JSON-RPC framing error, …).  Wraps the downstream error message
    /// as a string so this enum stays cycle-free.
    #[error("serena mcp channel error: {0}")]
    Channel(String),
    /// The hover response body failed to decode (bad UTF-8, missing
    /// required field in the MCP `tools/call` result, …).
    #[error("hover response decode failed: {0}")]
    Decode(String),
    /// The hover request exceeded its timeout budget.  Per the rust-style
    /// rules every IO-touching `.await` in UCIL is wrapped in
    /// `tokio::time::timeout` with a named const; the timeout value is
    /// carried through so the logger can print it verbatim.
    #[error("hover request timed out after {0:?}")]
    Timeout(std::time::Duration),
}

/// Dependency-inversion seam for fetching hover markdown from Serena.
///
/// Per DEC-0008 §4 this trait is UCIL-owned — it is **not** a re-export
/// or adapter of Serena's MCP `textDocument/hover` wire format.  A live
/// implementation (landing in a follow-up WO) converts the trait's
/// arguments into a Serena `tools/call` request and its response back
/// into a [`HoverDoc`].  The test suite drives [`enrich_find_definition`]
/// through `fake_serena_hover_client::ScriptedFakeSerenaHoverClient`,
/// a hand-written scripted fake implementing this exact trait — see the
/// sibling `SerenaClient` in `ucil-lsp-diagnostics` for the precedent
/// (WO-0015, already live and verifier-passed).
///
/// Returns:
///
/// * `Ok(Some(doc))` — Serena returned a hover payload.
/// * `Ok(None)` — Serena returned an empty hover (the LSP "no info"
///   case), or the symbol has no known hover info.  Distinguished from
///   an error so callers can decide whether to retry or fall back.
/// * `Err(e)` — transport / decode / timeout failure.  Callers should
///   treat this as a degraded upstream, not a user-visible error;
///   [`enrich_find_definition`] logs the error at `warn!` and yields
///   `hover: None` in the fused result.
///
/// `Send + Sync` bounds are required so trait objects can live in
/// `Arc<dyn SerenaHoverClient>` inside the daemon's long-lived server
/// state (the wiring WO constructs the `Arc` on startup).
#[async_trait::async_trait]
pub trait SerenaHoverClient: Send + Sync {
    /// Fetch hover markdown for `resolution`.
    ///
    /// The default live implementation will map `resolution.file_path`
    /// + `resolution.start_line` to an LSP `textDocument/hover` request
    /// routed through Serena's MCP pipe, but the trait intentionally
    /// hides that detail — implementors can synthesise the request
    /// however they like, and alternative upstreams (e.g. a pure-LSP
    /// bridge) can implement this trait directly.
    async fn hover(
        &self,
        resolution: &ucil_core::knowledge_graph::SymbolResolution,
    ) -> Result<Option<HoverDoc>, HoverFetchError>;
}

/// Projection of one `calls`-kind inbound relation's source entity — a
/// caller of the resolved definition.
///
/// Mirrors the JSON shape `{qualified_name, file_path, start_line}`
/// that `server::project_callers` emits onto the MCP
/// `_meta.callers` array (see `server.rs`).  Promoted to a typed struct
/// here so [`enrich_find_definition`] stays testable without round-
/// tripping through `serde_json::Value`.  The live-wiring WO that
/// threads this into `server::McpServer::handle_find_definition`
/// will either convert from this typed form back to `Value` at the
/// envelope boundary or push the typed form all the way through — the
/// choice is scoped to that WO's ADR.
///
/// Fields are named to match the JSON keys one-for-one so a reader who
/// knows the MCP envelope can recognise each field without a map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// Caller entity's `qualified_name` (e.g. `"mymod::bar"`).  `None`
    /// when the source row's `qualified_name` column is `NULL` (master-
    /// plan §12.1 allows a `NULL` `qualified_name` for `kind = "file"`
    /// rows; they are rare callers but possible).
    pub qualified_name: Option<String>,
    /// Caller entity's `file_path` — project-relative or absolute,
    /// matching however the ingest stored it.
    pub file_path: String,
    /// Caller entity's 1-based `start_line`, or `None` when the
    /// underlying column is `NULL`.
    pub start_line: Option<i64>,
}

/// The hover-enriched output of [`enrich_find_definition`].
///
/// Carries the original [`ucil_core::knowledge_graph::SymbolResolution`]
/// and its [`Caller`] list verbatim, plus an optional [`HoverDoc`].  The
/// `hover` field is `None` when:
///
/// * the caller passed `client: None` — Serena is degraded / not
///   installed in this deployment;
/// * the client returned `Ok(None)` — Serena has no hover info for the
///   symbol;
/// * the client returned `Err(_)` — transport / decode / timeout
///   failure; the specific variant is logged at `warn!` but suppressed
///   from the fused result per the §13.4 best-effort contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichedFindDefinition {
    /// Original resolution echoed through from the fusion input.
    pub resolution: ucil_core::knowledge_graph::SymbolResolution,
    /// Original callers echoed through from the fusion input.
    pub callers: Vec<Caller>,
    /// Hover markdown when available; `None` per the three degraded
    /// cases documented on [`EnrichedFindDefinition`].
    pub hover: Option<HoverDoc>,
}

/// Fuse a [`ucil_core::knowledge_graph::SymbolResolution`] + its
/// [`Caller`] list with optional Serena hover markdown into an
/// [`EnrichedFindDefinition`].
///
/// The function is pure (no I/O beyond the optional
/// [`SerenaHoverClient::hover`] call) and best-effort: a hover-fetch
/// error is logged at `warn!` via [`tracing::warn`] and suppressed from
/// the return value.  Master-plan §13.4 (diagnostics bridge best-effort
/// contract) applies — a Serena outage must never surface as a G1 tool
/// error; the MCP response just omits the hover field instead.
///
/// The type parameter `C: SerenaHoverClient + ?Sized` lets the function
/// accept both a concrete `&ScriptedFakeSerenaHoverClient` (hermetic
/// test path) and an `&dyn SerenaHoverClient` trait object (live path;
/// the `Arc<dyn SerenaHoverClient>` constructed by the wiring WO
/// auto-derefs to `&dyn SerenaHoverClient`).
///
/// # Examples
///
/// ```no_run
/// use ucil_core::knowledge_graph::SymbolResolution;
/// use ucil_daemon::executor::{
///     enrich_find_definition, Caller, SerenaHoverClient,
/// };
///
/// # async fn demo(
/// #     client: Option<&dyn SerenaHoverClient>,
/// #     resolution: SymbolResolution,
/// #     callers: Vec<Caller>,
/// # ) {
/// let enriched = enrich_find_definition(resolution, callers, client).await;
/// let _ = enriched.hover; // Option<HoverDoc>
/// # }
/// ```
#[tracing::instrument(
    name = "ucil.daemon.executor.enrich_find_definition",
    level = "debug",
    skip(client)
)]
pub async fn enrich_find_definition<C: SerenaHoverClient + ?Sized>(
    resolution: ucil_core::knowledge_graph::SymbolResolution,
    callers: Vec<Caller>,
    client: Option<&C>,
) -> EnrichedFindDefinition {
    let hover = match client {
        None => None,
        Some(c) => match c.hover(&resolution).await {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(
                    symbol = ?resolution.qualified_name,
                    error = ?e,
                    "serena hover fetch failed; returning unenriched result",
                );
                None
            }
        },
    };
    EnrichedFindDefinition {
        resolution,
        callers,
        hover,
    }
}

// ── G1 parallel-execution orchestrator (P2-W7-F01, WO-0047) ──────────────
//
// WO-0047 for `P2-W7-F01` (master-plan §5.1 lines 420-446 + §18 Phase 2
// Week 7 line 1780, "G1 (Structural) — All tools parallel, fuse
// everything") adds the parallel fan-out orchestrator that runs
// tree-sitter, Serena, ast-grep, and the LSP diagnostics bridge
// concurrently with a 5 s overall deadline, returning per-source
// results so partial outcomes remain usable when one tool is
// unavailable.
//
// Production wiring of real subprocess clients (Serena MCP plugin,
// ast-grep MCP plugin, real `ucil_treesitter::parser::Parser`, real
// `crates/ucil-lsp-diagnostics::bridge`) is deferred to P2-W7-F02
// (G1 fusion) and P2-W7-F05 (find_references).  F01 ships only the
// orchestrator + the [`G1Source`] dependency-inversion seam (per
// `DEC-0008`) plus its unit acceptance test that injects local trait
// impls of [`G1Source`] — UCIL's own abstraction boundary.
//
// The existing [`enrich_find_definition`] (WO-0037, see executor.rs
// above) stays unmodified — F02 will compose this orchestrator's
// outputs into a richer fused payload via [`execute_g1`]; F01 adds
// capability without removing the G1-fusion-lite hover-only helper
// or its `find_definition` call site.

/// Master timeout for the G1 parallel-execution orchestrator.
///
/// Master-plan §5.1 line 444 specifies a 5 s overall deadline for the
/// G1 fan-out so the daemon can return partial results to the host
/// adapter even when one of the four sources hangs.  When this
/// deadline elapses, [`execute_g1`] returns a [`G1Outcome`] with
/// `master_timed_out = true` and per-source [`G1ToolStatus::TimedOut`]
/// placeholders for any source that had not yet completed.
pub const G1_MASTER_DEADLINE: Duration = Duration::from_millis(5_000);

/// Per-source timeout applied to each [`G1Source::execute`] call.
///
/// 4.5 s leaves a 0.5 s margin under [`G1_MASTER_DEADLINE`] so the
/// per-source timeout always wins on a true global stall — the master
/// deadline is a safety net, not the primary timing path.  Avoids the
/// per-source-timeout fast-path racing the master deadline (master-plan
/// §5.1 line 444 single-tool-stall handling).
pub const G1_PER_SOURCE_DEADLINE: Duration = Duration::from_millis(4_500);

/// Structural-query input shape for the G1 fan-out.
///
/// Mirrors the master-plan §5.1 fan-out target ("Query → ALL of the
/// following run in parallel"): a symbol name plus its on-disk
/// location.  Live wiring will derive these from the host adapter's
/// `find_definition` / `find_references` request; the unit test
/// constructs them directly.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct G1Query {
    /// Symbol name to look up (e.g. `"TaskManager"`).
    pub symbol: String,
    /// File path containing the symbol's primary occurrence.
    pub file_path: PathBuf,
    /// 1-based line number of the symbol's primary occurrence.
    pub line: u32,
    /// 1-based column number of the symbol's primary occurrence.
    pub column: u32,
}

/// Identifier for one of the five G1 (structural) sources the
/// orchestrator fans out to.
///
/// Variants name each source's expected production wiring:
///
/// * [`G1ToolKind::TreeSitter`] — `ucil_treesitter::parser::Parser`,
///   wired through the existing [`IngestPipeline`] entry point in
///   F02 / F05.
/// * [`G1ToolKind::Serena`] — Serena MCP plugin reached via
///   `executor::SerenaHoverClient` (WO-0037, see executor.rs above).
/// * [`G1ToolKind::AstGrep`] — ast-grep MCP plugin landed by WO-0044
///   (see `plugins/structural/ast-grep/plugin.toml`).
/// * [`G1ToolKind::Diagnostics`] — `crates/ucil-lsp-diagnostics::bridge`
///   reached through the LSP diagnostics fan-in.
/// * [`G1ToolKind::Scip`] — `crate::scip::ScipG1Source` (CLI → `SQLite`
///   pipeline per `DEC-0014`; cross-repo compiler-accurate symbol
///   index emitted by `scip-rust` and queried via
///   `crate::scip::query_symbol`).
///
/// Joern is explicitly out of scope (post-Phase-2).  The enum is *not*
/// `#[non_exhaustive]` because each variant maps to a fixed master-plan
/// §5.1 source — extending it later is a deliberate additive
/// non-breaking change a future WO can make with an ADR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum G1ToolKind {
    /// Tree-sitter structural parse (production wiring: `ucil_treesitter`).
    TreeSitter,
    /// Serena MCP `textDocument/hover` channel (production wiring:
    /// `executor::SerenaHoverClient` from WO-0037).
    Serena,
    /// `ast-grep` MCP plugin (production wiring: WO-0044 manifest at
    /// `plugins/structural/ast-grep/plugin.toml`).
    AstGrep,
    /// LSP diagnostics bridge (production wiring:
    /// `crates/ucil-lsp-diagnostics::bridge`).
    Diagnostics,
    /// SCIP cross-repo symbol index (production wiring:
    /// `crate::scip::ScipG1Source` per `DEC-0014`).  Authority rank
    /// 4 — below the four pre-existing sources because SCIP is an
    /// offline batch indexer, so a freshly-indexed Serena/LSP signal
    /// beats a stale SCIP entry whenever they conflict.  Master-plan
    /// §22 line 616: "LSP/AST → SCIP → Dep tools → KG → Text".
    Scip,
}

/// Disposition of one G1 source on a given fan-out call.
///
/// Each variant is a discriminant with no inner data — the data lives
/// on [`G1ToolOutput`] via `payload` / `error` / `elapsed_ms`.  Master-
/// plan §5.1 prescribes per-source dispositions so partial outcomes
/// remain usable: a single [`G1ToolStatus::Errored`] does not turn the
/// entire fan-out into a failure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum G1ToolStatus {
    /// The source returned a payload within its per-source deadline.
    Available,
    /// The source is degraded or not installed in this deployment
    /// (e.g. `ast-grep` binary absent, Serena plugin disabled).
    Unavailable,
    /// The source's per-source `tokio::time::timeout` elapsed before
    /// it returned a response.
    TimedOut,
    /// The source returned an error (transport / decode / internal).
    Errored,
}

/// One source's contribution to a G1 fan-out outcome.
///
/// `payload` carries the source's emitted JSON (e.g. tree-sitter AST
/// snippet, Serena hover markdown, ast-grep matches, diagnostics
/// bundle) when [`Self::status`] is [`G1ToolStatus::Available`];
/// otherwise it is `serde_json::Value::Null` and `error` carries an
/// operator-readable description of the degraded path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct G1ToolOutput {
    /// Which source produced this output.
    pub kind: G1ToolKind,
    /// Disposition of the source on this fan-out call.
    pub status: G1ToolStatus,
    /// Wall-clock time the source spent before returning, in
    /// milliseconds.
    pub elapsed_ms: u64,
    /// Source-emitted JSON payload, or `Value::Null` on a degraded
    /// path.  The shape is source-specific and intentionally untyped
    /// here so F02 (G1 fusion) can layer a typed projection without
    /// further changes to this struct.
    pub payload: serde_json::Value,
    /// Operator-readable error description for any non-`Available`
    /// status.  `None` for [`G1ToolStatus::Available`].
    pub error: Option<String>,
}

/// Aggregate outcome of one [`execute_g1`] fan-out call.
///
/// `results` is a `Vec` (rather than a fixed-size array) so the same
/// orchestrator can be reused unchanged when SCIP/Joern land in
/// P2-W7-F08 and Phase-3.  Order matches the order of the input
/// `sources` argument.
///
/// `master_timed_out` is `true` when the outer
/// [`G1_MASTER_DEADLINE`] elapsed before all per-source futures
/// completed; in that case `results` carries
/// [`G1ToolStatus::TimedOut`] placeholders for every source so
/// downstream code never sees an empty-but-non-`master_timed_out`
/// outcome.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct G1Outcome {
    /// Per-source outputs, in the same order as the input `sources`.
    pub results: Vec<G1ToolOutput>,
    /// Wall-clock time the orchestrator spent in milliseconds.
    pub wall_elapsed_ms: u64,
    /// `true` iff the outer master deadline elapsed before all
    /// per-source futures completed.
    pub master_timed_out: bool,
}

/// Dependency-inversion seam for one of the four G1 (structural)
/// sources.
///
/// Per `DEC-0008` §4 this trait is UCIL-owned — it is **not** a
/// re-export or adapter of any external wire format.  The unit
/// acceptance test `test_g1_parallel_execution` supplies four local
/// trait impls of [`G1Source`] (UCIL's own abstraction boundary);
/// production wiring of real subprocess clients lands in P2-W7-F02
/// (fusion) and P2-W7-F05 (`find_references`).
///
/// The same dependency-inversion seam pattern as
/// [`SerenaHoverClient`] above (executor.rs:640) — a UCIL-owned
/// trait that a live implementation converts to whatever wire shape
/// its upstream speaks.
///
/// `Send + Sync` bounds are required so trait objects can live in
/// `Vec<Box<dyn G1Source + Send + Sync + 'static>>` inside the
/// daemon's long-lived server state once F02 / F05 land.
#[async_trait::async_trait]
pub trait G1Source: Send + Sync {
    /// Identifies this source's [`G1ToolKind`] without runtime
    /// introspection so [`execute_g1`] can label results by source.
    fn kind(&self) -> G1ToolKind;

    /// Run this source's structural query.
    ///
    /// Implementations are responsible for emitting their own
    /// [`G1ToolOutput`] with the appropriate [`G1ToolStatus`] —
    /// the orchestrator only overrides the status to
    /// [`G1ToolStatus::TimedOut`] when its per-source
    /// `tokio::time::timeout` elapses.
    async fn execute(&self, query: &G1Query) -> G1ToolOutput;
}

/// Run one source under [`G1_PER_SOURCE_DEADLINE`] (or `deadline`,
/// whichever is smaller), converting a per-source timeout into a
/// [`G1ToolStatus::TimedOut`] [`G1ToolOutput`] without ever panicking.
///
/// The helper keeps [`execute_g1`] focused on the fan-out shape —
/// per-source timeout handling lives here so the orchestrator does
/// not need a `match` arm per disposition.
async fn run_g1_source<S>(
    source: &S,
    query: &G1Query,
    per_source_deadline: Duration,
) -> G1ToolOutput
where
    S: G1Source + ?Sized,
{
    let kind = source.kind();
    let start = std::time::Instant::now();
    tokio::time::timeout(per_source_deadline, source.execute(query))
        .await
        .unwrap_or_else(|_| {
            let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            G1ToolOutput {
                kind,
                status: G1ToolStatus::TimedOut,
                elapsed_ms,
                payload: serde_json::Value::Null,
                error: Some(format!(
                    "per-source deadline {} ms exceeded",
                    per_source_deadline.as_millis()
                )),
            }
        })
}

/// Poll a `Vec` of pinned-boxed futures concurrently and collect every
/// output once all are ready.
///
/// Behaviourally equivalent to `futures::future::join_all` but avoids
/// pulling the `futures` crate as a workspace dependency (per WO-0047
/// `acceptance` AC18 — `tokio` ships everything we need for a 4-way
/// fan-out).  Each `poll_fn` cycle iterates every still-pending
/// future and re-registers their wakers, so the moment any inner
/// `tokio::time::sleep` fires the outer future is re-polled and the
/// newly-ready slots are drained.
async fn join_all_g1<'a, T>(
    mut futures: Vec<Pin<Box<dyn Future<Output = T> + Send + 'a>>>,
) -> Vec<T>
where
    T: 'a,
{
    let len = futures.len();
    let mut slots: Vec<Option<T>> = (0..len).map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut any_pending = false;
        for (i, fut) in futures.iter_mut().enumerate() {
            if slots[i].is_some() {
                continue;
            }
            match fut.as_mut().poll(cx) {
                Poll::Ready(out) => {
                    slots[i] = Some(out);
                }
                Poll::Pending => {
                    any_pending = true;
                }
            }
        }
        if any_pending {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    slots
        .into_iter()
        .map(|r| r.expect("join_all_g1: every slot must be filled before returning"))
        .collect()
}

/// G1 (Structural) parallel-execution orchestrator.
///
/// Master-plan §5.1 lines 420-446 prescribes the fan-out shape:
/// `Query → ALL of {tree-sitter, Serena, ast-grep, diagnostics-bridge}
/// run in parallel`, with a 5 s overall deadline so partial outcomes
/// stay usable when one source stalls.
///
/// Implementation:
///
/// 1. Cap each source's per-call timeout at `min(deadline,
///    G1_PER_SOURCE_DEADLINE)` so the master deadline always wins
///    on a true global stall.
/// 2. Build one boxed future per source via `run_g1_source` and
///    poll them concurrently through `join_all_g1` (single-task
///    poll-fn fan-out — equivalent to `futures::join_all` but
///    pulls in zero new dependencies, per WO-0047 AC18).
/// 3. Wrap the whole join in an outer `tokio::time::timeout(deadline,
///    ...)`.  On `Err(Elapsed)`, return a [`G1Outcome`] with
///    [`G1ToolStatus::TimedOut`] placeholders for every source and
///    `master_timed_out = true` so downstream code never sees an
///    empty result vector when the master deadline fires.
///
/// The orchestrator never `panic!`s and never `?` propagates an error
/// out — partial results are valid output per master-plan §5.1.
///
/// # Examples
///
/// ```no_run
/// use std::time::Duration;
/// use ucil_daemon::executor::{
///     execute_g1, G1Query, G1Source, G1_MASTER_DEADLINE,
/// };
///
/// # async fn demo(sources: Vec<Box<dyn G1Source>>) {
/// let q = G1Query {
///     symbol: "foo".to_owned(),
///     file_path: std::path::PathBuf::from("src/lib.rs"),
///     line: 1,
///     column: 1,
/// };
/// let outcome = execute_g1(q, sources, G1_MASTER_DEADLINE).await;
/// assert!(!outcome.master_timed_out || !outcome.results.is_empty());
/// # }
/// ```
#[tracing::instrument(
    name = "ucil.group.structural",
    level = "debug",
    skip(sources),
    fields(symbol = %query.symbol, source_count = sources.len()),
)]
pub async fn execute_g1<S>(query: G1Query, sources: Vec<Box<S>>, deadline: Duration) -> G1Outcome
where
    S: G1Source + ?Sized,
{
    let per_source_deadline = std::cmp::min(deadline, G1_PER_SOURCE_DEADLINE);
    let start = std::time::Instant::now();

    let mut futures: Vec<Pin<Box<dyn Future<Output = G1ToolOutput> + Send + '_>>> =
        Vec::with_capacity(sources.len());
    let q_ref = &query;
    for s in &sources {
        futures.push(Box::pin(run_g1_source(
            s.as_ref(),
            q_ref,
            per_source_deadline,
        )));
    }

    let outer = tokio::time::timeout(deadline, join_all_g1(futures)).await;
    let wall_elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    outer.map_or_else(
        |_| {
            let results = sources
                .iter()
                .map(|s| G1ToolOutput {
                    kind: s.kind(),
                    status: G1ToolStatus::TimedOut,
                    elapsed_ms: wall_elapsed_ms,
                    payload: serde_json::Value::Null,
                    error: Some(format!(
                        "G1 master deadline {} ms elapsed",
                        deadline.as_millis()
                    )),
                })
                .collect();
            G1Outcome {
                results,
                wall_elapsed_ms,
                master_timed_out: true,
            }
        },
        |results| G1Outcome {
            results,
            wall_elapsed_ms,
            master_timed_out: false,
        },
    )
}

// ── G1 Fusion (P2-W7-F02 / WO-0048) ───────────────────────────────────────
//
// Master-plan §5.1 lines 430-442 prescribes the post-orchestrator fusion
// step: merge per-source outputs by source location, union unique fields,
// and resolve conflicts by source authority Serena > tree-sitter >
// ast-grep > diagnostics.  Production wiring of real subprocess clients
// into the fusion path is deferred to P2-W7-F05 (`find_references`); F02
// ships only the fusion algorithm + types over a [`G1Outcome`] produced
// by [`execute_g1`] (which this WO does not modify — see executor.rs
// preamble paragraph for WO-0047 in `lib.rs`).

/// Source location key used to merge G1 fusion entries.
///
/// Master-plan §5.1 line 430 ("merge by location") groups per-source
/// `G1ToolOutput.payload` entries by `(file_path, start_line,
/// end_line)` so two sources reporting on the same definition collapse
/// into one fused entry.  Lines are 1-based; `start_line == end_line`
/// is permitted (single-line entries).
///
/// The `Hash` + `Eq` pair enables `HashMap`-keyed grouping; `Ord`
/// enables deterministic output sorting via `BTreeMap` iteration.
/// `Default` is derived transitively to support the `Default` derive
/// on [`G1FusedEntry`] (per WO-0048 `scope_in`).
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Hash,
    Ord,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct G1FusedLocation {
    /// Path to the file the entry refers to.  Relative or absolute is
    /// caller-defined; fusion does not normalise.
    pub file_path: PathBuf,
    /// 1-based start line of the entry.
    pub start_line: u32,
    /// 1-based end line of the entry.  Equal to `start_line` for
    /// single-line entries.
    pub end_line: u32,
}

/// Per-source location-bearing payload entry consumed by [`fuse_g1`].
///
/// Every [`G1ToolOutput::payload`] whose [`G1ToolOutput::status`] is
/// [`G1ToolStatus::Available`] is expected to deserialize into
/// `Vec<G1FusionEntry>`.  Production wiring (P2-W7-F05
/// `find_references`) is responsible for adapting raw source outputs
/// (Serena hover JSON, tree-sitter AST snippets, ast-grep matches,
/// diagnostics bundles) into this normalised shape; F02 ships only the
/// fusion layer that consumes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct G1FusionEntry {
    /// Source location of this entry.
    pub location: G1FusedLocation,
    /// Free-form per-source fields.  Field-name collisions across
    /// sources at the same location are resolved by `authority_rank`
    /// during fusion.
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// One field-name conflict recorded during fusion.
///
/// Recorded when two or more sources contributed non-equal
/// `serde_json::Value`s for the same field key at the same location.
/// `winner` is the higher-authority source per `authority_rank`;
/// `losers` carries the lower-authority `(source, value)` pairs whose
/// values were not equal to `winner_value`, ordered by authority rank
/// ascending.  Equal-value contributors are corroboration, not
/// conflicts, and are NOT recorded here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct G1Conflict {
    /// Field name where the conflict arose.
    pub field: String,
    /// Higher-authority source whose value won.
    pub winner: G1ToolKind,
    /// Winning value (also stored under `fields[field]` of the parent
    /// [`G1FusedEntry`]).
    pub winner_value: serde_json::Value,
    /// Lower-authority `(source, value)` pairs whose values were not
    /// equal to `winner_value`, ordered by authority rank ascending.
    pub losers: Vec<(G1ToolKind, serde_json::Value)>,
}

/// One fused entry in the [`fuse_g1`] output — a merged-by-location
/// projection of every contributing source's [`G1FusionEntry`].
///
/// `contributing_sources` is sorted by authority (Serena first,
/// Diagnostics last) so a reader can spot the highest-authority
/// contributor without scanning `conflicts`.  `Default` derive enables
/// the runtime-only mutation variant for AC21 per WO-0046 lessons.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct G1FusedEntry {
    /// Source location all contributing entries share.
    pub location: G1FusedLocation,
    /// Sources that contributed to this entry, ordered by authority
    /// rank (lowest rank first).
    pub contributing_sources: Vec<G1ToolKind>,
    /// Unioned per-source fields with conflicts resolved by source
    /// authority.
    pub fields: serde_json::Map<String, serde_json::Value>,
    /// Field-level conflicts recorded during fusion, ordered by field
    /// name lexicographically.
    pub conflicts: Vec<G1Conflict>,
}

/// Aggregate output of one [`fuse_g1`] call.
///
/// `entries` is sorted by `(file_path, start_line, end_line)` for
/// deterministic output (`BTreeMap` iteration order).  `master_timed_out`
/// is forwarded from the input [`G1Outcome::master_timed_out`] so
/// downstream code does not have to thread both.  `source_dispositions`
/// carries `(kind, status)` pairs in input order so a
/// [`G1ToolStatus::Errored`] or [`G1ToolStatus::TimedOut`] can be
/// surfaced even though those sources contribute no entries to the
/// fused result.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct G1FusedOutcome {
    /// Fused per-location entries, sorted by location key.
    pub entries: Vec<G1FusedEntry>,
    /// Forwarded from the input [`G1Outcome::master_timed_out`].
    pub master_timed_out: bool,
    /// Per-source dispositions in input order so downstream code can
    /// observe degraded paths even though they contribute no entries.
    pub source_dispositions: Vec<(G1ToolKind, G1ToolStatus)>,
}

/// Authority rank for a [`G1ToolKind`] — lower is higher authority.
///
/// Master-plan §5.1 prescribes Serena > tree-sitter > ast-grep >
/// diagnostics; we encode this as `0` through `3` so the natural
/// ordering on `u8` matches the authority ordering.  P2-W7-F08
/// (WO-0055, DEC-0014) added rank `4` for [`G1ToolKind::Scip`] —
/// below the four pre-existing sources because SCIP is an offline
/// batch indexer (a freshly-indexed Serena/LSP signal beats a stale
/// SCIP entry whenever they conflict).  Master-plan §22 line 616:
/// "Source authority as soft guidance: LSP/AST → SCIP → Dep tools →
/// KG → Text".
///
/// The exhaustive `match` is the compile-time guarantee that any
/// future [`G1ToolKind`] variant gains an explicit rank — adding a
/// variant without updating this function fails the build.
pub(crate) const fn authority_rank(kind: G1ToolKind) -> u8 {
    match kind {
        G1ToolKind::Serena => 0,
        G1ToolKind::TreeSitter => 1,
        G1ToolKind::AstGrep => 2,
        G1ToolKind::Diagnostics => 3,
        G1ToolKind::Scip => 4,
    }
}

/// Fuse a [`G1Outcome`] into a [`G1FusedOutcome`] per master-plan §5.1
/// lines 430-442.
///
/// Algorithm:
///
/// 1. Build `source_dispositions` from `outcome.results` in input
///    order so callers can observe degraded paths (e.g. a
///    [`G1ToolStatus::TimedOut`] source) even when no entries from
///    that source land in the fused output.
/// 2. For each [`G1ToolStatus::Available`] result, attempt to decode
///    `payload` as `Vec<G1FusionEntry>`.  Decode failures are logged
///    at `warn!` and the source is silently skipped — partial results
///    are valid output (master-plan §5.1 line 445), and a misshapen
///    payload from one source MUST NOT poison fusion of the rest.
/// 3. Group entries by [`G1FusedLocation`] (file + line range).
/// 4. Within each group, union the per-entry `fields` map.  On
///    field-name collision with non-equal values, the higher-authority
///    source wins via `authority_rank` and a [`G1Conflict`] row is
///    recorded.  Equal-value contributors are corroboration, not
///    conflicts, and are not recorded.
/// 5. Sort fused entries by location for deterministic output —
///    `BTreeMap` iteration order makes this free.
///
/// Pure CPU-bound transform on the input — no `tokio::spawn`, no
/// `tokio::time::timeout`.  The orchestrator ([`execute_g1`]) already
/// owns the deadline guard.  The function never `panic!`s and never
/// `?`-propagates an error out — partial results are valid output.
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
/// use ucil_daemon::executor::{
///     execute_g1, fuse_g1, G1Query, G1Source, G1_MASTER_DEADLINE,
/// };
///
/// # async fn demo(sources: Vec<Box<dyn G1Source>>) {
/// let q = G1Query {
///     symbol: "foo".to_owned(),
///     file_path: PathBuf::from("src/lib.rs"),
///     line: 1,
///     column: 1,
/// };
/// let raw = execute_g1(q, sources, G1_MASTER_DEADLINE).await;
/// let fused = fuse_g1(&raw);
/// assert!(fused
///     .entries
///     .iter()
///     .all(|e| !e.contributing_sources.is_empty()));
/// # }
/// ```
#[tracing::instrument(
    name = "ucil.group.structural.fusion",
    level = "debug",
    skip(outcome),
    fields(
        input_results = outcome.results.len(),
        input_master_timed_out = outcome.master_timed_out,
    ),
)]
pub fn fuse_g1(outcome: &G1Outcome) -> G1FusedOutcome {
    // Step 1: dispositions in input order.  `G1ToolKind` is `Copy`
    // (per its derive); `G1ToolStatus` is `Clone`-only so we clone
    // the status field explicitly.
    let source_dispositions: Vec<(G1ToolKind, G1ToolStatus)> = outcome
        .results
        .iter()
        .map(|r| (r.kind, r.status.clone()))
        .collect();

    // Step 2: decode each Available source's payload into
    // `Vec<G1FusionEntry>`.  A decode failure on one source is logged
    // and the source skipped — partial-results semantics per
    // master-plan §5.1 line 445.
    let mut per_source: Vec<(G1ToolKind, Vec<G1FusionEntry>)> = Vec::new();
    for output in &outcome.results {
        if output.status != G1ToolStatus::Available {
            continue;
        }
        match serde_json::from_value::<Vec<G1FusionEntry>>(output.payload.clone()) {
            Ok(entries) => per_source.push((output.kind, entries)),
            Err(e) => tracing::warn!(
                target = "ucil.group.structural.fusion",
                kind = ?output.kind,
                err = %e,
                "payload decode failed; source skipped",
            ),
        }
    }

    // Step 3: group by location.  `BTreeMap` (not `HashMap`) so
    // iteration order is the deterministic `G1FusedLocation` ordering
    // — eliminates a sort pass at the end.  `type_complexity` allowed
    // here: a top-level type alias would need a `'_` lifetime
    // parameter and noises up the public surface for a single-use
    // intermediate.
    #[allow(clippy::type_complexity)]
    let mut groups: BTreeMap<
        G1FusedLocation,
        Vec<(G1ToolKind, &serde_json::Map<String, serde_json::Value>)>,
    > = BTreeMap::new();
    for (kind, entries) in &per_source {
        for entry in entries {
            groups
                .entry(entry.location.clone())
                .or_default()
                .push((*kind, &entry.fields));
        }
    }

    // Step 4: per-group fusion.
    let mut entries: Vec<G1FusedEntry> = Vec::with_capacity(groups.len());
    for (location, contributors) in groups {
        // contributing_sources, sorted by authority rank ascending.
        let mut contributing_sources: Vec<G1ToolKind> =
            contributors.iter().map(|(k, _)| *k).collect();
        contributing_sources.sort_by_key(|k| authority_rank(*k));

        // Per-field grouping inside this location: `BTreeMap` keyed by
        // field name so the `conflicts` Vec ends up in field-name
        // lexicographic order without an explicit sort.
        let mut by_field: BTreeMap<String, Vec<(G1ToolKind, &serde_json::Value)>> = BTreeMap::new();
        for (kind, map) in &contributors {
            for (name, value) in *map {
                by_field
                    .entry(name.clone())
                    .or_default()
                    .push((*kind, value));
            }
        }

        let mut fields: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        let mut conflicts: Vec<G1Conflict> = Vec::new();
        for (name, candidates) in by_field {
            if candidates.len() == 1 {
                // Single contributor: copy verbatim, no conflict.
                let (_kind, value) = candidates[0];
                fields.insert(name, value.clone());
            } else {
                // Multiple contributors: highest-authority wins.
                let mut sorted = candidates;
                sorted.sort_by_key(|(k, _)| authority_rank(*k));
                let (winner_kind, winner_value) = sorted[0];
                fields.insert(name.clone(), winner_value.clone());
                let losers: Vec<(G1ToolKind, serde_json::Value)> = sorted[1..]
                    .iter()
                    .filter(|(_, v)| *v != winner_value)
                    .map(|(k, v)| (*k, (*v).clone()))
                    .collect();
                if !losers.is_empty() {
                    conflicts.push(G1Conflict {
                        field: name,
                        winner: winner_kind,
                        winner_value: winner_value.clone(),
                        losers,
                    });
                }
            }
        }

        entries.push(G1FusedEntry {
            location,
            contributing_sources,
            fields,
            conflicts,
        });
    }

    G1FusedOutcome {
        entries,
        master_timed_out: outcome.master_timed_out,
        source_dispositions,
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────
//
// Per DEC-0005 (WO-0006 module-coherence commits), tests live at module
// root — NOT wrapped in `#[cfg(test)] mod tests { … }` — so the frozen
// acceptance selector `executor::test_treesitter_to_kg_pipeline`
// resolves to `ucil_daemon::executor::test_treesitter_to_kg_pipeline`
// without a `tests::` intermediate.

#[cfg(test)]
use tempfile::TempDir;

/// Locate the repo's `tests/fixtures/rust-project` directory regardless
/// of whether the test runs under the workspace root (`cargo nextest
/// run` from repo root) or the crate dir (`cargo nextest run -p
/// ucil-daemon` from crate dir).
///
/// The two paths differ by one parent:
/// * workspace-root cwd → `./tests/fixtures/rust-project`
/// * crate-root cwd     → `../../tests/fixtures/rust-project`
///
/// Both shapes are probed so the test passes under every invocation the
/// master workflow uses.
#[cfg(test)]
pub(crate) fn rust_project_fixture() -> PathBuf {
    let candidates = [
        PathBuf::from("tests/fixtures/rust-project"),
        PathBuf::from("../../tests/fixtures/rust-project"),
    ];
    for c in &candidates {
        if c.is_dir() {
            return c.clone();
        }
    }
    panic!(
        "could not locate tests/fixtures/rust-project from cwd {:?}",
        std::env::current_dir()
    );
}

/// Frozen acceptance selector for feature `P1-W4-F04` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon executor::test_treesitter_to_kg_pipeline`.
///
/// Exercises the full pipeline end-to-end against a real fixture rust
/// file and asserts:
///
/// 1. Entities are present in the KG after the run (via
///    [`KnowledgeGraph::list_entities_by_file`]).
/// 2. Every inserted row carries `source_tool = "tree-sitter"`.
/// 3. Every inserted row carries `language = "rust"`.
/// 4. Re-running the pipeline on the same file is idempotent — the
///    entity count is stable.
/// 5. The pipeline returns the same insert count on both runs
///    (ON CONFLICT fires instead of appending duplicates).
#[cfg(test)]
#[test]
fn test_treesitter_to_kg_pipeline() {
    use ucil_core::KnowledgeGraph;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let kg_path = tmp.path().join("knowledge.db");
    let mut kg = KnowledgeGraph::open(&kg_path).expect("KnowledgeGraph::open must succeed");

    let fixture = rust_project_fixture();
    let target = fixture.join("src/util.rs");
    assert!(
        target.is_file(),
        "fixture file {target:?} must exist in the repo"
    );

    let mut pipeline = IngestPipeline::new();
    let first = pipeline
        .ingest_file(&mut kg, &target)
        .expect("first ingest must succeed");
    assert!(
        first > 0,
        "first ingest must upsert at least one symbol (got {first})"
    );

    let entities = kg
        .list_entities_by_file(&target.display().to_string())
        .expect("list_entities_by_file must succeed");
    assert!(
        !entities.is_empty(),
        "list_entities_by_file returned no rows after ingest"
    );

    for e in &entities {
        assert_eq!(
            e.source_tool.as_deref(),
            Some(SOURCE_TOOL),
            "every inserted entity must carry source_tool = {SOURCE_TOOL:?}: got {:?}",
            e.source_tool
        );
        assert_eq!(
            e.language.as_deref(),
            Some("rust"),
            "every inserted entity must carry language = \"rust\": got {:?}",
            e.language
        );
        assert_eq!(
            e.t_valid_from.as_deref(),
            Some(TREE_SITTER_VALID_FROM),
            "every inserted entity must carry t_valid_from = {TREE_SITTER_VALID_FROM:?}"
        );
        assert!(
            e.start_line.unwrap_or(0) >= 1,
            "start_line must be 1-based positive: got {:?}",
            e.start_line
        );
        assert!(
            e.qualified_name.is_some(),
            "qualified_name must be non-NULL for ON CONFLICT idempotency"
        );
    }

    // Pipeline must have inserted at least one row of each of the kinds
    // we know the fixture contains (functions, enums).  The exact set
    // varies by fixture content, so assert presence, not equality.
    let has_kind = |k: &str| entities.iter().any(|e| e.kind == k);
    assert!(
        has_kind("function") || has_kind("method"),
        "fixture must contribute ≥1 function or method; got kinds {:?}",
        entities.iter().map(|e| &e.kind).collect::<Vec<_>>()
    );

    // ── Idempotency: second ingest leaves entity count stable ──────
    let count_before = entities.len();
    let second = pipeline
        .ingest_file(&mut kg, &target)
        .expect("second ingest must succeed");
    assert_eq!(
        second, first,
        "second ingest must upsert the same number of symbols \
         (first={first}, second={second})"
    );

    let after = kg
        .list_entities_by_file(&target.display().to_string())
        .expect("list_entities_by_file must succeed");
    assert_eq!(
        after.len(),
        count_before,
        "re-running the pipeline must not add rows \
         (before={count_before}, after={})",
        after.len()
    );

    // ── Idempotency: ON CONFLICT DO UPDATE bumps access_count ──────
    //
    // A second ingest should increment every row's access_count; the
    // exact starting value is an implementation detail of
    // `upsert_entity`'s `access_count = access_count + 1` path, but we
    // can assert the count is strictly > 0 after two runs.
    let access_count_after: i64 = kg
        .conn()
        .query_row(
            "SELECT SUM(access_count) FROM entities WHERE file_path = ?1;",
            rusqlite::params![target.display().to_string()],
            |row| row.get::<_, i64>(0),
        )
        .expect("SUM(access_count) read must succeed");
    assert!(
        access_count_after >= i64::try_from(entities.len()).unwrap_or(i64::MAX),
        "each row's access_count must be >= 1 after second ingest \
         (sum after={access_count_after}, row count={})",
        entities.len()
    );
}
/// Multi-file ingest: each file gets its own transaction scope (no
/// cross-file atomic requirement) but every file's symbols land in the
/// knowledge graph.
#[cfg(test)]
#[test]
fn test_ingest_multi_file_isolation() {
    use ucil_core::KnowledgeGraph;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let kg_path = tmp.path().join("kg.db");
    let mut kg = KnowledgeGraph::open(&kg_path).expect("KnowledgeGraph::open must succeed");

    let fixture = rust_project_fixture();
    let files = [fixture.join("src/util.rs"), fixture.join("src/parser.rs")];
    let mut pipeline = IngestPipeline::new();
    for f in &files {
        assert!(f.is_file(), "fixture file {f:?} must exist");
        let n = pipeline
            .ingest_file(&mut kg, f)
            .expect("ingest_file must succeed");
        assert!(n > 0, "ingest {f:?} must contribute ≥1 symbol");
    }

    for f in &files {
        let rows = kg
            .list_entities_by_file(&f.display().to_string())
            .expect("list_entities_by_file must succeed");
        assert!(!rows.is_empty(), "{f:?} must produce ≥1 entity");
        for r in &rows {
            assert_eq!(r.source_tool.as_deref(), Some(SOURCE_TOOL));
            assert_eq!(r.file_path, f.display().to_string());
        }
    }
}

/// `ingest_file` rejects unknown extensions before opening the file or
/// invoking tree-sitter — the error type is
/// [`ExecutorError::UnsupportedExtension`] and the offending path is
/// carried through.
#[cfg(test)]
#[test]
fn test_ingest_rejects_unsupported_extension() {
    use ucil_core::KnowledgeGraph;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let kg_path = tmp.path().join("kg.db");
    let mut kg = KnowledgeGraph::open(&kg_path).expect("KnowledgeGraph::open must succeed");

    // `xyz` is not in the extension table; path need not exist — the
    // extension check happens before any `fs::read_to_string` call.
    let bogus = tmp.path().join("unknown.xyz");

    let mut pipeline = IngestPipeline::new();
    let err = pipeline
        .ingest_file(&mut kg, &bogus)
        .expect_err("unsupported extension must error");
    match err {
        ExecutorError::UnsupportedExtension { path } => {
            assert_eq!(path, bogus);
        }
        other => panic!("expected UnsupportedExtension, got {other:?}"),
    }
}

/// `language_from_extension` recognises every extension the module-level
/// table documents — a regression fence against an accidental removal
/// of one mapping.
#[cfg(test)]
#[test]
fn test_language_from_extension_table() {
    let cases: &[(&str, Language)] = &[
        ("a.rs", Language::Rust),
        ("a.py", Language::Python),
        ("a.ts", Language::TypeScript),
        ("a.tsx", Language::TypeScript),
        ("a.js", Language::JavaScript),
        ("a.jsx", Language::JavaScript),
        ("a.mjs", Language::JavaScript),
        ("a.cjs", Language::JavaScript),
        ("a.go", Language::Go),
        ("a.java", Language::Java),
        ("a.c", Language::C),
        ("a.h", Language::C),
        ("a.cc", Language::Cpp),
        ("a.cpp", Language::Cpp),
        ("a.cxx", Language::Cpp),
        ("a.hpp", Language::Cpp),
        ("a.hh", Language::Cpp),
        ("a.hxx", Language::Cpp),
        ("a.rb", Language::Ruby),
        ("a.sh", Language::Bash),
        ("a.bash", Language::Bash),
        ("a.json", Language::Json),
    ];
    for (name, expected) in cases {
        let got = language_from_extension(Path::new(name));
        assert_eq!(got, Some(*expected), "extension {name:?}");
    }

    // Unknown extensions return None.
    assert_eq!(language_from_extension(Path::new("a.xyz")), None);
    // Extensionless paths return None.
    assert_eq!(language_from_extension(Path::new("Makefile")), None);
    // Case-insensitive match: `.RS` also resolves to Rust.
    assert_eq!(
        language_from_extension(Path::new("a.RS")),
        Some(Language::Rust)
    );
}

/// `kind_tag` covers every [`SymbolKind`] variant with a stable lowercase
/// tag matching Serde's `rename_all = "snake_case"`.
#[cfg(test)]
#[test]
fn test_kind_tag_covers_all_variants() {
    let cases: &[(SymbolKind, &str)] = &[
        (SymbolKind::Function, "function"),
        (SymbolKind::Method, "method"),
        (SymbolKind::Class, "class"),
        (SymbolKind::Struct, "struct"),
        (SymbolKind::Enum, "enum"),
        (SymbolKind::Trait, "trait"),
        (SymbolKind::Interface, "interface"),
        (SymbolKind::TypeAlias, "type_alias"),
        (SymbolKind::Constant, "constant"),
        (SymbolKind::Module, "module"),
    ];
    for (k, tag) in cases {
        assert_eq!(kind_tag(*k), *tag, "{k:?}");
    }
}

/// `build_qualified_name` produces the
/// `<file>::<name>@<line>:<col>` shape the ON CONFLICT path relies on,
/// and is stable across identical inputs.
#[cfg(test)]
#[test]
fn test_build_qualified_name_shape_and_stability() {
    let sym = ExtractedSymbol {
        name: "foo".to_owned(),
        kind: SymbolKind::Function,
        file_path: PathBuf::from("src/a.rs"),
        language: Language::Rust,
        start_line: 10,
        start_col: 1,
        end_line: 15,
        end_col: 2,
        signature: None,
        doc_comment: None,
    };
    let q1 = build_qualified_name("src/a.rs", &sym);
    let q2 = build_qualified_name("src/a.rs", &sym);
    assert_eq!(q1, "src/a.rs::foo@10:1");
    assert_eq!(q1, q2, "qualified_name must be stable across calls");

    // Distinct start_line → distinct qualified_name (disambiguates
    // name-colliding methods like three `fn fmt` impls).
    let sym2 = ExtractedSymbol {
        start_line: 20,
        ..sym
    };
    let q3 = build_qualified_name("src/a.rs", &sym2);
    assert_ne!(q1, q3);
}

/// `compute_source_hash` is deterministic across calls and returns a
/// 16-hex-char string.
#[cfg(test)]
#[test]
fn test_compute_source_hash_deterministic_and_hex16() {
    let src = "fn foo() {}\nfn bar() {}\n";
    let sym = ExtractedSymbol {
        name: "foo".to_owned(),
        kind: SymbolKind::Function,
        file_path: PathBuf::from("x.rs"),
        language: Language::Rust,
        start_line: 1,
        start_col: 1,
        end_line: 1,
        end_col: 12,
        signature: None,
        doc_comment: None,
    };
    let h1 = compute_source_hash(src, &sym);
    let h2 = compute_source_hash(src, &sym);
    assert_eq!(h1, h2, "source_hash must be deterministic");
    assert_eq!(h1.len(), 16, "source_hash must be 16 hex chars: {h1:?}");
    assert!(
        h1.chars().all(|c| c.is_ascii_hexdigit()),
        "source_hash must be pure hex: {h1:?}"
    );

    // Distinct ranges feed the hasher distinct line/col inputs; assert
    // the hash shape survives the second call too (value-equality with
    // the first call would be a tolerated `SipHash-1-3` collision,
    // which is why we only assert shape, not difference).
    let sym_different_line = ExtractedSymbol {
        start_line: 2,
        end_line: 2,
        ..sym
    };
    let h3 = compute_source_hash(src, &sym_different_line);
    assert_eq!(
        h3.len(),
        16,
        "second hash must also be 16 hex chars: {h3:?}"
    );
    assert!(
        h3.chars().all(|c| c.is_ascii_hexdigit()),
        "second hash must be pure hex: {h3:?}"
    );
}

/// Default impl exists and matches `IngestPipeline::new` — the pipeline
/// is movable into thread-local handles / struct fields that only have a
/// `Default` bound.
#[cfg(test)]
#[test]
fn test_ingest_pipeline_default_available() {
    let _p: IngestPipeline = IngestPipeline::default();
}

// ── Serena hover fusion: scripted fake + test ────────────────────────────

/// Scripted [`SerenaHoverClient`] impl driving [`test_serena_g1_fusion`].
///
/// The pattern mirrors
/// `ucil-lsp-diagnostics::call_hierarchy::fake_serena_client`
/// (WO-0015, already live and verifier-passed): a `Mutex<Vec<_>>` of
/// `(key, response)` tuples scripted at construction time and looked up
/// on each call by matching `key` against the request.  This is NOT a
/// mock of Serena's MCP wire format — per DEC-0008 §4 it implements
/// UCIL's own [`SerenaHoverClient`] trait, which is the dependency-
/// inversion seam, so "mocks of Serena critical deps" (root
/// `CLAUDE.md`) does not apply.
///
/// Responses are wrapped in [`std::sync::Arc`] so the fake can return a
/// clone for the matched entry — [`HoverFetchError`] is not `Clone` by
/// design (transport errors carry strings that may be large), and the
/// `Arc` sidesteps that restriction without widening `HoverFetchError`'s
/// trait bounds.
#[cfg(test)]
mod fake_serena_hover_client {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use ucil_core::knowledge_graph::SymbolResolution;

    use super::{HoverDoc, HoverFetchError, SerenaHoverClient};

    /// `(qualified_name, response)` tuples: the fake's `hover` method
    /// finds the first tuple whose `qualified_name` matches the request
    /// and returns a clone of its `response`; unscripted symbols resolve
    /// to `Ok(None)` (mirroring LSP "no hover info" semantics).
    pub(super) type HoverScript = Vec<(String, Arc<Result<Option<HoverDoc>, HoverFetchError>>)>;

    /// Scripted fake [`SerenaHoverClient`] impl.  See module docs.
    pub(super) struct ScriptedFakeSerenaHoverClient {
        by_qname: Mutex<HoverScript>,
    }

    impl ScriptedFakeSerenaHoverClient {
        /// Construct a fake pre-loaded with `script`.
        pub(super) fn new(script: HoverScript) -> Self {
            Self {
                by_qname: Mutex::new(script),
            }
        }
    }

    #[async_trait]
    impl SerenaHoverClient for ScriptedFakeSerenaHoverClient {
        async fn hover(
            &self,
            resolution: &SymbolResolution,
        ) -> Result<Option<HoverDoc>, HoverFetchError> {
            // Clone-out under the lock so the guard drops before any
            // `await` point (there is none here but the discipline keeps
            // `clippy::await_holding_lock` satisfied).
            let script = self
                .by_qname
                .lock()
                .expect("ScriptedFakeSerenaHoverClient mutex poisoned")
                .clone();
            let key = resolution.qualified_name.as_deref().unwrap_or("");
            for (scripted_qname, response) in script {
                if scripted_qname == key {
                    // `Arc<Result<_, _>>` — clone-out the inner value
                    // (the `Result` contents are `Clone` by virtue of
                    // the per-variant derives; for `HoverFetchError`
                    // we manually reconstruct the variant).
                    return match response.as_ref() {
                        Ok(opt) => Ok(opt.clone()),
                        Err(HoverFetchError::Channel(s)) => {
                            Err(HoverFetchError::Channel(s.clone()))
                        }
                        Err(HoverFetchError::Decode(s)) => Err(HoverFetchError::Decode(s.clone())),
                        Err(HoverFetchError::Timeout(d)) => Err(HoverFetchError::Timeout(*d)),
                    };
                }
            }
            Ok(None)
        }
    }
}

/// Frozen acceptance selector for feature `P1-W5-F02` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon executor::test_serena_g1_fusion`.
///
/// Exercises [`enrich_find_definition`] against three scripted
/// scenarios to prove the DEC-0008 dependency-inversion seam and the
/// master-plan §13.4 best-effort contract both hold:
///
/// 1. **Scenario A — Serena ACTIVE returns hover**: the scripted fake
///    returns `Ok(Some(doc))` for a given `qualified_name`; the fused
///    result carries that exact `HoverDoc` verbatim plus the original
///    resolution + (empty) callers list untouched.
/// 2. **Scenario B — Serena absent**: the caller passes `client = None`
///    (Serena-degraded deployment); the fused result carries
///    `hover = None` and a non-empty `callers` list threaded through
///    unchanged, proving the fusion is a passthrough on the zero-
///    upstream path.
/// 3. **Scenario C — Serena returns Err**: the scripted fake returns
///    `Err(HoverFetchError::Timeout(..))`; the fused result carries
///    `hover = None` — errors are logged at `warn!` per the function's
///    rustdoc contract but suppressed from the fused result so a
///    Serena outage never breaks a G1 response.
///
/// The test does not assert on `tracing` output (`tracing-test` is not
/// a workspace dependency and adding it is out of scope for this WO);
/// the `warn!()` call is documented in the fusion function's rustdoc.
#[cfg(test)]
#[tokio::test(flavor = "current_thread")]
async fn test_serena_g1_fusion() {
    use std::sync::Arc;
    use std::time::Duration;

    use ucil_core::knowledge_graph::SymbolResolution;

    use self::fake_serena_hover_client::ScriptedFakeSerenaHoverClient;

    // Shared fixture: the resolved definition under test.  Field shape
    // matches `ucil_core::knowledge_graph::SymbolResolution`'s declared
    // fields (no `Default` derive on that type, so we construct every
    // field explicitly — the WO's `scope_out` forbids modifying
    // `ucil-core`).
    let resolution = SymbolResolution {
        id: Some(42),
        qualified_name: Some("mymod::foo".to_owned()),
        file_path: "src/lib.rs".to_owned(),
        start_line: Some(42),
        signature: Some("fn foo() -> Bar".to_owned()),
        doc_comment: None,
        parent_module: Some("mymod".to_owned()),
    };

    // ── Scenario A: Serena ACTIVE returns hover ──────────────────────
    let scripted_doc = HoverDoc {
        markdown: "## foo\n\nReturns bar".to_owned(),
        source: HoverSource::Serena,
    };
    let client_a = ScriptedFakeSerenaHoverClient::new(vec![(
        "mymod::foo".to_owned(),
        Arc::new(Ok(Some(scripted_doc.clone()))),
    )]);
    let result_a = enrich_find_definition(resolution.clone(), Vec::new(), Some(&client_a)).await;
    assert_eq!(
        result_a.hover,
        Some(scripted_doc),
        "Scenario A: Serena-active path must return the scripted HoverDoc verbatim",
    );
    assert_eq!(
        result_a.resolution, resolution,
        "Scenario A: resolution must thread through unchanged",
    );
    assert!(
        result_a.callers.is_empty(),
        "Scenario A: empty callers list must round-trip as empty",
    );

    // ── Scenario B: Serena absent (client = None) ────────────────────
    let callers_b = vec![Caller {
        qualified_name: Some("caller::one".to_owned()),
        file_path: "src/caller.rs".to_owned(),
        start_line: Some(10),
    }];
    let result_b = enrich_find_definition(
        resolution.clone(),
        callers_b.clone(),
        None::<&ScriptedFakeSerenaHoverClient>,
    )
    .await;
    assert!(
        result_b.hover.is_none(),
        "Scenario B: client=None (Serena-degraded) must yield hover=None",
    );
    assert_eq!(
        result_b.resolution, resolution,
        "Scenario B: resolution must thread through unchanged",
    );
    assert_eq!(
        result_b.callers, callers_b,
        "Scenario B: callers must thread through unchanged",
    );

    // ── Scenario C: Serena returns Err ───────────────────────────────
    let client_c = ScriptedFakeSerenaHoverClient::new(vec![(
        "mymod::foo".to_owned(),
        Arc::new(Err(HoverFetchError::Timeout(Duration::from_millis(500)))),
    )]);
    let result_c = enrich_find_definition(resolution.clone(), Vec::new(), Some(&client_c)).await;
    assert!(
        result_c.hover.is_none(),
        "Scenario C: Serena error path must suppress hover to None \
         (best-effort fusion per master-plan §13.4)",
    );
    assert_eq!(
        result_c.resolution, resolution,
        "Scenario C: resolution must thread through unchanged even on Serena error",
    );
}

// ── G1 parallel-execution acceptance test (P2-W7-F01) ────────────────────
//
// Per `DEC-0007` (frozen-selector module-root placement), the
// acceptance test `test_g1_parallel_execution` lives at the module
// root of `executor.rs` — NOT inside `mod tests {}` — so the
// `feature-list.json` selector
// `-p ucil-daemon executor::test_g1_parallel_execution`
// resolves cleanly without a `tests::` intermediate.
//
// Per `DEC-0008` §4 the four trait impls below (`TestG1Source` driven
// by `TestBehaviour`) are local to the test — UCIL's own abstraction
// boundary, not a mock of any external wire format.  Production
// wiring of real subprocess clients (Serena MCP plugin, ast-grep MCP
// plugin, real `ucil_treesitter::parser::Parser`, real
// `crates/ucil-lsp-diagnostics::bridge`) is deferred to P2-W7-F02
// (G1 fusion) and P2-W7-F05 (`find_references`).

/// Frozen acceptance selector for feature `P2-W7-F01` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon executor::test_g1_parallel_execution`.
///
/// Asserts three properties of [`execute_g1`] in one function (single
/// frozen selector — no separate tests, per `DEC-0007`):
///
/// 1. **Parallel timing** — 4 sources each sleeping 200 ms must
///    return with `wall_elapsed_ms` in `[180, 600)` (lower bound
///    proves at least one source's sleep elapsed; upper bound proves
///    serial 4×200 ms = 800 ms did not happen → parallelism
///    confirmed).  Dual-bound discipline per WO-0043 lessons.
/// 2. **Partial-Errored** — 1 of 4 sources returns
///    [`G1ToolStatus::Errored`]; outcome carries exactly 1 Errored +
///    3 [`G1ToolStatus::Available`] entries.
/// 3. **Partial-TimedOut** — 1 of 4 sources sleeps 6 s
///    (> [`G1_PER_SOURCE_DEADLINE`]); outcome carries exactly 1
///    [`G1ToolStatus::TimedOut`] + 3 [`G1ToolStatus::Available`]
///    entries; whole test wall-time stays under 5500 ms (the
///    per-source 4.5 s ceiling fires before the master 5 s
///    deadline).
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
pub async fn test_g1_parallel_execution() {
    use std::time::Instant;

    /// Behaviour switches for the per-test [`G1Source`] impl below.
    #[derive(Clone)]
    enum TestBehaviour {
        /// Sleep then return [`G1ToolStatus::Available`].
        Sleep(Duration),
        /// Return [`G1ToolStatus::Errored`] without any sleep.
        Error(String),
        /// Sleep longer than [`G1_PER_SOURCE_DEADLINE`] —
        /// [`run_g1_source`]'s `tokio::time::timeout` wrapper must
        /// fire and return [`G1ToolStatus::TimedOut`] before this
        /// branch returns.
        LongSleep(Duration),
    }

    /// Local [`G1Source`] impl driving the three sub-scenarios.
    /// Per `DEC-0008` §4 this is a UCIL-internal trait (the
    /// dependency-inversion seam), so a local impl in a test is
    /// not a mock of any external wire format — the same shape as
    /// `fake_serena_hover_client::ScriptedFakeSerenaHoverClient`
    /// above.
    struct TestG1Source {
        kind: G1ToolKind,
        behaviour: TestBehaviour,
    }

    #[async_trait::async_trait]
    impl G1Source for TestG1Source {
        fn kind(&self) -> G1ToolKind {
            self.kind
        }

        async fn execute(&self, _query: &G1Query) -> G1ToolOutput {
            match &self.behaviour {
                TestBehaviour::Sleep(d) => {
                    let start = Instant::now();
                    tokio::time::sleep(*d).await;
                    let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
                    G1ToolOutput {
                        kind: self.kind,
                        status: G1ToolStatus::Available,
                        elapsed_ms,
                        payload: serde_json::json!({
                            "slept_ms": u64::try_from(d.as_millis())
                                .unwrap_or(u64::MAX),
                        }),
                        error: None,
                    }
                }
                TestBehaviour::Error(msg) => G1ToolOutput {
                    kind: self.kind,
                    status: G1ToolStatus::Errored,
                    elapsed_ms: 0,
                    payload: serde_json::Value::Null,
                    error: Some(msg.clone()),
                },
                TestBehaviour::LongSleep(d) => {
                    tokio::time::sleep(*d).await;
                    // The orchestrator's per-source timeout must
                    // fire before this branch returns; if the test
                    // ever sees `Available` here, the timeout
                    // wrapper has regressed (AC21 mutation #2
                    // would land us here).
                    G1ToolOutput {
                        kind: self.kind,
                        status: G1ToolStatus::Available,
                        elapsed_ms: u64::try_from(d.as_millis()).unwrap_or(u64::MAX),
                        payload: serde_json::Value::Null,
                        error: None,
                    }
                }
            }
        }
    }

    // The four kinds, in master-plan §5.1 order.
    let all_kinds: [G1ToolKind; 4] = [
        G1ToolKind::TreeSitter,
        G1ToolKind::Serena,
        G1ToolKind::AstGrep,
        G1ToolKind::Diagnostics,
    ];

    let q = G1Query {
        symbol: "foo".to_owned(),
        file_path: PathBuf::from("src/lib.rs"),
        line: 1,
        column: 1,
    };

    // ── (a) Parallel timing: 4×200 ms sources, wall in [180, 600) ────
    let sources_a: Vec<Box<dyn G1Source + Send + Sync>> = all_kinds
        .iter()
        .map(|k| {
            Box::new(TestG1Source {
                kind: *k,
                behaviour: TestBehaviour::Sleep(Duration::from_millis(200)),
            }) as Box<dyn G1Source + Send + Sync>
        })
        .collect();
    let outcome_a = execute_g1(q.clone(), sources_a, G1_MASTER_DEADLINE).await;
    assert!(
        outcome_a.wall_elapsed_ms >= 180,
        "(a) parallel timing lower bound: wall_elapsed_ms must be >= 180 \
         (proves at least one 200 ms sleep elapsed); got {} ms",
        outcome_a.wall_elapsed_ms
    );
    assert!(
        outcome_a.wall_elapsed_ms < 600,
        "(a) parallel timing upper bound: wall_elapsed_ms must be < 600 \
         (proves serial 4x200=800 ms did not happen → parallelism confirmed); \
         got {} ms",
        outcome_a.wall_elapsed_ms
    );
    assert!(
        !outcome_a.master_timed_out,
        "(a) master_timed_out must be false on a 200 ms-per-source happy path"
    );
    assert_eq!(
        outcome_a.results.len(),
        4,
        "(a) outcome.results must contain exactly 4 entries (one per source)"
    );
    for r in &outcome_a.results {
        assert_eq!(
            r.status,
            G1ToolStatus::Available,
            "(a) every source must report Available; got {:?} for kind {:?}",
            r.status,
            r.kind
        );
    }

    // ── (b) Partial-Errored: 1 Errored + 3 Available ─────────────────
    let sources_b: Vec<Box<dyn G1Source + Send + Sync>> = vec![
        Box::new(TestG1Source {
            kind: G1ToolKind::TreeSitter,
            behaviour: TestBehaviour::Sleep(Duration::from_millis(50)),
        }),
        Box::new(TestG1Source {
            kind: G1ToolKind::Serena,
            behaviour: TestBehaviour::Error("injected".to_owned()),
        }),
        Box::new(TestG1Source {
            kind: G1ToolKind::AstGrep,
            behaviour: TestBehaviour::Sleep(Duration::from_millis(50)),
        }),
        Box::new(TestG1Source {
            kind: G1ToolKind::Diagnostics,
            behaviour: TestBehaviour::Sleep(Duration::from_millis(50)),
        }),
    ];
    let outcome_b = execute_g1(q.clone(), sources_b, G1_MASTER_DEADLINE).await;
    let errored_b = outcome_b
        .results
        .iter()
        .filter(|r| r.status == G1ToolStatus::Errored)
        .count();
    let available_b = outcome_b
        .results
        .iter()
        .filter(|r| r.status == G1ToolStatus::Available)
        .count();
    assert_eq!(
        errored_b, 1,
        "(b) outcome must contain exactly 1 Errored entry, got {errored_b} \
         (results = {:?})",
        outcome_b.results
    );
    assert_eq!(
        available_b, 3,
        "(b) outcome must contain exactly 3 Available entries, got {available_b} \
         (results = {:?})",
        outcome_b.results
    );

    // ── (c) Partial-TimedOut: 1 TimedOut + 3 Available, wall < 5500 ms ──
    let sources_c: Vec<Box<dyn G1Source + Send + Sync>> = vec![
        Box::new(TestG1Source {
            kind: G1ToolKind::TreeSitter,
            behaviour: TestBehaviour::Sleep(Duration::from_millis(50)),
        }),
        Box::new(TestG1Source {
            kind: G1ToolKind::Serena,
            behaviour: TestBehaviour::Sleep(Duration::from_millis(50)),
        }),
        Box::new(TestG1Source {
            kind: G1ToolKind::AstGrep,
            behaviour: TestBehaviour::LongSleep(Duration::from_secs(6)),
        }),
        Box::new(TestG1Source {
            kind: G1ToolKind::Diagnostics,
            behaviour: TestBehaviour::Sleep(Duration::from_millis(50)),
        }),
    ];
    let outcome_c = execute_g1(q.clone(), sources_c, G1_MASTER_DEADLINE).await;
    let timed_out_c = outcome_c
        .results
        .iter()
        .filter(|r| r.status == G1ToolStatus::TimedOut)
        .count();
    let available_c = outcome_c
        .results
        .iter()
        .filter(|r| r.status == G1ToolStatus::Available)
        .count();
    assert_eq!(
        timed_out_c, 1,
        "(c) outcome must contain exactly 1 TimedOut entry, got {timed_out_c} \
         (results = {:?})",
        outcome_c.results
    );
    assert_eq!(
        available_c, 3,
        "(c) outcome must contain exactly 3 Available entries, got {available_c} \
         (results = {:?})",
        outcome_c.results
    );
    assert!(
        outcome_c.wall_elapsed_ms < 5_500,
        "(c) test wall-time must be < 5500 ms \
         (per-source 4.5 s ceiling, master 5 s); got {} ms",
        outcome_c.wall_elapsed_ms
    );
    assert!(
        !outcome_c.master_timed_out,
        "(c) master_timed_out must be false — per-source 4.5 s ceiling \
         fires before master 5 s; got master_timed_out=true"
    );
}

// ── G1 result-fusion acceptance test (P2-W7-F02) ──────────────────────────
//
// Per `DEC-0007` (frozen-selector module-root placement), the
// acceptance test `test_g1_result_fusion` lives at the module root of
// `executor.rs` — NOT inside `mod tests {}` — so the
// `feature-list.json` selector
// `-p ucil-daemon executor::test_g1_result_fusion`
// resolves cleanly without a `tests::` intermediate.
//
// Per `DEC-0008` §4 the four `TestG1Source` impls below are local to
// the test — UCIL's own abstraction boundary, not a mock of any
// external wire format.  Production wiring of real subprocess clients
// (Serena MCP plugin, ast-grep MCP plugin, real
// `ucil_treesitter::parser::Parser`, real
// `crates/ucil-lsp-diagnostics::bridge`) into the fusion path is
// deferred to P2-W7-F05 (`find_references`).

/// Frozen acceptance selector for feature `P2-W7-F02` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon executor::test_g1_result_fusion`.
///
/// Drives [`fuse_g1`] over a real [`execute_g1`] outcome built from
/// four local [`G1Source`] impls and asserts four properties:
///
/// 1. **Location merge** — 4 source entries (3 at `(util.rs, 10, 20)`,
///    1 at `(util.rs, 30, 35)`) collapse into 2 fused entries.
/// 2. **Field union** — disjoint fields from `TreeSitter`, `Serena`,
///    and `AstGrep` at the same location are unioned into one map;
///    the `contributing_sources` list is authority-ordered
///    `[Serena, TreeSitter, AstGrep]`.
/// 3. **Authority resolution** — `Serena` and `AstGrep` both contribute
///    a `signature` field with non-equal values; `Serena` (rank 0)
///    wins and a [`G1Conflict`] row is recorded for `AstGrep` (rank
///    2).  The `ast_kind` field has only `TreeSitter` as a contributor
///    so NO conflict is recorded for it.
/// 4. **Disposition pass-through** — every source's status is
///    forwarded on `source_dispositions`; `master_timed_out` is
///    forwarded too.
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
pub async fn test_g1_result_fusion() {
    /// Local [`G1Source`] impl that returns a pre-baked
    /// `Vec<G1FusionEntry>` JSON-encoded as `payload`.  Per
    /// `DEC-0008` §4 this is a UCIL-internal trait (the
    /// dependency-inversion seam), so a local impl in a test is not a
    /// mock of any external wire format.
    struct TestG1Source {
        kind: G1ToolKind,
        entries: Vec<G1FusionEntry>,
    }

    #[async_trait::async_trait]
    impl G1Source for TestG1Source {
        fn kind(&self) -> G1ToolKind {
            self.kind
        }

        async fn execute(&self, _query: &G1Query) -> G1ToolOutput {
            let payload = serde_json::to_value(&self.entries)
                .expect("test entries must serialize to a JSON value");
            G1ToolOutput {
                kind: self.kind,
                status: G1ToolStatus::Available,
                elapsed_ms: 0,
                payload,
                error: None,
            }
        }
    }

    fn make_entry(file: &str, start: u32, end: u32, fields: serde_json::Value) -> G1FusionEntry {
        let map = match fields {
            serde_json::Value::Object(m) => m,
            other => panic!("make_entry: fields argument must be a JSON object, got {other:?}"),
        };
        G1FusionEntry {
            location: G1FusedLocation {
                file_path: PathBuf::from(file),
                start_line: start,
                end_line: end,
            },
            fields: map,
        }
    }

    let q = G1Query {
        symbol: "foo".to_owned(),
        file_path: PathBuf::from("util.rs"),
        line: 10,
        column: 1,
    };

    // Sources in input order: TreeSitter, Serena, AstGrep, Diagnostics.
    let sources: Vec<Box<dyn G1Source + Send + Sync>> = vec![
        Box::new(TestG1Source {
            kind: G1ToolKind::TreeSitter,
            entries: vec![make_entry(
                "util.rs",
                10,
                20,
                serde_json::json!({ "ast_kind": "function" }),
            )],
        }),
        Box::new(TestG1Source {
            kind: G1ToolKind::Serena,
            entries: vec![make_entry(
                "util.rs",
                10,
                20,
                serde_json::json!({
                    "signature": "fn foo() -> i32",
                    "hover_doc": "Computes foo",
                }),
            )],
        }),
        Box::new(TestG1Source {
            kind: G1ToolKind::AstGrep,
            entries: vec![make_entry(
                "util.rs",
                10,
                20,
                serde_json::json!({
                    "signature": "fn foo()",
                    "pattern": "fn foo($_)",
                }),
            )],
        }),
        Box::new(TestG1Source {
            kind: G1ToolKind::Diagnostics,
            entries: vec![make_entry(
                "util.rs",
                30,
                35,
                serde_json::json!({ "diagnostic": "unused variable" }),
            )],
        }),
    ];

    let raw = execute_g1(q, sources, G1_MASTER_DEADLINE).await;
    let fused = fuse_g1(&raw);

    // ── Sub-assertion 1: location merge (4 sources at 2 locations → 2 entries) ──
    assert_eq!(
        fused.entries.len(),
        2,
        "(1) location merge: expected 2 fused entries (3 contributors at \
         (util.rs, 10, 20) + 1 at (util.rs, 30, 35)); got {}: entries={:?}",
        fused.entries.len(),
        fused.entries
    );

    // ── Sub-assertion 2: field union ──
    let entry_at_10_20 = fused
        .entries
        .iter()
        .find(|e| e.location.start_line == 10 && e.location.end_line == 20)
        .expect("must contain a fused entry at (util.rs, 10, 20)");
    assert_eq!(
        entry_at_10_20.location.file_path,
        PathBuf::from("util.rs"),
        "(2) location.file_path must be \"util.rs\"; got {:?}",
        entry_at_10_20.location.file_path
    );
    let mut keys: Vec<&str> = entry_at_10_20.fields.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["ast_kind", "hover_doc", "pattern", "signature"],
        "(2) field union: expected 4 keys [ast_kind, hover_doc, pattern, signature] \
         at (util.rs, 10, 20); got fields={:?}",
        entry_at_10_20.fields
    );
    assert_eq!(
        entry_at_10_20.contributing_sources,
        vec![
            G1ToolKind::Serena,
            G1ToolKind::TreeSitter,
            G1ToolKind::AstGrep,
        ],
        "(2) contributing_sources must be authority-ordered \
         [Serena, TreeSitter, AstGrep]; got {:?}",
        entry_at_10_20.contributing_sources
    );

    // ── Sub-assertion 3: authority resolution ──
    assert_eq!(
        entry_at_10_20.fields.get("signature"),
        Some(&serde_json::json!("fn foo() -> i32")),
        "(3) authority resolution: signature must be Serena's value \
         \"fn foo() -> i32\"; got {:?}",
        entry_at_10_20.fields.get("signature")
    );
    assert_eq!(
        entry_at_10_20.conflicts.len(),
        1,
        "(3) exactly one G1Conflict expected (only `signature` has \
         multi-source contributors with non-equal values); got {}: {:?}",
        entry_at_10_20.conflicts.len(),
        entry_at_10_20.conflicts
    );
    let conflict = &entry_at_10_20.conflicts[0];
    assert_eq!(
        conflict.field, "signature",
        "(3) conflict.field must be \"signature\"; got {:?}",
        conflict.field
    );
    assert_eq!(
        conflict.winner,
        G1ToolKind::Serena,
        "(3) conflict.winner must be G1ToolKind::Serena; got {:?}",
        conflict.winner
    );
    assert_eq!(
        conflict.winner_value,
        serde_json::json!("fn foo() -> i32"),
        "(3) conflict.winner_value must be \"fn foo() -> i32\"; got {:?}",
        conflict.winner_value
    );
    assert_eq!(
        conflict.losers,
        vec![(G1ToolKind::AstGrep, serde_json::json!("fn foo()"))],
        "(3) conflict.losers must be [(AstGrep, \"fn foo()\")]; got {:?}",
        conflict.losers
    );

    // ── Sub-assertion 4: disposition pass-through ──
    assert_eq!(
        fused.source_dispositions.len(),
        4,
        "(4) source_dispositions must carry all 4 sources; got {}: {:?}",
        fused.source_dispositions.len(),
        fused.source_dispositions
    );
    for (kind, status) in &fused.source_dispositions {
        assert_eq!(
            *status,
            G1ToolStatus::Available,
            "(4) every source must be Available in this happy-path \
             scenario; got status={status:?} for kind={kind:?}",
        );
    }
    let kinds_in_order: Vec<G1ToolKind> =
        fused.source_dispositions.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        kinds_in_order,
        vec![
            G1ToolKind::TreeSitter,
            G1ToolKind::Serena,
            G1ToolKind::AstGrep,
            G1ToolKind::Diagnostics,
        ],
        "(4) source_dispositions order must match the input source vec \
         order [TreeSitter, Serena, AstGrep, Diagnostics]; got {kinds_in_order:?}",
    );
    assert!(
        !fused.master_timed_out,
        "(4) master_timed_out must be false on the happy path"
    );

    // ── Sub-assertion 5: the (util.rs, 30, 35) Diagnostics-only entry ──
    let entry_at_30_35 = fused
        .entries
        .iter()
        .find(|e| e.location.start_line == 30 && e.location.end_line == 35)
        .expect("must contain a fused entry at (util.rs, 30, 35)");
    assert_eq!(
        entry_at_30_35.contributing_sources,
        vec![G1ToolKind::Diagnostics],
        "(5) (util.rs, 30, 35) is Diagnostics-only; got {:?}",
        entry_at_30_35.contributing_sources
    );
    assert_eq!(
        entry_at_30_35.fields.get("diagnostic"),
        Some(&serde_json::json!("unused variable")),
        "(5) Diagnostics-only entry must carry the `diagnostic` field \
         verbatim; got {:?}",
        entry_at_30_35.fields.get("diagnostic")
    );
    assert!(
        entry_at_30_35.conflicts.is_empty(),
        "(5) Diagnostics-only entry has no multi-source field, so \
         conflicts must be empty; got {:?}",
        entry_at_30_35.conflicts
    );
}

// ── WO-0064 / P2-W8-F04 frozen acceptance tests ────────────────────────────
//
// `executor::test_lancedb_incremental_indexing` and
// `executor::test_lancedb_indexer_handle_processes_events` live at MODULE
// ROOT (NOT inside `mod tests {}`) per `DEC-0007` (frozen-selector
// module-root placement).  The selectors resolve to
// `ucil_daemon::executor::test_lancedb_incremental_indexing` and
// `ucil_daemon::executor::test_lancedb_indexer_handle_processes_events`
// respectively, which only matches when the fns are at column 0 in this
// file.

/// Synthetic-tokenizer JSON used by the WO-0064 acceptance tests.
///
/// Single-vocab `WordLevel` + `WhitespaceSplit` pre-tokenizer; every
/// word maps to the `<unk>` id, so the encoded id stream's length
/// equals the number of whitespace-separated words.  Built via the
/// real `tokenizers` crate API per `DEC-0008` carve-out — distinct
/// from the prohibited critical-dep substitution layers.
///
/// **Visibility**: `pub(crate)` per `WO-0066`'s second-consumer
/// carve-out — `crate::server::test_find_similar_tool` reuses the
/// same fixture-builder pattern as
/// [`test_lancedb_incremental_indexing`] and needs to construct an
/// in-process synthetic chunker without re-defining the JSON literal.
#[cfg(test)]
pub(crate) const SYNTHETIC_TOKENIZER_JSON_FOR_LANCEDB_F04: &str = r#"{
    "version": "1.0",
    "truncation": null,
    "padding": null,
    "added_tokens": [],
    "normalizer": null,
    "pre_tokenizer": {"type": "WhitespaceSplit"},
    "post_processor": null,
    "decoder": null,
    "model": {
        "type": "WordLevel",
        "vocab": {"<unk>": 0},
        "unk_token": "<unk>"
    }
}"#;

/// Deterministic [`crate::lancedb_indexer::EmbeddingSource`] used by
/// the WO-0064 acceptance tests.
///
/// Per `DEC-0008` §4 carve-out, this is a `UCIL`-internal trait
/// impl — distinct from the prohibited critical-dep substitution
/// layers for `Serena` / `LSP` / `SQLite` / `LanceDB` / `Docker`.
/// Returns a `Vec<f32>` of length `dim` derived from a `Sha256`
/// hash of the input — deterministic, non-trivial, and depends on
/// input so different chunks produce different vectors.
///
/// **Visibility**: `pub(crate)` per `WO-0066`'s second-consumer
/// carve-out — `crate::server::test_find_similar_tool` reuses the
/// same fixture-builder pattern as
/// [`test_lancedb_incremental_indexing`].  Per `WO-0048` line 363
/// the `Test*` trait-impl naming is exempt from the production-code
/// `mock|fake|stub` word-ban.
#[cfg(test)]
pub(crate) struct TestEmbeddingSource {
    pub(crate) dim: usize,
}

#[cfg(test)]
#[async_trait::async_trait]
impl crate::lancedb_indexer::EmbeddingSource for TestEmbeddingSource {
    fn name(&self) -> &'static str {
        "test"
    }

    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed(
        &self,
        code: &str,
    ) -> Result<Vec<f32>, crate::lancedb_indexer::EmbeddingSourceError> {
        use sha2::Digest as _;
        let h = sha2::Sha256::digest(code.as_bytes());
        let v = (0..self.dim)
            .map(|i| f32::from(h[i % 32]) / 255.0)
            .collect();
        Ok(v)
    }
}

/// Build an [`ucil_embeddings::EmbeddingChunker`] from the
/// in-process synthetic tokenizer (no on-disk model artefact
/// required).  The resulting chunker is owned by the caller; tests
/// wrap it in `Arc<tokio::sync::Mutex<_>>` for the indexer.
///
/// **Visibility**: `pub(crate)` per `WO-0066`'s second-consumer
/// carve-out — `crate::server::test_find_similar_tool` reuses this
/// helper to seed the per-branch `code_chunks` table for the
/// `find_similar` MCP tool's frozen acceptance test.
#[cfg(test)]
pub(crate) fn build_synthetic_chunker_for_lancedb_f04() -> ucil_embeddings::EmbeddingChunker {
    let tokenizer: tokenizers::Tokenizer = SYNTHETIC_TOKENIZER_JSON_FOR_LANCEDB_F04
        .parse()
        .expect("synthetic tokenizer parses");
    ucil_embeddings::EmbeddingChunker::from_tokenizer(tokenizer)
}

/// Helper — open the per-branch `code_chunks` `LanceDB` table and
/// return a vector of `(file_path, file_hash)` rows, plus the row
/// count.  Used by `SA2`, `SA4`, and the handle test to inspect
/// the post-pass state of the table without depending on the
/// in-memory `IndexerStats`.
///
/// **Visibility**: `pub(crate)` per `WO-0066`'s second-consumer
/// carve-out — `crate::server::test_find_similar_tool` reuses this
/// helper to verify the LanceDB table is populated post-`index_paths`.
#[cfg(test)]
pub(crate) async fn read_table_rows_for_lancedb_f04(
    branches_root: &Path,
    branch_sanitised: &str,
) -> (Vec<(String, String)>, usize) {
    use arrow_array::cast::AsArray;
    use futures::TryStreamExt as _;
    use lancedb::query::{ExecutableQuery as _, QueryBase as _};
    let vectors_dir = branches_root.join(branch_sanitised).join("vectors");
    let conn = lancedb::connect(vectors_dir.to_str().expect("utf8 vectors"))
        .execute()
        .await
        .expect("lancedb connect");
    let table = conn
        .open_table("code_chunks")
        .execute()
        .await
        .expect("open code_chunks");
    let count = table.count_rows(None).await.expect("count_rows");
    let stream = table.query().limit(100_000).execute().await.expect("query");
    let batches: Vec<_> = stream.try_collect().await.expect("collect");
    let mut rows: Vec<(String, String)> = Vec::new();
    for batch in batches {
        let file_paths = batch
            .column_by_name("file_path")
            .expect("file_path col")
            .as_string::<i32>();
        let file_hashes = batch
            .column_by_name("file_hash")
            .expect("file_hash col")
            .as_string::<i32>();
        for i in 0..batch.num_rows() {
            rows.push((
                file_paths.value(i).to_owned(),
                file_hashes.value(i).to_owned(),
            ));
        }
    }
    (rows, count)
}

/// Frozen acceptance test for `P2-W8-F04` per `WO-0064`.
///
/// Six sub-assertions (SA1-SA6) per the WO scope:
/// 1. first pass indexes all paths;
/// 2. second pass over unchanged files skips them;
/// 3. touching one file reindexes only the touched file;
/// 4. `file_hash` differs across the touched file's pre/post rows;
/// 5. persisted `indexer-state.json` round-trips through
///    `IndexerState::load_or_default`;
/// 6. `code_chunks.embedding` schema is
///    `FixedSizeList<Float32, 768>`.
///
/// # Panics
///
/// Panics on any sub-assertion failure (test-only).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)]
pub async fn test_lancedb_incremental_indexing() {
    use std::sync::Arc;

    use crate::branch_manager::BranchManager;
    use crate::lancedb_indexer::{IndexerState, LancedbChunkIndexer};

    let repo = tempfile::tempdir().expect("tmp repo");
    let branches_root = repo.path().join(".ucil/branches");
    tokio::fs::create_dir_all(&branches_root)
        .await
        .expect("mkdir branches");

    // BranchManager + create_branch_table("main", None) creates the
    // `code_chunks` table this test exercises.
    let mgr = Arc::new(BranchManager::new(&branches_root));
    mgr.create_branch_table("main", None)
        .await
        .expect("create main");

    // Synthetic chunker via the WO-0060-established pattern (no
    // on-disk tokenizer.json artefact required).
    let chunker = Arc::new(tokio::sync::Mutex::new(
        build_synthetic_chunker_for_lancedb_f04(),
    ));

    // TestEmbeddingSource — deterministic Sha256-derived 768-dim
    // vectors per chunk content.
    let source = Arc::new(TestEmbeddingSource { dim: 768 });

    // Source files — multi-statement bodies so the chunker emits ≥1
    // chunk per file under the WordLevel synthetic tokenizer.
    let foo_path = repo.path().join("src/foo.rs");
    let bar_path = repo.path().join("src/bar.rs");
    tokio::fs::create_dir_all(foo_path.parent().expect("foo parent"))
        .await
        .expect("mkdir src");
    let foo_original = "pub fn foo() { let x = 1; let y = 2; let z = x + y; }";
    let bar_original = "pub fn bar() -> i32 { 42 }";
    tokio::fs::write(&foo_path, foo_original)
        .await
        .expect("write foo.rs");
    tokio::fs::write(&bar_path, bar_original)
        .await
        .expect("write bar.rs");

    let mut indexer =
        LancedbChunkIndexer::new(mgr.clone(), "main", chunker.clone(), source.clone());

    // ── SA1 — first pass indexes all ──────────────────────────────
    let stats1 = indexer
        .index_paths(repo.path(), &[foo_path.clone(), bar_path.clone()])
        .await
        .expect("first pass");
    assert_eq!(
        stats1.files_scanned, 2,
        "SA1: files_scanned=2; got stats={stats1:?}"
    );
    assert_eq!(
        stats1.files_skipped_unchanged, 0,
        "SA1: files_skipped_unchanged=0; got stats={stats1:?}"
    );
    assert_eq!(
        stats1.files_indexed, 2,
        "SA1: files_indexed=2; got stats={stats1:?}"
    );
    assert!(
        stats1.chunks_inserted >= 2,
        "SA1: chunks_inserted >= 2; got stats={stats1:?}"
    );
    assert_eq!(
        stats1.chunks_failed, 0,
        "SA1: chunks_failed=0; got stats={stats1:?}"
    );

    // Capture post-pass-1 row count for SA2's no-duplicate-inserts
    // assertion.
    let (_rows1, count1) = read_table_rows_for_lancedb_f04(&branches_root, "main").await;

    // ── SA2 — second pass over unchanged files skips them ─────────
    let stats2 = indexer
        .index_paths(repo.path(), &[foo_path.clone(), bar_path.clone()])
        .await
        .expect("second pass");
    assert_eq!(
        stats2.files_scanned, 2,
        "SA2: files_scanned=2; got stats={stats2:?}"
    );
    assert_eq!(
        stats2.files_skipped_unchanged, 2,
        "SA2: files_skipped_unchanged=2; got stats={stats2:?}"
    );
    assert_eq!(
        stats2.files_indexed, 0,
        "SA2: files_indexed=0; got stats={stats2:?}"
    );
    assert_eq!(
        stats2.chunks_inserted, 0,
        "SA2: chunks_inserted=0; got stats={stats2:?}"
    );
    let (_rows2, count2) = read_table_rows_for_lancedb_f04(&branches_root, "main").await;
    assert_eq!(
        count1, count2,
        "SA2: row count must not grow across no-op pass; got {count1}, {count2}"
    );

    // ── SA3 — touch one file then re-index ────────────────────────
    // Write NEW contents to bump mtime (and produce a different
    // file_hash for SA4).  Sleep ≥1 sec so seconds-resolution mtime
    // actually changes (Linux mtime resolution can be <1s but write
    // semantics may collapse same-second writes — the seconds
    // granularity in IndexerState requires a >=1s gap).
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let foo_modified = format!("{foo_original}\nfn extra() {{}}\n");
    tokio::fs::write(&foo_path, &foo_modified)
        .await
        .expect("touch foo.rs");

    let stats3 = indexer
        .index_paths(repo.path(), &[foo_path.clone(), bar_path.clone()])
        .await
        .expect("third pass");
    assert_eq!(
        stats3.files_scanned, 2,
        "SA3: files_scanned=2; got stats={stats3:?}"
    );
    assert_eq!(
        stats3.files_skipped_unchanged, 1,
        "SA3: files_skipped_unchanged=1 (bar.rs); got stats={stats3:?}"
    );
    assert_eq!(
        stats3.files_indexed, 1,
        "SA3: files_indexed=1 (foo.rs); got stats={stats3:?}"
    );
    assert!(
        stats3.chunks_inserted >= 1,
        "SA3: chunks_inserted >= 1; got stats={stats3:?}"
    );

    // ── SA4 — file_hash differs across foo.rs's pre/post rows ─────
    let (rows3, _count3) = read_table_rows_for_lancedb_f04(&branches_root, "main").await;
    let foo_hashes: std::collections::BTreeSet<String> = rows3
        .iter()
        .filter(|(p, _)| p == "src/foo.rs")
        .map(|(_, h)| h.clone())
        .collect();
    let bar_hashes: std::collections::BTreeSet<String> = rows3
        .iter()
        .filter(|(p, _)| p == "src/bar.rs")
        .map(|(_, h)| h.clone())
        .collect();
    assert!(
        foo_hashes.len() > 1,
        "SA4: src/foo.rs must contain >1 distinct file_hash (pre+post touch); \
         got {foo_hashes:?} from rows {rows3:?}"
    );
    assert_eq!(
        bar_hashes.len(),
        1,
        "SA4: src/bar.rs must contain exactly 1 distinct file_hash (untouched); \
         got {bar_hashes:?} from rows {rows3:?}"
    );

    // ── SA5 — persisted state JSON round-trips ────────────────────
    let state_path = indexer.state_path();
    let raw = tokio::fs::read_to_string(&state_path)
        .await
        .expect("indexer-state.json must exist post-pass-1");
    let parsed: IndexerState = serde_json::from_str(&raw).expect("indexer-state.json parses");
    assert!(
        parsed
            .file_mtimes
            .contains_key(&PathBuf::from("src/foo.rs")),
        "SA5: parsed state must contain src/foo.rs entry; got {parsed:?}"
    );
    assert!(
        parsed
            .file_mtimes
            .contains_key(&PathBuf::from("src/bar.rs")),
        "SA5: parsed state must contain src/bar.rs entry; got {parsed:?}"
    );
    assert_eq!(
        parsed.schema_version,
        IndexerState::schema_version_current(),
        "SA5: schema_version must match current; got {parsed:?}"
    );

    // ── SA6 — embedding column schema is FixedSizeList<Float32, 768> ─
    let vectors_dir = branches_root.join("main/vectors");
    let conn = lancedb::connect(vectors_dir.to_str().expect("utf8"))
        .execute()
        .await
        .expect("lancedb connect for SA6");
    let table = conn
        .open_table("code_chunks")
        .execute()
        .await
        .expect("open table for SA6");
    let schema = table.schema().await.expect("schema");
    let embedding_field = schema
        .field_with_name("embedding")
        .expect("embedding column present");
    match embedding_field.data_type() {
        arrow_schema::DataType::FixedSizeList(inner, 768) => {
            assert_eq!(
                inner.data_type(),
                &arrow_schema::DataType::Float32,
                "SA6: embedding inner type Float32; got {:?}",
                inner.data_type()
            );
        }
        other => panic!("SA6: embedding column must be FixedSizeList<Float32, 768>; got {other:?}"),
    }
}

/// Companion frozen test for `P2-W8-F04`.
///
/// Proves the
/// [`crate::lancedb_indexer::IndexerHandle::spawn`] watcher → indexer
/// dispatch wiring actually invokes
/// [`crate::lancedb_indexer::LancedbChunkIndexer::index_paths`]
/// (not just compiles).
///
/// # Panics
///
/// Panics if the spawned handle does not produce ≥1 row in the
/// `code_chunks` table after a `Created` event is sent (test-only).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
pub async fn test_lancedb_indexer_handle_processes_events() {
    use std::sync::Arc;

    use crate::branch_manager::BranchManager;
    use crate::lancedb_indexer::{IndexerHandle, LancedbChunkIndexer};
    use crate::watcher::{EventSource, FileEvent, FileEventKind};

    let repo = tempfile::tempdir().expect("tmp repo");
    let branches_root = repo.path().join(".ucil/branches");
    tokio::fs::create_dir_all(&branches_root)
        .await
        .expect("mkdir branches");

    let mgr = Arc::new(BranchManager::new(&branches_root));
    mgr.create_branch_table("main", None)
        .await
        .expect("create main");

    let chunker = Arc::new(tokio::sync::Mutex::new(
        build_synthetic_chunker_for_lancedb_f04(),
    ));
    let source = Arc::new(TestEmbeddingSource { dim: 768 });

    // Single source file the spawned handle will process.
    let foo_path = repo.path().join("src/foo.rs");
    tokio::fs::create_dir_all(foo_path.parent().expect("foo parent"))
        .await
        .expect("mkdir src");
    tokio::fs::write(
        &foo_path,
        "pub fn foo() { let x = 1; let y = 2; let z = x + y; }",
    )
    .await
    .expect("write foo.rs");

    let indexer = Arc::new(tokio::sync::Mutex::new(LancedbChunkIndexer::new(
        mgr.clone(),
        "main",
        chunker.clone(),
        source.clone(),
    )));

    let (tx, rx) = tokio::sync::mpsc::channel::<FileEvent>(8);
    let handle = IndexerHandle::spawn(indexer.clone(), repo.path().to_owned(), rx);

    tx.send(FileEvent {
        path: foo_path.clone(),
        kind: FileEventKind::Created,
        source: EventSource::PostToolUseHook,
    })
    .await
    .expect("send create event");

    drop(tx);
    handle.shutdown().await.expect("join handle");

    let (_rows, count) = read_table_rows_for_lancedb_f04(&branches_root, "main").await;
    assert!(
        count >= 1,
        "indexer-handle must dispatch the Created event \
         to index_paths and produce ≥1 row; got {count}"
    );
}
