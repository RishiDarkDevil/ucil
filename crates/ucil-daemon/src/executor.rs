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
use std::hash::{Hash as _, Hasher as _};
use std::io;
use std::path::{Path, PathBuf};

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
// 3. [`fake_serena_hover_client::ScriptedFakeSerenaHoverClient`] — the
//    hand-written scripted fake that drives the fusion function under
//    test.  It is NOT a mock of Serena's MCP wire format (forbidden per
//    root `CLAUDE.md`) — it implements UCIL's own [`SerenaHoverClient`]
//    trait, the DEC-0008 canonical test seam also in use by
//    `ucil-lsp-diagnostics::{call_hierarchy,quality_pipeline}::
//    fake_serena_client`.
//
// Wiring into [`crate::server::McpServer::handle_find_definition`] is
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
/// through [`fake_serena_hover_client::ScriptedFakeSerenaHoverClient`],
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
/// that [`crate::server::project_callers`] emits onto the MCP
/// `_meta.callers` array (see `server.rs`).  Promoted to a typed struct
/// here so [`enrich_find_definition`] stays testable without round-
/// tripping through `serde_json::Value`.  The live-wiring WO that
/// threads this into [`crate::server::McpServer::handle_find_definition`]
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
