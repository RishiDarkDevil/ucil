//! Handler + supporting types for the `understand_code` MCP tool
//! (`P1-W4-F09`, master-plan §3.2 row 1 / §18 Phase 1 Week 4 line 1751).
//!
//! `understand_code` is the first MCP tool whose response fuses two
//! structural data sources: tree-sitter AST symbols (the G1 group per
//! master-plan §3.2 row 1) and knowledge-graph entity/relation metadata
//! (master-plan §12.1 + §12.2).  The dispatch entry point
//! [`handle_understand_code`] lives in
//! [`crate::server::McpServer::handle_tools_call`]; the helpers
//! ([`explain_file`], [`explain_symbol`], [`count_imports`]) and their
//! data types (`UnderstandCodeFileSummary`, `UnderstandCodeSymbolSummary`,
//! `UnderstandCodeError`, …) live in this module so `server.rs` stays
//! readable.
//!
//! Data lineage on the wire: `_meta.source = "tree-sitter+kg"` so
//! downstream fusion layers know both authoritative structural sources
//! have been consulted.  Semantic enrichment (LanceDB embeddings,
//! Serena/LSP hover) is explicitly out of scope — see the WO-0036
//! `scope_out` list.
//!
//! References:
//! * master-plan §3.2 row 1 — `understand_code` — explain what a
//!   file/function/module does, why it exists, its context; groups
//!   G1, G3, G5.
//! * master-plan §12.1 — `entities` schema consumed via
//!   [`ucil_core::KnowledgeGraph::list_entities_by_file`] +
//!   [`ucil_core::KnowledgeGraph::get_entity_by_qualified_name`] +
//!   [`ucil_core::KnowledgeGraph::get_entity_by_id`].
//! * master-plan §12.2 — `relations` schema consumed via
//!   [`ucil_core::KnowledgeGraph::list_relations_by_source`] +
//!   [`ucil_core::KnowledgeGraph::list_relations_by_target`].
//! * feature `P1-W4-F09` — frozen acceptance selector
//!   `-p ucil-daemon server::test_understand_code_tool`.
//! * DEC-0007 — mutation verification is the stash-based reality
//!   check; this module's handler + dispatch wiring must fail the
//!   acceptance test when stashed.

// Public API items share a name prefix with the module — mirrors the
// escape used in `server.rs` / `parser.rs` / `symbols.rs`.
#![allow(clippy::module_name_repetitions)]

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::Serialize;
use serde_json::{json, Value};
use thiserror::Error;
use tree_sitter::{Query, QueryCursor};
use ucil_core::{Entity, KnowledgeGraph, KnowledgeGraphError};
use ucil_treesitter::{Language, ParseError, Parser, SymbolExtractor};

use crate::server::{jsonrpc_error, JSONRPC_VERSION};

/// Upper bound on the number of inbound / outbound edges serialised in
/// a single `understand_code` symbol-mode response.
///
/// The `relations` table is append-only and unbounded, so a symbol at a
/// hub position (e.g. a logging macro referenced from every crate) can
/// easily produce tens of thousands of edges.  Cap at 50 per direction
/// so the MCP response envelope stays well under a host's token budget;
/// callers that need the full adjacency list can query
/// `list_relations_by_source` / `list_relations_by_target` directly via
/// a future G2 tool.  Clamps are logged at `tracing::warn` so agents see
/// the truncation in their own logs.
pub const UNDERSTAND_CODE_MAX_EDGES: usize = 50;

