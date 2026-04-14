//! Core domain types shared across all UCIL crates.
//!
//! Every type in this module is `Serialize + Deserialize` so it can cross
//! MCP tool call boundaries without custom marshalling.  All types also
//! derive `Debug`, `Clone`, and `PartialEq` for testability.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── QueryPlan ────────────────────────────────────────────────────────────────

/// The intent decomposition produced by the CEQP planner for a single user query.
///
/// `QueryPlan` is the primary hand-off object between the query-understanding
/// layer and the retrieval/fusion layer.  Consumers must not assume any field
/// is non-empty; every `Vec` and `HashMap` may legitimately be empty for
/// trivial or malformed queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryPlan {
    /// High-level user intent extracted from the raw query string
    /// (e.g., `"explain the tokio scheduler"`).
    pub intent: String,

    /// Knowledge domains the query touches (e.g., `["rust", "async", "tokio"]`).
    pub domains: Vec<String>,

    /// Decomposed sub-queries dispatched to individual retrieval agents.
    pub sub_queries: Vec<String>,

    /// Concepts the system could not resolve from the existing knowledge graph.
    pub knowledge_gaps: Vec<String>,

    /// Context inferred automatically (e.g., `{"language": "rust", "edition": "2021"}`).
    pub inferred_context: HashMap<String, String>,

    /// When `true` the engine degrades to best-effort retrieval because a
    /// required plugin is unavailable or the query could not be parsed.
    pub fallback_mode: bool,
}

// ── Symbol ───────────────────────────────────────────────────────────────────

/// A code symbol resolved from a source file via an LSP or tree-sitter pass.
///
/// `Symbol` is immutable once constructed; callers that need mutation should
/// clone and rebuild.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    /// Unqualified symbol name (e.g., `"my_function"`, `"MyStruct"`).
    pub name: String,

    /// Symbol category as reported by the language server
    /// (e.g., `"function"`, `"struct"`, `"method"`, `"variable"`).
    pub kind: String,

    /// Absolute path to the file that defines this symbol.
    pub file_path: PathBuf,

    /// 1-indexed line number of the symbol definition.
    pub line: u32,

    /// 1-indexed column offset of the symbol definition.
    pub col: u32,

    /// Source language identifier (e.g., `"rust"`, `"python"`, `"typescript"`).
    pub language: String,

    /// Extracted documentation comment attached to this symbol, if any.
    pub doc_comment: Option<String>,
}

// ── Diagnostic ───────────────────────────────────────────────────────────────

/// A compiler or linter diagnostic attached to a specific source location.
///
/// Diagnostics are collected by the LSP layer and forwarded to the fusion
/// engine for ranking against the knowledge graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Absolute path of the file that produced this diagnostic.
    pub file_path: PathBuf,

    /// 1-indexed line number of the diagnostic anchor.
    pub line: u32,

    /// 1-indexed column offset of the diagnostic anchor.
    pub col: u32,

    /// Severity level as a string (e.g., `"error"`, `"warning"`, `"hint"`,
    /// `"information"`).
    pub severity: String,

    /// Tool-specific diagnostic code (e.g., `"E0308"`, `"clippy::pedantic"`).
    /// `None` when the tool does not emit codes.
    pub code: Option<String>,

    /// Human-readable diagnostic description.
    pub message: String,

    /// Name of the tool or language server that produced this diagnostic
    /// (e.g., `"rustc"`, `"clippy"`, `"mypy"`, `"typescript"`).
    pub source: String,
}

// ── KnowledgeEntry ───────────────────────────────────────────────────────────

