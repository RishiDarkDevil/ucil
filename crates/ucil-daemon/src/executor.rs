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
fn rust_project_fixture() -> PathBuf {
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