/// Upper bound on the number of top-level symbols serialised in a
/// single `understand_code` file-mode response.
///
/// A generated-code file (e.g. a 10k-line protobuf rust binding)
/// can produce thousands of symbols.  Cap the top-level roster at 100
/// so the MCP response envelope stays within a reasonable token
/// budget; clamps are logged at `tracing::warn` so agents see the
/// truncation in their own logs.
pub const UNDERSTAND_CODE_MAX_TOP_LEVEL_SYMBOLS: usize = 100;

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by the `understand_code` dispatch pipeline.
///
/// Every variant carries a user-safe message.  Absolute paths under the
/// caller-supplied `root` are stripped to relative via
/// `strip_prefix(root).unwrap_or(path)` before embedding so the MCP
/// response never leaks paths outside the project the caller pointed at.
///
/// See `handle_understand_code` in `server.rs` for the mapping from
/// each variant to its JSON-RPC error code.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum UnderstandCodeError {
    /// `std::fs::read_to_string` failed on the target file.
    #[error("understand_code: file read failed: {0}")]
    Io(#[from] std::io::Error),

    /// The tree-sitter parser returned a `ParseError` (grammar load
    /// failure or timeout).  Ordinary syntax errors are surfaced as
    /// error nodes inside the tree, not as this variant.
    #[error("understand_code: tree-sitter parse failed: {0}")]
    Parse(#[from] ParseError),

    /// A knowledge-graph read (`list_entities_by_file`,
    /// `get_entity_by_qualified_name`, `list_relations_by_source`,
    /// `list_relations_by_target`, `get_entity_by_id`) failed.
    #[error("understand_code: knowledge graph error: {0}")]
    KnowledgeGraph(#[from] KnowledgeGraphError),

    /// The target file's extension does not map to any tree-sitter
    /// grammar bundled with this build of UCIL.  Carries the
    /// project-relative path of the offending file so the caller
    /// knows which file was skipped.
    #[error("understand_code: unsupported language for file: {0}")]
    UnsupportedLanguage(String),

    /// The resolved target path escaped the caller-supplied `root`
    /// after canonicalisation (e.g. a `..` traversal).  Carries the
    /// project-relative representation to avoid leaking the absolute
    /// path.
    #[error("understand_code: target path escapes root: {0}")]
    TargetNotInRoot(String),

    /// The symbol-mode resolver returned `Ok(None)` —
    /// `get_entity_by_qualified_name` did not find a matching row.
    /// The outer handler converts this into a well-formed
    /// `_meta.found == false` envelope rather than a JSON-RPC error.
    #[error("understand_code: symbol not found: {0}")]
    NotFound(String),

    /// The knowledge graph mutex was poisoned — a prior handler panicked
    /// while holding the lock.
    #[error("understand_code: internal error (knowledge graph mutex poisoned)")]
    Poisoned,
}

// ── Structured payload shapes ────────────────────────────────────────────────

/// One top-level symbol row inside an
/// [`UnderstandCodeFileSummary::top_level_symbols`] list.
///
/// Every row pairs the tree-sitter-extracted symbol (`kind`, `name`,
/// `start_line`) with the KG-side `qualified_name` whenever a matching
/// entity row exists in the knowledge graph; rows produced purely by
/// tree-sitter (i.e. a file the KG has never ingested) carry
/// `qualified_name == None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileSymbolRow {
    /// Kind of the symbol (`"function"`, `"method"`, `"class"`,
    /// `"struct"`, `"enum"`, `"trait"`, `"interface"`, `"type_alias"`,
    /// `"constant"`, `"module"`).  Mirrors `ucil_treesitter::SymbolKind`'s
    /// snake-case tagging so consumers can deserialise without a
    /// `ucil_treesitter` dependency.
    pub kind: String,
    /// Unqualified symbol name as written in source (e.g. `"parse_file"`,
    /// `"MyStruct"`).
    pub name: String,
    /// Fully qualified name from the KG entity row whose `(name,
    /// start_line)` matched this symbol, or `None` when no KG row
    /// corresponds.
    pub qualified_name: Option<String>,
    /// 1-based line number of the symbol's first character.
    pub start_line: u32,
}

/// Structured file-mode response payload emitted by
/// [`handle_understand_code`] when the target resolves to a regular
/// file under `arguments.root`.
///
/// Fields follow master-plan §3.2 row 1 ("explain what a file does")
/// and the WO-0036 `scope_in` bullet 2 contract verbatim.  Serialised
/// at `_meta.summary` in the MCP response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnderstandCodeFileSummary {
    /// The tree-sitter language tag the file was parsed as — `"rust"`,
    /// `"python"`, `"typescript"`, etc.  Matches
    /// `ucil_treesitter::symbols::language_serde`'s lowercase names.
    pub language: &'static str,
    /// Path of the target file, project-relative when the caller
    /// supplied a `root` that prefixes it; otherwise absolute.
    pub file_path: PathBuf,
    /// Total line count reported by `source.lines().count()` — the
    /// master-plan §3.2 row 1 "shape" hint.
    pub line_count: u64,
    /// Number of language-appropriate import declarations in the file,
    /// counted by [`count_imports`].  Indicative of the file's
    /// coupling surface.
    pub import_count: usize,
    /// Top-level symbol roster, capped at
    /// [`UNDERSTAND_CODE_MAX_TOP_LEVEL_SYMBOLS`].
    pub top_level_symbols: Vec<FileSymbolRow>,
    /// Number of [`ucil_core::Entity`] rows whose `file_path` matches
    /// the target file.  Zero for a file the KG has never ingested.
    pub kg_entity_count: usize,
}

/// Projection of [`ucil_core::Entity`] fields safe to serialise into
/// the `understand_code` response.
///
/// Keeps the outgoing JSON focused on the fields MCP hosts render —
/// qualified name, location, signature, doc comment — and drops the
/// bi-temporal / provenance columns (`t_valid_from`, `t_valid_to`,
/// `importance`, `source_tool`, `source_hash`) that callers can fetch
/// directly through future G2 tools.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntitySummary {
    /// Fully qualified name of the entity, or `None` when the `kind`
    /// does not carry one (e.g. `"file"`).
    pub qualified_name: Option<String>,
    /// Unqualified entity name.
    pub name: String,
    /// `entities.kind` — `"function"`, `"method"`, `"struct"`, etc.
    pub entity_type: String,
    /// Source-file path stored on the entity row.
    pub file_path: String,
    /// 1-based inclusive start line.
    pub start_line: Option<i64>,
    /// 1-based inclusive end line.
    pub end_line: Option<i64>,
    /// Best-effort signature captured at ingest time.
    pub signature: Option<String>,
    /// Attached doc comment captured at ingest time.
    pub doc_comment: Option<String>,
}

impl EntitySummary {
    /// Project an [`Entity`] onto its public [`EntitySummary`] shape.
    #[must_use]
    pub fn from_entity(e: &Entity) -> Self {
        Self {
            qualified_name: e.qualified_name.clone(),
            name: e.name.clone(),
            entity_type: e.kind.clone(),
            file_path: e.file_path.clone(),
            start_line: e.start_line,
            end_line: e.end_line,
            signature: e.signature.clone(),
            doc_comment: e.doc_comment.clone(),
        }
    }
}