/// A persisted entry in the UCIL knowledge graph.
///
/// Each `KnowledgeEntry` corresponds to one [`Symbol`] and stores its
/// documentation, embedding vector, and arbitrary metadata.  Entries are
/// identified by a stable, content-addressed `id` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    /// Stable, content-addressed identifier (e.g., SHA-256 hex of canonical form).
    pub id: String,

    /// The code symbol this entry describes.
    pub symbol: Symbol,

    /// Free-text or structured content derived from rustdoc / docstrings /
    /// comment extraction.
    pub content: String,

    /// Dense embedding vector for semantic retrieval.  Empty (`vec![]`) before
    /// the embedding pass runs.
    pub embedding_vec: Vec<f32>,

    /// ISO-8601 creation timestamp (e.g., `"2026-04-15T06:00:00Z"`).
    pub created_at: String,

    /// ISO-8601 last-modified timestamp.
    pub updated_at: String,

    /// Arbitrary metadata key–value pairs (e.g., crate name, git revision).
    pub meta: HashMap<String, String>,
}

// ── ToolGroup ────────────────────────────────────────────────────────────────

/// A named group of MCP tools that may be executed concurrently.
///
/// The UCIL planner assembles `ToolGroup`s before dispatching work to the
/// MCP adapter layer.  Tools within a group may run in parallel up to
/// [`ToolGroup::parallelism`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolGroup {
    /// Unique identifier for this tool group (e.g., `"tg-lsp-retrieval-01"`).
    pub id: String,

    /// Human-readable name displayed in traces and logs.
    pub name: String,

    /// Ordered list of MCP tool names belonging to this group.
    pub tools: Vec<String>,

    /// Maximum number of tools to execute concurrently within this group.
    /// A value of `1` forces sequential execution.
    pub parallelism: u32,
}

// ── CeqpParams ───────────────────────────────────────────────────────────────

/// Parameters for a CEQP (Continuous Evaluation and Query Planning) run.
///
/// These parameters are passed from the orchestration layer down to the CEQP
/// engine.  All fields are required; callers must not supply placeholder values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CeqpParams {
    /// Human-readable rationale for initiating this evaluation
    /// (e.g., `"user invoked /explain"`).
    pub reason: String,

    /// Evaluation target — typically a file path, symbol pattern, or natural-
    /// language query (e.g., `"src/lib.rs"`, `"tokio::spawn"`).
    pub target: String,

    /// Identifier of the Claude Code session that triggered this evaluation.
    pub session_id: String,

    /// Current git branch at the time of the request.
    pub branch: String,

    /// Maximum recursion depth for sub-query expansion.  The engine aborts
    /// further expansion once this limit is reached.
    pub depth_limit: u32,

    /// Maximum wall-clock budget for the entire evaluation, in milliseconds.
    /// The engine applies `tokio::time::timeout` using this value.
    pub timeout_ms: u64,
}

// ── ResponseEnvelope ─────────────────────────────────────────────────────────