/// One inbound or outbound relation edge projected onto a wire-friendly
/// shape.
///
/// The `peer_*` fields are resolved by
/// [`ucil_core::KnowledgeGraph::get_entity_by_id`] against the relation's
/// opposite vertex (`target_id` for an outbound edge, `source_id` for an
/// inbound edge).  Dangling foreign keys (peer entity deleted between
/// queries) are logged at `tracing::debug` and dropped — see
/// [`explain_symbol`] for the filtering step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelationEdge {
    /// Kind of relation (`"calls"`, `"imports"`, `"implements"`, ...).
    pub relation_type: String,
    /// Qualified name of the peer entity.  Falls back to the peer's
    /// unqualified `name` when the entity row has no
    /// `qualified_name`.
    pub peer_qualified_name: String,
    /// File path of the peer entity — useful for letting the MCP host
    /// jump directly to the caller / callee.
    pub peer_file_path: Option<String>,
    /// 1-based start line of the peer entity — paired with
    /// `peer_file_path` for location resolution.
    pub peer_start_line: Option<u32>,
}

/// Structured symbol-mode response payload emitted by
/// [`handle_understand_code`] when `arguments.target` resolves to a
/// fully qualified symbol name.
///
/// `containing_file` is populated opportunistically: when the
/// resolved entity's `file_path` is a readable file under
/// `arguments.root`, [`explain_file`] runs against that path and the
/// summary is attached.  A missing file (e.g. stale KG row) is logged
/// at `tracing::debug` and `containing_file` is `None` — never an
/// error.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnderstandCodeSymbolSummary {
    /// Projection of the resolved entity's metadata columns.
    pub entity: EntitySummary,
    /// Count of inbound edges (pre-cap) targeting the entity.
    pub inbound_relation_count: usize,
    /// Count of outbound edges (pre-cap) originating from the entity.
    pub outbound_relation_count: usize,
    /// Inbound edges, capped at [`UNDERSTAND_CODE_MAX_EDGES`], sorted
    /// by `(relation_type, peer_qualified_name)` for deterministic
    /// output.
    pub inbound_edges: Vec<RelationEdge>,
    /// Outbound edges, capped at [`UNDERSTAND_CODE_MAX_EDGES`], sorted
    /// by `(relation_type, peer_qualified_name)` for deterministic
    /// output.
    pub outbound_edges: Vec<RelationEdge>,
    /// File summary of the entity's `file_path`, if the file is
    /// readable under `arguments.root`.
    pub containing_file: Option<UnderstandCodeFileSummary>,
}

// ── count_imports ────────────────────────────────────────────────────────────

/// Count the number of language-appropriate import declarations in
/// `source`.
///
/// Uses tree-sitter AST queries (not string matching) so that
/// comments, string literals, and nested macros cannot inflate the
/// count.  Each language maps to a set of top-level node kinds:
///
/// * **Rust** — `use_declaration`, `extern_crate_declaration`.
/// * **Python** — `import_statement`, `import_from_statement`.
/// * **TypeScript / JavaScript** — `import_statement`,
///   `call_expression` with `require` callee.
/// * **Go** — `import_declaration` (one per declaration; a single
///   `import ( ... )` block counts as one declaration).
/// * **Other** — returns `0` (no heuristic for languages without a
///   clean import construct, e.g. Bash, JSON).
///
/// Failures to compile a query (should be impossible with pinned
/// grammar crates) are logged at `tracing::warn` and return `0` for
/// that kind — the overall function cannot fail.
///
/// References:
/// * WO-0036 `scope_in` bullet 5 (file-mode helper contract).
/// * feature `P1-W4-F09` (acceptance test asserts `import_count == 2`
///   on a two-`use` Rust fixture).
#[must_use]
pub fn count_imports(source: &str, lang: Language) -> usize {
    let mut parser = Parser::new();
    let tree = match parser.parse(source, lang) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("count_imports: parse failed: {e}");
            return 0;
        }
    };
    let ts_lang = lang.ts_language();
    let kinds: &[&str] = match lang {
        Language::Rust => &["use_declaration", "extern_crate_declaration"],
        Language::Python => &["import_statement", "import_from_statement"],
        Language::TypeScript | Language::JavaScript => &["import_statement"],
        Language::Go => &["import_declaration"],
        _ => return 0,
    };

    let mut total = 0usize;
    for kind in kinds {
        let q_str = format!("({kind}) @node");
        let Ok(query) = Query::new(&ts_lang, &q_str) else {
            tracing::warn!(query = %q_str, "count_imports: query compile failed");
            continue;
        };
        let mut cursor = QueryCursor::new();
        // `QueryMatches` implements `StreamingIterator`, not `Iterator`;
        // we only need the count so a plain increment in a `while let`
        // loop is sufficient.
        use streaming_iterator::StreamingIterator as _;
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while matches.next().is_some() {
            total += 1;
        }
    }

    // TypeScript / JavaScript additionally treat `require(...)` calls
    // as imports.  tree-sitter does not expose them as a distinct node
    // kind, so we match the `call_expression` shape and check the
    // callee text.
    if matches!(lang, Language::TypeScript | Language::JavaScript) {
        total += count_require_calls(&tree, source, &ts_lang);
    }

    total
}

/// Count `require("…")` calls in a TS/JS parse tree — a helper for
/// [`count_imports`] kept out-of-line so the main function stays
/// readable.
fn count_require_calls(
    tree: &tree_sitter::Tree,
    source: &str,
    ts_lang: &tree_sitter::Language,
) -> usize {
    let q_str = "(call_expression function: (identifier) @fn) @call";
    let Ok(query) = Query::new(ts_lang, q_str) else {
        return 0;
    };
    let fn_idx = match query.capture_index_for_name("fn") {
        Some(i) => i,
        None => return 0,
    };
    let mut cursor = QueryCursor::new();
    use streaming_iterator::StreamingIterator as _;
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut n = 0usize;
    while let Some(m) = matches.next() {
        let fn_node = m
            .captures
            .iter()
            .find(|c| c.index == fn_idx)
            .map(|c| c.node);
        if let Some(node) = fn_node {
            let start = node.start_byte().min(source.len());
            let end = node.end_byte().min(source.len());
            if &source[start..end] == "require" {
                n += 1;
            }
        }
    }
    n
}

// ── explain_file ─────────────────────────────────────────────────────────────

/// Language-tag string — one of the lowercase names the
/// `ucil_treesitter::symbols::language_serde` module uses — for the
/// `_meta.summary.language` wire field.
///
/// [`Language`] is `#[non_exhaustive]` so the compiler forces a
/// wildcard arm; new variants added upstream before this module is
/// taught about them fall through to `"unknown"` — the response still
/// serialises and agents can see the gap in their own logs.
#[must_use]
pub fn language_tag(lang: Language) -> &'static str {
    match lang {
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

/// Build the file-mode [`UnderstandCodeFileSummary`] for `target_path`.
///
/// Runs:
/// 1. `std::fs::read_to_string(target_path)` — surfaced as `Io`.
/// 2. [`Language::from_extension`] on the file's final component's
///    extension — `UnsupportedLanguage` for an extension the build
///    does not ship a grammar for.
/// 3. [`ucil_treesitter::Parser::new`] + [`Parser::parse`] — surfaced
///    as `Parse`.
/// 4. [`ucil_treesitter::SymbolExtractor::extract`] — never fails.
/// 5. [`count_imports`] — pure function, never fails.
/// 6. [`KnowledgeGraph::list_entities_by_file`] on the canonicalised
///    target path — surfaced as `KnowledgeGraph`.
/// 7. Zip of tree-sitter symbols with KG entities on `(name,
///    start_line)`, producing per-row `qualified_name` when a KG
///    entity corresponds.
///
/// `root` is used to strip the absolute path into a project-relative
/// path for the returned `file_path` field — never leaked on failure
/// messages either (the `Io` variant carries the OS message verbatim,
/// but `std::fs::read_to_string` only surfaces its message, not the
/// path).
///
/// Top-level symbols are capped at
/// [`UNDERSTAND_CODE_MAX_TOP_LEVEL_SYMBOLS`] with a `tracing::warn`
/// when truncation fires.
///
/// # Errors
///
/// Returns any of [`UnderstandCodeError::Io`], [`Parse`],
/// [`UnsupportedLanguage`], or [`KnowledgeGraph`] as produced by the
/// steps above.
///
/// [`Parse`]: UnderstandCodeError::Parse
/// [`UnsupportedLanguage`]: UnderstandCodeError::UnsupportedLanguage
/// [`KnowledgeGraph`]: UnderstandCodeError::KnowledgeGraph
pub fn explain_file(
    target_path: &Path,
    root: &Path,
    kg: &Arc<Mutex<KnowledgeGraph>>,
) -> Result<UnderstandCodeFileSummary, UnderstandCodeError> {
    // Read source (std::io → UnderstandCodeError::Io via `From`).
    let source = std::fs::read_to_string(target_path)?;

    // Detect language from extension.
    let ext = target_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let lang = Language::from_extension(ext).ok_or_else(|| {
        let pretty = project_relative(target_path, root);
        UnderstandCodeError::UnsupportedLanguage(pretty.display().to_string())
    })?;

    // Parse + extract symbols.
    let mut parser = Parser::new();
    let tree = parser.parse(&source, lang)?;
    let extractor = SymbolExtractor::new();
    let symbols = extractor.extract(&tree, &source, target_path, lang);

    // Count imports and lines.
    let import_count = count_imports(&source, lang);
    let line_count: u64 = u64::try_from(source.lines().count()).unwrap_or(u64::MAX);

    // KG side — acquire lock for a short read.  The knowledge graph's
    // `entities.file_path` column stores whatever string the ingest
    // pipeline was given (see `ucil_daemon::executor::IngestPipeline::
    // ingest_file`, which records `path.display().to_string()` verbatim).
    // In production the daemon feeds absolute paths after watcher-side
    // canonicalisation, but historical/fixture rows may carry a
    // project-relative path.  To stay robust to both conventions we
    // query by both the canonical absolute form AND the path the
    // caller handed us, then dedupe by `entities.id`.
    let canonical = target_path
        .canonicalize()
        .unwrap_or_else(|_| target_path.to_path_buf());
    let canonical_str = canonical.display().to_string();
    let raw_str = target_path.display().to_string();
    let relative_str = project_relative(target_path, root).display().to_string();
    let kg_entities: Vec<Entity> = {
        let guard = kg.lock().map_err(|_| UnderstandCodeError::Poisoned)?;
        let mut seen_ids: std::collections::HashSet<Option<i64>> = std::collections::HashSet::new();
        let mut merged: Vec<Entity> = Vec::new();
        for key in [&canonical_str, &raw_str, &relative_str] {
            for row in guard.list_entities_by_file(key)? {
                if seen_ids.insert(row.id) {
                    merged.push(row);
                }
            }
        }
        merged
    };
    let kg_entity_count = kg_entities.len();

    // Zip tree-sitter symbols with KG entities on (name, start_line).
    let mut rows: Vec<FileSymbolRow> = symbols
        .iter()
        .map(|s| {
            let kind = symbol_kind_tag(s.kind).to_owned();
            let matching_qn = kg_entities
                .iter()
                .find(|e| e.name == s.name && e.start_line == Some(i64::from(s.start_line)))
                .and_then(|e| e.qualified_name.clone());
            FileSymbolRow {
                kind,
                name: s.name.clone(),
                qualified_name: matching_qn,
                start_line: s.start_line,
            }
        })
        .collect();

    // Cap the roster.
    if rows.len() > UNDERSTAND_CODE_MAX_TOP_LEVEL_SYMBOLS {
        tracing::warn!(
            raw = rows.len(),
            cap = UNDERSTAND_CODE_MAX_TOP_LEVEL_SYMBOLS,
            "understand_code: top_level_symbols truncated",
        );
        rows.truncate(UNDERSTAND_CODE_MAX_TOP_LEVEL_SYMBOLS);
    }

    let file_path_out = project_relative(target_path, root);

    Ok(UnderstandCodeFileSummary {
        language: language_tag(lang),
        file_path: file_path_out,
        line_count,
        import_count,
        top_level_symbols: rows,
        kg_entity_count,
    })
}

/// Strip `root` from `path` when possible, returning a relative path.
/// Otherwise return `path` unchanged — never errors.
fn project_relative(path: &Path, root: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Map a [`ucil_treesitter::SymbolKind`] to its snake-case wire tag,
/// matching the `#[serde(rename_all = "snake_case")]` convention on
/// the upstream enum.
///
/// [`ucil_treesitter::SymbolKind`] is `#[non_exhaustive]` so the
/// compiler requires a wildcard arm; unseen variants fall through to
/// `"other"` rather than panicking.
fn symbol_kind_tag(kind: ucil_treesitter::SymbolKind) -> &'static str {
    use ucil_treesitter::SymbolKind;
    match kind {
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
        _ => "other",
    }
}

// ── explain_symbol ───────────────────────────────────────────────────────────

/// Build the symbol-mode [`UnderstandCodeSymbolSummary`] for
/// `target` (a qualified name).
///
/// Runs:
/// 1. [`KnowledgeGraph::get_entity_by_qualified_name`] — `Ok(None)` →
///    `NotFound` (the outer handler translates into a well-formed
///    `_meta.found == false` envelope).
/// 2. [`KnowledgeGraph::list_relations_by_source`] +
///    `list_relations_by_target` — enumerate outbound / inbound edges.
/// 3. [`KnowledgeGraph::get_entity_by_id`] per edge to resolve the
///    peer.  Relations whose peer is missing (dangling FK) are logged
///    at `tracing::debug` and dropped.
/// 4. Sort edges by `(relation_type, peer_qualified_name)` for
///    deterministic output.
/// 5. Cap each direction at [`UNDERSTAND_CODE_MAX_EDGES`].
/// 6. If the resolved entity's `file_path` is a readable file under
///    `root`, call [`explain_file`] and attach the file summary to
///    `containing_file`.  File-not-found or non-file parents are
///    logged at `tracing::debug` and set `containing_file = None` —
///    never an error.
///
/// # Errors
///
/// Returns [`UnderstandCodeError::KnowledgeGraph`] on mutex poisoning
/// or a KG read failure, or [`UnderstandCodeError::NotFound`] when the
/// symbol does not resolve.
pub fn explain_symbol(
    target: &str,
    kg: &Arc<Mutex<KnowledgeGraph>>,
    root: &Path,
) -> Result<UnderstandCodeSymbolSummary, UnderstandCodeError> {
    // 1. Resolve entity.
    let entity: Entity = {
        let guard = kg.lock().map_err(|_| UnderstandCodeError::Poisoned)?;
        match guard.get_entity_by_qualified_name(target, None)? {
            Some(e) => e,
            None => return Err(UnderstandCodeError::NotFound(target.to_owned())),
        }
    };
    let entity_id = match entity.id {
        Some(i) => i,
        None => return Err(UnderstandCodeError::NotFound(target.to_owned())),
    };

    // 2. Enumerate edges — inbound (target_id = entity) +
    //    outbound (source_id = entity).  Peer projection uses
    //    get_entity_by_id per edge; dangling rows are logged and
    //    dropped.
    let (inbound_edges, outbound_edges) = {
        let guard = kg.lock().map_err(|_| UnderstandCodeError::Poisoned)?;
        let outbound_rel = guard.list_relations_by_source(entity_id)?;
        let inbound_rel = guard.list_relations_by_target(entity_id)?;
        let outbound: Vec<RelationEdge> = outbound_rel
            .iter()
            .filter_map(|r| match guard.get_entity_by_id(r.target_id) {
                Ok(Some(peer)) => Some(relation_edge_from(r.kind.clone(), &peer)),
                Ok(None) => {
                    tracing::debug!(
                        relation_id = ?r.id,
                        target_id = r.target_id,
                        "understand_code: outbound peer missing (dangling fk)",
                    );
                    None
                }
                Err(e) => {
                    tracing::debug!(
                        relation_id = ?r.id,
                        target_id = r.target_id,
                        "understand_code: get_entity_by_id failed on outbound: {e}",
                    );
                    None
                }
            })
            .collect();
        let inbound: Vec<RelationEdge> = inbound_rel
            .iter()
            .filter_map(|r| match guard.get_entity_by_id(r.source_id) {
                Ok(Some(peer)) => Some(relation_edge_from(r.kind.clone(), &peer)),
                Ok(None) => {
                    tracing::debug!(
                        relation_id = ?r.id,
                        source_id = r.source_id,
                        "understand_code: inbound peer missing (dangling fk)",
                    );
                    None
                }
                Err(e) => {
                    tracing::debug!(
                        relation_id = ?r.id,
                        source_id = r.source_id,
                        "understand_code: get_entity_by_id failed on inbound: {e}",
                    );
                    None
                }
            })
            .collect();
        (inbound, outbound)
    };

    let inbound_relation_count = inbound_edges.len();
    let outbound_relation_count = outbound_edges.len();

    // 3. Sort deterministically and cap.
    let inbound_edges = cap_and_sort(inbound_edges);
    let outbound_edges = cap_and_sort(outbound_edges);

    // 4. Opportunistically load the containing-file summary.
    let containing_file = try_containing_file(&entity, root, kg);

    Ok(UnderstandCodeSymbolSummary {
        entity: EntitySummary::from_entity(&entity),
        inbound_relation_count,
        outbound_relation_count,
        inbound_edges,
        outbound_edges,
        containing_file,
    })
}

/// Build a `RelationEdge` from a relation kind and a resolved peer
/// entity.  Falls back to the peer's unqualified `name` when the peer
/// has no `qualified_name` (the schema allows `None` per §12.1).
fn relation_edge_from(kind: String, peer: &Entity) -> RelationEdge {
    let peer_qn = peer
        .qualified_name
        .clone()
        .unwrap_or_else(|| peer.name.clone());
    RelationEdge {
        relation_type: kind,
        peer_qualified_name: peer_qn,
        peer_file_path: Some(peer.file_path.clone()),
        peer_start_line: peer.start_line.and_then(|n| u32::try_from(n).ok()),
    }
}

/// Sort `edges` by `(relation_type, peer_qualified_name)` and cap at
/// [`UNDERSTAND_CODE_MAX_EDGES`], logging a `tracing::warn` when the
/// cap fires.
fn cap_and_sort(mut edges: Vec<RelationEdge>) -> Vec<RelationEdge> {
    edges.sort_by(|a, b| {
        a.relation_type
            .cmp(&b.relation_type)
            .then_with(|| a.peer_qualified_name.cmp(&b.peer_qualified_name))
    });
    if edges.len() > UNDERSTAND_CODE_MAX_EDGES {
        tracing::warn!(
            raw = edges.len(),
            cap = UNDERSTAND_CODE_MAX_EDGES,
            "understand_code: edges truncated",
        );
        edges.truncate(UNDERSTAND_CODE_MAX_EDGES);
    }
    edges
}

/// Try to attach an [`UnderstandCodeFileSummary`] for the resolved
/// entity's `file_path` — best-effort.  File read errors, parse
/// errors, and KG errors are logged at `tracing::debug` and return
/// `None` so `explain_symbol` stays successful.
fn try_containing_file(
    entity: &Entity,
    root: &Path,
    kg: &Arc<Mutex<KnowledgeGraph>>,
) -> Option<UnderstandCodeFileSummary> {
    let entity_path = Path::new(&entity.file_path);
    if !entity_path.is_file() {
        tracing::debug!(
            path = %entity.file_path,
            "understand_code: entity.file_path is not a readable file; skipping containing_file",
        );
        return None;
    }
    match explain_file(entity_path, root, kg) {
        Ok(summary) => Some(summary),
        Err(e) => {
            tracing::debug!(
                path = %entity.file_path,
                "understand_code: containing_file fetch failed: {e}",
            );
            None
        }
    }
}

// ── handle_understand_code ───────────────────────────────────────────────────

/// Dispatch the `understand_code` MCP tool call.
///
/// Argument shape (master-plan §3.2 row 1 + WO-0036 `scope_in`):
///
/// * `arguments.target` — required non-empty string.  Interpreted as
///   a file path (when `kind == "file" | "module"` or auto-detected
///   to be a file under `root`) or a qualified symbol name (when
///   `kind == "symbol"` or auto-detected as such).
/// * `arguments.kind` — optional enum `"file" | "symbol" | "module"`.
///   Absent / `null` triggers auto-detection.  `"module"` is mapped
///   onto file mode in Phase 1 (the real module walk lives in
///   Phase 2 under `trace_dependencies`).
/// * `arguments.root` — optional string path that defaults to
///   `std::env::current_dir`.  The string is canonicalised and must
///   resolve to an existing directory.
///
/// Error codes (JSON-RPC):
///
/// * `-32602` — `arguments.target` missing/non-string/empty,
///   `arguments.kind` non-string or not in `{"file","symbol","module"}`,
///   `arguments.root` non-string/missing-on-disk/not-a-directory.
/// * `-32603` — file read / parse / KG / mutex-poisoning failure.
///
/// Not-found shape (scope_in bullet 7): when
/// `get_entity_by_qualified_name` returns `None`, the response
/// envelope carries `_meta.found == false` and `isError == false` —
/// *not* a JSON-RPC error.
pub fn handle_understand_code(
    id: &Value,
    params: &Value,
    kg: &Arc<Mutex<KnowledgeGraph>>,
) -> Value {
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // ── arguments.target ─────────────────────────────────────────────
    let target: String = match args.get("target") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::String(_)) => {
            return jsonrpc_error(
                id,
                -32602,
                "understand_code: `arguments.target` is required and must be a non-empty string",
            );
        }
        _ => {
            return jsonrpc_error(
                id,
                -32602,
                "understand_code: `arguments.target` is required and must be a non-empty string",
            );
        }
    };

    // ── arguments.kind ───────────────────────────────────────────────
    let kind_opt: Option<&str> = match args.get("kind") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            if !matches!(s.as_str(), "file" | "symbol" | "module") {
                return jsonrpc_error(
                    id,
                    -32602,
                    "understand_code: `arguments.kind` must be one of \"file\", \"symbol\", or \"module\"",
                );
            }
            Some(s.as_str())
        }
        Some(_) => {
            return jsonrpc_error(
                id,
                -32602,
                "understand_code: `arguments.kind` must be a string (or omitted/null)",
            );
        }
    };

    // ── arguments.root ───────────────────────────────────────────────
    let root: PathBuf = match args.get("root") {
        None | Some(Value::Null) => match std::env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                return jsonrpc_error(
                    id,
                    -32603,
                    &format!("understand_code: could not resolve current_dir: {e}"),
                );
            }
        },
        Some(Value::String(s)) if s.is_empty() => match std::env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                return jsonrpc_error(
                    id,
                    -32603,
                    &format!("understand_code: could not resolve current_dir: {e}"),
                );
            }
        },
        Some(Value::String(s)) => PathBuf::from(s),
        Some(_) => {
            return jsonrpc_error(
                id,
                -32602,
                "understand_code: `arguments.root` must be a string (or omitted/null)",
            );
        }
    };
    let root = match root.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return jsonrpc_error(
                id,
                -32602,
                &format!(
                    "understand_code: `arguments.root` does not exist: {}",
                    root.display()
                ),
            );
        }
    };
    if !root.is_dir() {
        return jsonrpc_error(
            id,
            -32602,
            &format!(
                "understand_code: `arguments.root` is not a directory: {}",
                root.display()
            ),
        );
    }

    // ── Auto-detect kind + candidate path ────────────────────────────
    let candidate_path = root.join(&target);
    let is_existing_file = candidate_path
        .canonicalize()
        .map(|p| p.is_file())
        .unwrap_or(false);
    let effective_kind: &str = match kind_opt {
        Some(k) => k,
        None => {
            if is_existing_file {
                "file"
            } else {
                "symbol"
            }
        }
    };

    // ── Dispatch ─────────────────────────────────────────────────────
    match effective_kind {
        "file" | "module" => {
            // Resolve and canonicalise the file.  Under file/module
            // mode, the target MUST resolve to a regular file under
            // `root`.
            let resolved = match candidate_path.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    return jsonrpc_error(
                        id,
                        -32602,
                        &format!("understand_code: target file not found: {target}"),
                    );
                }
            };
            if !resolved.is_file() {
                return jsonrpc_error(
                    id,
                    -32602,
                    &format!("understand_code: target is not a regular file: {target}"),
                );
            }
            if resolved.strip_prefix(&root).is_err() {
                return jsonrpc_error(
                    id,
                    -32602,
                    &format!("understand_code: target {target} escapes root"),
                );
            }
            match explain_file(&resolved, &root, kg) {
                Ok(summary) => file_response(id, &target, effective_kind, &summary),
                Err(e) => understand_error_to_envelope(id, &e),
            }
        }
        "symbol" => match explain_symbol(&target, kg, &root) {
            Ok(summary) => symbol_response(id, &target, &summary),
            Err(UnderstandCodeError::NotFound(_)) => not_found_response(id, &target),
            Err(e) => understand_error_to_envelope(id, &e),
        },
        _ => jsonrpc_error(
            id,
            -32602,
            "understand_code: `arguments.kind` must be one of \"file\", \"symbol\", or \"module\"",
        ),
    }
}