/// Top-level response envelope returned by every UCIL MCP tool call.
///
/// The envelope provides uniform metadata across all tool responses so that
/// adapters (Claude Code, Codex, Cursor, …) can handle degraded-mode and
/// observability signals without inspecting the inner `result`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    /// Echo of the originating request identifier for correlation.
    pub request_id: String,

    /// Name of the MCP tool that produced this response
    /// (e.g., `"ucil_query"`, `"ucil_diagnostics"`).
    pub tool_name: String,

    /// JSON-encoded tool result payload.  Structure is tool-specific.
    pub result: serde_json::Value,

    /// Arbitrary response metadata (e.g., `{"cache_hit": "true", "latency_ms": "42"}`).
    pub meta: HashMap<String, String>,

    /// Names of plugins that were unavailable during this call.  Empty when
    /// all required plugins responded successfully.
    pub degraded_plugins: Vec<String>,

    /// Current indexing progress, in the range `[0.0, 1.0]`.  `1.0` means
    /// the knowledge graph is fully up-to-date.
    pub indexing_status: f64,

    /// OpenTelemetry trace identifier for log correlation, if tracing is
    /// active.  `None` when the `OTel` provider has not been initialised.
    pub otel_trace_id: Option<String>,
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{
        CeqpParams, Diagnostic, KnowledgeEntry, QueryPlan, ResponseEnvelope, Symbol, ToolGroup,
    };

    // Helper: assert round-trip JSON serialisation preserves equality.
    fn roundtrip<T>(value: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let decoded: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(value, &decoded);
    }

    #[test]
    fn query_plan_roundtrip() {
        let qp = QueryPlan {
            intent: "explain tokio scheduler".into(),
            domains: vec!["rust".into(), "async".into()],
            sub_queries: vec!["how does tokio schedule tasks?".into()],
            knowledge_gaps: vec![],
            inferred_context: [("edition".into(), "2021".into())].into(),
            fallback_mode: false,
        };
        roundtrip(&qp);
        // clone + PartialEq
        assert_eq!(qp.clone(), qp);
        // Debug must not panic
        let _ = format!("{qp:?}");
    }

    #[test]
    fn symbol_roundtrip() {
        let sym = Symbol {
            name: "my_fn".into(),
            kind: "function".into(),
            file_path: PathBuf::from("/src/lib.rs"),
            line: 42,
            col: 1,
            language: "rust".into(),
            doc_comment: Some("Does something useful.".into()),
        };
        roundtrip(&sym);
        assert_eq!(sym.clone(), sym);
        let _ = format!("{sym:?}");
    }

    #[test]
    fn diagnostic_roundtrip() {
        let diag = Diagnostic {
            file_path: PathBuf::from("/src/main.rs"),
            line: 10,
            col: 5,
            severity: "error".into(),
            code: Some("E0308".into()),
            message: "mismatched types".into(),
            source: "rustc".into(),
        };
        roundtrip(&diag);
        assert_eq!(diag.clone(), diag);
        let _ = format!("{diag:?}");
    }

    #[test]
    fn knowledge_entry_roundtrip() {
        let sym = Symbol {
            name: "spawn".into(),
            kind: "function".into(),
            file_path: PathBuf::from("/tokio/src/task.rs"),
            line: 1,
            col: 1,
            language: "rust".into(),
            doc_comment: None,
        };
        let ke = KnowledgeEntry {
            id: "abc123".into(),
            symbol: sym,
            content: "Spawns a new async task.".into(),
            embedding_vec: vec![0.1_f32, 0.2_f32, 0.3_f32],
            created_at: "2026-04-15T00:00:00Z".into(),
            updated_at: "2026-04-15T00:00:00Z".into(),
            meta: [("crate".into(), "tokio".into())].into(),
        };
        roundtrip(&ke);
        assert_eq!(ke.clone(), ke);
        let _ = format!("{ke:?}");
    }

    #[test]
    fn tool_group_roundtrip() {
        let tg = ToolGroup {
            id: "tg-01".into(),
            name: "LSP retrieval".into(),
            tools: vec!["ucil_query".into(), "ucil_diagnostics".into()],
            parallelism: 2,
        };
        roundtrip(&tg);
        assert_eq!(tg.clone(), tg);
        let _ = format!("{tg:?}");
    }

    #[test]
    fn ceqp_params_roundtrip() {
        let params = CeqpParams {
            reason: "user invoked /explain".into(),
            target: "src/lib.rs".into(),
            session_id: "sess-xyz".into(),
            branch: "main".into(),
            depth_limit: 3,
            timeout_ms: 5000,
        };
        roundtrip(&params);
        assert_eq!(params.clone(), params);
        let _ = format!("{params:?}");
    }

    #[test]
    fn response_envelope_roundtrip() {
        let env = ResponseEnvelope {
            request_id: "req-001".into(),
            tool_name: "ucil_query".into(),
            result: json!({"answer": "42"}),
            meta: [("latency_ms".into(), "7".into())].into(),
            degraded_plugins: vec![],
            indexing_status: 1.0,
            otel_trace_id: Some("abcdef0123456789".into()),
        };
        roundtrip(&env);
        assert_eq!(env.clone(), env);
        let _ = format!("{env:?}");
    }
}