/// Convert a non-`NotFound` [`UnderstandCodeError`] into a JSON-RPC
/// error envelope with code `-32603` (internal error) — never `-32602`
/// (those are all caught pre-dispatch in [`handle_understand_code`]).
fn understand_error_to_envelope(id: &Value, e: &UnderstandCodeError) -> Value {
    tracing::error!("understand_code: {e}");
    let code = match e {
        UnderstandCodeError::UnsupportedLanguage(_) | UnderstandCodeError::TargetNotInRoot(_) => {
            -32602
        }
        _ => -32603,
    };
    jsonrpc_error(id, code, &e.to_string())
}

/// Build the file-mode happy-path response envelope.
fn file_response(
    id: &Value,
    target: &str,
    kind: &str,
    summary: &UnderstandCodeFileSummary,
) -> Value {
    let summary_json = serde_json::to_value(summary).unwrap_or(Value::Null);
    let text = format!(
        "{lang} file `{path}` — {lines} lines, {imports} imports, {syms} top-level symbols",
        lang = summary.language,
        path = summary.file_path.display(),
        lines = summary.line_count,
        imports = summary.import_count,
        syms = summary.top_level_symbols.len(),
    );
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "result": {
            "_meta": {
                "tool": "understand_code",
                "source": "tree-sitter+kg",
                "kind": kind,
                "target": target,
                "summary": summary_json,
            },
            "content": [ { "type": "text", "text": text } ],
            "isError": false
        }
    })
}

/// Build the symbol-mode happy-path response envelope.
fn symbol_response(id: &Value, target: &str, summary: &UnderstandCodeSymbolSummary) -> Value {
    let summary_json = serde_json::to_value(summary).unwrap_or(Value::Null);
    let text = format!(
        "symbol `{name}` — {inbound} inbound, {outbound} outbound relations",
        name = summary
            .entity
            .qualified_name
            .clone()
            .unwrap_or_else(|| summary.entity.name.clone()),
        inbound = summary.inbound_relation_count,
        outbound = summary.outbound_relation_count,
    );
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "result": {
            "_meta": {
                "tool": "understand_code",
                "source": "tree-sitter+kg",
                "kind": "symbol",
                "target": target,
                "found": true,
                "summary": summary_json,
            },
            "content": [ { "type": "text", "text": text } ],
            "isError": false
        }
    })
}

/// Build the symbol-not-found response envelope — per WO-0036
/// `scope_in` bullet 7 this is a successful response with
/// `_meta.found == false`, NOT a JSON-RPC error.
fn not_found_response(id: &Value, target: &str) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "result": {
            "_meta": {
                "tool": "understand_code",
                "source": "tree-sitter+kg",
                "kind": "symbol",
                "target": target,
                "found": false,
            },
            "content": [
                { "type": "text", "text": format!("no symbol found for `{target}`") }
            ],
            "isError": false
        }
    })
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_imports_rust_counts_use_declarations() {
        let src = "use std::collections::HashMap;\nuse serde::Deserialize;\nfn main() {}\n";
        assert_eq!(count_imports(src, Language::Rust), 2);
    }

    #[test]
    fn count_imports_rust_ignores_comments_and_strings() {
        // A string that mentions "use" and a line_comment that does —
        // string matching would give 3+; tree-sitter gives 1.
        let src = "// use not_really\n\
                   fn main() { let s = \"use std::foo;\"; }\n\
                   use real::imp;\n";
        assert_eq!(count_imports(src, Language::Rust), 1);
    }

    #[test]
    fn count_imports_python_counts_both_forms() {
        let src = "import os\nfrom typing import List\n\ndef main(): pass\n";
        assert_eq!(count_imports(src, Language::Python), 2);
    }

    #[test]
    fn count_imports_typescript_counts_import_and_require() {
        let src = "import { foo } from 'bar';\nconst x = require('baz');\n";
        assert_eq!(count_imports(src, Language::TypeScript), 2);
    }

    #[test]
    fn count_imports_go_counts_declarations() {
        let src = "package main\n\nimport \"fmt\"\n\nimport (\n  \"os\"\n  \"strings\"\n)\n";
        // Two import declarations (the block counts as one).
        assert_eq!(count_imports(src, Language::Go), 2);
    }

    #[test]
    fn count_imports_returns_zero_for_unsupported_language() {
        assert_eq!(count_imports("echo hi", Language::Bash), 0);
        assert_eq!(count_imports("{\"a\":1}", Language::Json), 0);
    }

    #[test]
    fn language_tag_roundtrip() {
        assert_eq!(language_tag(Language::Rust), "rust");
        assert_eq!(language_tag(Language::Python), "python");
        assert_eq!(language_tag(Language::TypeScript), "typescript");
        assert_eq!(language_tag(Language::Go), "go");
    }

    #[test]
    fn relation_edge_falls_back_to_name_when_no_qualified_name() {
        let peer = Entity {
            id: Some(7),
            kind: "function".to_owned(),
            name: "orphan".to_owned(),
            qualified_name: None,
            file_path: "/tmp/a.rs".to_owned(),
            start_line: Some(9),
            end_line: Some(11),
            signature: None,
            doc_comment: None,
            language: Some("rust".to_owned()),
            t_valid_from: None,
            t_valid_to: None,
            importance: 0.5,
            source_tool: None,
            source_hash: None,
        };
        let edge = relation_edge_from("calls".to_owned(), &peer);
        assert_eq!(edge.peer_qualified_name, "orphan");
        assert_eq!(edge.peer_start_line, Some(9));
    }

    #[test]
    fn cap_and_sort_sorts_deterministically_and_caps() {
        let edges = vec![
            RelationEdge {
                relation_type: "calls".to_owned(),
                peer_qualified_name: "b".to_owned(),
                peer_file_path: None,
                peer_start_line: None,
            },
            RelationEdge {
                relation_type: "calls".to_owned(),
                peer_qualified_name: "a".to_owned(),
                peer_file_path: None,
                peer_start_line: None,
            },
            RelationEdge {
                relation_type: "imports".to_owned(),
                peer_qualified_name: "z".to_owned(),
                peer_file_path: None,
                peer_start_line: None,
            },
        ];
        let sorted = cap_and_sort(edges);
        assert_eq!(sorted[0].peer_qualified_name, "a");
        assert_eq!(sorted[1].peer_qualified_name, "b");
        assert_eq!(sorted[2].relation_type, "imports");
    }
}
