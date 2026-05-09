//! MCP server skeleton — JSON-RPC 2.0 over stdio.
//!
//! This module implements the **basic** subset of UCIL's Model Context
//! Protocol surface mandated by `P1-W3-F07`:
//!
//! * A [`ToolDescriptor`] catalog listing all 22 UCIL tools from
//!   master-plan §3.2.  Every descriptor's `input_schema` is a valid
//!   JSON-Schema object carrying the four CEQP universal parameters
//!   (`reason`, `current_task`, `files_in_context`, `token_budget`) per
//!   master-plan §8.2.
//! * An [`McpServer`] façade whose [`McpServer::serve`] method reads
//!   newline-delimited JSON-RPC 2.0 requests from any
//!   [`tokio::io::AsyncRead`] and writes newline-delimited responses to
//!   any [`tokio::io::AsyncWrite`] — the same wire format the daemon
//!   will use against a host agent's stdio (master-plan §10.2, phase-1
//!   invariant #6).
//! * Three JSON-RPC methods: `initialize`, `tools/list`, and
//!   `tools/call`.  Every `tools/call` handler is a **stub** that
//!   returns an envelope whose top-level result contains
//!   `_meta.not_yet_implemented: true` (phase-1 invariant #9).
//!
//! # Wire protocol
//!
//! One JSON-RPC 2.0 message per line, terminated by `\n`:
//!
//! ```json
//! // request
//! {"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}
//! // response
//! {"jsonrpc":"2.0","id":1,"result":{"tools":[…22 descriptors…]}}
//! ```
//!
//! Every `.await` on IO is wrapped in a [`tokio::time::timeout`] with a
//! named const (rust-style.md), and the read loop exits cleanly on EOF.

// Public API items share a name prefix with the module ("server" →
// `McpServer`, `McpError`).  Matches the convention set by
// `plugin_manager` and `session_manager`.
#![allow(clippy::module_name_repetitions)]

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{json, Value};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    time::timeout,
};
use ucil_core::KnowledgeGraph;

use crate::branch_manager::BranchManager;
use crate::g2_search::G2SourceFactory;
use crate::g4::{
    execute_g4, G4Query, G4Source, G4SourceOutput, G4SourceStatus, G4UnifiedEdge, G4UnionOutcome,
    G4_MASTER_DEADLINE,
};
use crate::g7::{execute_g7, merge_g7_by_severity, G7Query, G7Source, G7_DEFAULT_MASTER_DEADLINE};
use crate::g8::{
    execute_g8, merge_g8_test_discoveries, G8Query, G8Source, G8_DEFAULT_MASTER_DEADLINE,
};
use crate::lancedb_indexer::EmbeddingSource;
use crate::text_search::{self, TextMatch, TextSearchError};
use ucil_core::{fuse_g2_rrf, G2FusedOutcome, G2SourceResults};
use ucil_lsp_diagnostics::{DiagnosticsClient, Language};

// ── Constants ────────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 protocol version string — written on every response
/// frame and required on every inbound request.
pub const JSONRPC_VERSION: &str = "2.0";

/// Protocol version advertised in the `initialize` response.  Matches
/// the shipped MCP spec that UCIL targets for Phase 1.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Timeout budget, in milliseconds, for a single `read_line` of an
/// inbound JSON-RPC frame.
///
/// Ten seconds is generous enough for a human-paced test harness and
/// tight enough that a wedged client does not hang the server
/// indefinitely.
pub const READ_TIMEOUT_MS: u64 = 10_000;

/// Timeout budget, in milliseconds, for a single response write +
/// flush.
pub const WRITE_TIMEOUT_MS: u64 = 5_000;

const READ_TIMEOUT: Duration = Duration::from_millis(READ_TIMEOUT_MS);
const WRITE_TIMEOUT: Duration = Duration::from_millis(WRITE_TIMEOUT_MS);

/// The number of UCIL tools exposed over MCP, per master-plan §3.2.
pub const TOOL_COUNT: usize = 22;

/// Default value for `search_code`'s `arguments.max_results`.
///
/// Chosen to keep the default response envelope well under an MCP
/// host's context window while still covering the common "show me
/// every hit in a small tree" use case.  Paired with
/// [`SEARCH_CODE_MAX_RESULTS`], which caps whatever the caller asks
/// for so a pathological query cannot flood the response.
pub const SEARCH_CODE_DEFAULT_MAX_RESULTS: usize = 50;

/// Per-source deadline for the G2 fan-out per `DEC-0015` D1.
///
/// Wraps each [`crate::g2_search::G2SourceProvider::execute`] future so
/// a single slow engine cannot stall the response — partial results
/// semantics matching `fuse_g1` from WO-0048.
pub const G2_PER_SOURCE_DEADLINE: Duration = Duration::from_secs(2);

/// Master deadline for the G2 fan-out per `DEC-0015` D1.
///
/// Bounds the total wall-time across all parallel providers — the
/// outer cap on top of the per-source deadlines.
pub const G2_MASTER_DEADLINE: Duration = Duration::from_secs(5);

/// Saturating cap on `search_code`'s `arguments.max_results`.
///
/// `search_code` (`P1-W5-F09`, master-plan §3.2 row 4) merges symbol
/// and text matches; a pathological query such as `.` against a large
/// tree would otherwise produce megabytes of JSON.  The handler clamps
/// the caller's request at this ceiling and emits `tracing::warn!` so
/// the agent can see the clamp in its own logs.
pub const SEARCH_CODE_MAX_RESULTS: usize = 500;

/// Default value for `find_similar`'s `arguments.max_results`.
///
/// Ten covers the common "show me the closest semantic matches"
/// use case while keeping the response envelope under typical
/// context budgets.  Paired with
/// [`FIND_SIMILAR_MAX_RESULTS_CAP`], which caps whatever the caller
/// asks for so a pathological request cannot drain the `LanceDB`
/// query (master-plan §3.2 line 219).
pub const FIND_SIMILAR_DEFAULT_MAX_RESULTS: u64 = 10;

/// Saturating cap on `find_similar`'s `arguments.max_results`.
///
/// `LanceDB`'s flat-scan `nearest_to(...)` cost grows linearly in
/// the table's row count when no ANN index has been created (the
/// small fixture tables this WO exercises do not need an index per
/// the `scope_out` carve-out); we cap the per-call result count
/// at 100 to keep the response envelope finite.
pub const FIND_SIMILAR_MAX_RESULTS_CAP: u64 = 100;

/// Per-call deadline for `find_similar`'s `LanceDB` query path.
///
/// Combined budget for the embedding + `LanceDB` `nearest_to` +
/// `RecordBatch` drain.  Wraps every `.await` in
/// `handle_find_similar` per `.claude/rules/rust-style.md`'s
/// invariant that every `.await` touching IO is wrapped in
/// `tokio::time::timeout` with a named const.
///
/// Five seconds is generous for the test corpus's ≤10 chunks and
/// tight enough to bound a wedged `LanceDB` call.  Production-side
/// tuning is deferred to a follow-up `WO` once the daemon's
/// startup orchestrator wires in the production embedder.
pub const FIND_SIMILAR_QUERY_TIMEOUT: Duration = Duration::from_secs(5);

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by the MCP server skeleton.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum McpError {
    /// Reading from, or writing to, the transport failed.
    #[error("mcp transport i/o error: {0}")]
    Io(#[source] std::io::Error),
    /// A read exceeded [`READ_TIMEOUT_MS`] without delivering a full
    /// line.
    #[error("mcp read timed out after {ms} ms")]
    ReadTimeout {
        /// The configured budget, in milliseconds.
        ms: u64,
    },
    /// A write exceeded [`WRITE_TIMEOUT_MS`].
    #[error("mcp write timed out after {ms} ms")]
    WriteTimeout {
        /// The configured budget, in milliseconds.
        ms: u64,
    },
    /// Serialising a response to JSON failed — only possible if the
    /// handler built a non-serialisable `Value`, which is a bug.
    #[error("failed to serialise mcp response: {0}")]
    Encode(#[source] serde_json::Error),
}

// ── Tool catalog ─────────────────────────────────────────────────────────────

/// A single UCIL tool advertised over MCP.
///
/// The three fields map 1:1 onto the MCP `Tool` object: `name`,
/// `description`, `inputSchema`.  `name` and `description` are static
/// strings because the 22-tool catalog is compiled in; `input_schema`
/// is a [`Value`] because JSON-Schema objects are not `const`-able.
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    /// Unique tool identifier, `snake_case`, matching master-plan §3.2.
    pub name: &'static str,
    /// One-line human-readable purpose of the tool.
    pub description: &'static str,
    /// JSON-Schema object describing accepted input parameters.  Every
    /// descriptor in [`ucil_tools`] carries the four CEQP universal
    /// properties (`reason`, `current_task`, `files_in_context`,
    /// `token_budget`) per master-plan §8.2.
    pub input_schema: Value,
}

impl ToolDescriptor {
    /// Build a descriptor whose `input_schema` is the CEQP universal
    /// envelope — every tool in the Phase-1 catalog uses this helper.
    #[must_use]
    pub fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            input_schema: ceqp_input_schema(),
        }
    }

    /// Render the descriptor as a JSON object matching the MCP `Tool`
    /// shape (`name` / `description` / `inputSchema`).
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
        })
    }
}

/// Build the JSON-Schema object that every Phase-1 tool advertises as
/// its `inputSchema`.
///
/// The four properties are the CEQP universal parameters from
/// master-plan §8.2 — all optional:
///
/// * `reason`: string, **strongly encouraged** — the richer the
///   reason, the more UCIL's bonus-context compiler will proactively
///   include in future responses.
/// * `current_task`: string — one-line summary of the user's overall
///   task.
/// * `files_in_context`: array of strings — files the agent already has
///   loaded; UCIL avoids repeating them.
/// * `token_budget`: integer — advisory token cap; UCIL reports this
///   back in `_meta.token_count` but never enforces it.
///
/// `additionalProperties: true` so per-tool extras (e.g. `target`,
/// `query`) can be layered on in later phases without schema churn.
#[must_use]
pub fn ceqp_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "reason": {
                "type": "string",
                "description": "Why the agent is making this call. Richer reasons unlock richer bonus context. (CEQP universal, optional but strongly encouraged.)"
            },
            "current_task": {
                "type": "string",
                "description": "One-line summary of the user's overall task. (CEQP universal, optional.)"
            },
            "files_in_context": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Files the agent already has open. UCIL avoids repeating them. (CEQP universal, optional.)"
            },
            "token_budget": {
                "type": "integer",
                "minimum": 0,
                "description": "Advisory token cap. UCIL reports `_meta.token_count` but does not enforce. (CEQP universal, optional.)"
            }
        },
        "required": [],
        "additionalProperties": true
    })
}

/// Build the 22-descriptor catalog mandated by master-plan §3.2.
///
/// The order of entries matches the §3.2 table rows (1 → 22).  Every
/// descriptor uses [`ceqp_input_schema`] so the CEQP universals are
/// present on every tool.
#[must_use]
pub fn ucil_tools() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor::new(
            "understand_code",
            "Explain what a file/function/module does, why it exists, its context.",
        ),
        ToolDescriptor::new(
            "find_definition",
            "Go-to-definition with full context (signature, docs, callers).",
        ),
        ToolDescriptor::new(
            "find_references",
            "All references to a symbol, grouped by usage type (call, import, type).",
        ),
        ToolDescriptor::new(
            "search_code",
            "Hybrid search: text + structural + semantic.",
        ),
        ToolDescriptor::new(
            "find_similar",
            "Find code similar to a given snippet or pattern.",
        ),
        ToolDescriptor::new(
            "get_context_for_edit",
            "Optimal context for editing a file/region. Token-budget-aware. Conventions, pitfalls, related code, tests included.",
        ),
        ToolDescriptor::new(
            "get_conventions",
            "Project coding style, naming conventions, patterns in use.",
        ),
        ToolDescriptor::new(
            "get_architecture",
            "High-level architecture overview, module boundaries, data flow.",
        ),
        ToolDescriptor::new(
            "trace_dependencies",
            "Upstream and downstream dependency chains for a file/module/symbol.",
        ),
        ToolDescriptor::new(
            "blast_radius",
            "What would be affected by changing this code?",
        ),
        ToolDescriptor::new(
            "explain_history",
            "Why was this code written this way? PR/issue/ADR context.",
        ),
        ToolDescriptor::new(
            "remember",
            "Store or retrieve agent learnings, decisions, observations.",
        ),
        ToolDescriptor::new(
            "review_changes",
            "Analyze diff/PR against conventions, quality, security, tests, blast radius.",
        ),
        ToolDescriptor::new(
            "check_quality",
            "Run lint + type check + security scan on specified code.",
        ),
        ToolDescriptor::new(
            "run_tests",
            "Execute tests for changed code, return results + coverage.",
        ),
        ToolDescriptor::new(
            "security_scan",
            "Deep security analysis: SAST + SCA + secrets + container scan.",
        ),
        ToolDescriptor::new(
            "lint_code",
            "Language-specific deep linting (ESLint, Ruff, RuboCop, clippy).",
        ),
        ToolDescriptor::new(
            "type_check",
            "Type checking diagnostics via LSP diagnostics bridge.",
        ),
        ToolDescriptor::new(
            "refactor",
            "Safe refactoring with cross-file reference updates via Serena.",
        ),
        ToolDescriptor::new(
            "generate_docs",
            "Generate/update project documentation (architecture, module, API, onboarding).",
        ),
        ToolDescriptor::new(
            "query_database",
            "Schema inspection, migration status, query analysis.",
        ),
        ToolDescriptor::new(
            "check_runtime",
            "Query Sentry/Datadog for errors, traces, performance data.",
        ),
    ]
}

// ── Server ──────────────────────────────────────────────────────────────────

/// Per-branch `find_similar` executor.
///
/// Bundles the dependencies the `handle_find_similar` MCP handler
/// needs to embed a query snippet and run a `LanceDB` `nearest_to`
/// query against the per-branch `code_chunks` table.
///
/// Master-plan §3.2 line 219 freezes the `find_similar` tool's
/// contract ("Find code similar to a given snippet or pattern").
/// Master-plan §18 Phase 2 Week 8 line 1791 frames the deliverable
/// as "Vector search works" — `P2-W8-F08` closes Phase 2 Week 8 and
/// the entire Phase 2 envelope.  Master-plan §12.2 lines 1321-1346
/// freezes the per-branch `code_chunks` table the executor queries.
///
/// The executor wraps three injected collaborators:
///
/// * [`BranchManager`] — resolves the per-branch `vectors/`
///   directory via [`BranchManager::branch_vectors_dir`] (see
///   `WO-0064` line 660-672 for the canonical connect/open pattern).
/// * [`EmbeddingSource`] — `UCIL`-internal trait seam per `DEC-0008`
///   §4 (production `CodeRankEmbeddingSource` from `WO-0059`; tests
///   inject a deterministic `TestEmbeddingSource`).
/// * `default_branch` — fall-back branch name when
///   `arguments.branch` is omitted from the MCP request.
///
/// Production wiring of the executor into the daemon's startup
/// orchestrator is deferred to a follow-up `WO` per the
/// `WO-0066` `scope_out`.
pub struct FindSimilarExecutor {
    branch_manager: Arc<BranchManager>,
    embedding_source: Arc<dyn EmbeddingSource>,
    default_branch: String,
}

impl std::fmt::Debug for FindSimilarExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FindSimilarExecutor")
            .field("branch_manager", &self.branch_manager)
            .field("embedding_source", &self.embedding_source.name())
            .field("default_branch", &self.default_branch)
            .finish()
    }
}

impl FindSimilarExecutor {
    /// Build a new executor from the three injected collaborators.
    ///
    /// `default_branch` is the branch name the
    /// [`McpServer::handle_find_similar`] handler falls back to when
    /// the inbound request omits `arguments.branch`.
    pub fn new(
        branch_manager: Arc<BranchManager>,
        embedding_source: Arc<dyn EmbeddingSource>,
        default_branch: impl Into<String>,
    ) -> Self {
        Self {
            branch_manager,
            embedding_source,
            default_branch: default_branch.into(),
        }
    }

    /// Read-only accessor for the fall-back branch name.  Used by
    /// the handler when `arguments.branch` is omitted.
    #[must_use]
    pub fn default_branch(&self) -> &str {
        &self.default_branch
    }
}

/// UCIL's MCP server over newline-delimited JSON-RPC 2.0.
///
/// Tool *dispatch* is still a Phase-2 concern (G1/G2 fusion) — every
/// `tools/call` handler in this skeleton returns a stub envelope
/// carrying `_meta.not_yet_implemented: true`, per phase-1 invariant
/// #9.  What this type **does** provide, and what the verifier tests,
/// is a working wire protocol: a host agent can `initialize`, list the
/// 22 UCIL tools, and receive structured (stub) responses on
/// `tools/call`.
#[derive(Clone)]
pub struct McpServer {
    /// Advertised tool catalog.  Populated by
    /// [`McpServer::new`] from [`ucil_tools`].
    pub tools: Vec<ToolDescriptor>,
    /// Optional handle onto the bi-temporal knowledge graph populated by
    /// the `P1-W4-F04` tree-sitter → KG ingest pipeline.  When present,
    /// `tools/call` dispatches the `find_definition` tool
    /// (`P1-W4-F05`, master-plan §3.2 row 2), the `get_conventions`
    /// tool (`P1-W4-F10`, master-plan §3.2 row 7), and the
    /// `search_code` tool (`P1-W5-F09`, master-plan §3.2 row 4) to
    /// real handlers that pull from the graph; when absent, each tool
    /// falls through to the `_meta.not_yet_implemented: true` stub path
    /// every other tool still uses (phase-1 invariant #9).
    pub kg: Option<Arc<Mutex<KnowledgeGraph>>>,
    /// Optional G2 source factory.  When present, `handle_search_code`
    /// additively emits a `_meta.g2_fused` field carrying the result
    /// of fanning out to Probe / ripgrep / `LanceDB` and fusing via
    /// [`ucil_core::fuse_g2_rrf`] per master-plan §5.2 lines 447-461 —
    /// `P2-W7-F06`, master-plan §3.2 row 4 G2 lane.  When absent, the
    /// handler returns the legacy `P1-W5-F09` KG+ripgrep merge envelope
    /// byte-identically per `DEC-0015` D1.
    pub g2_sources: Option<Arc<G2SourceFactory>>,
    /// Optional `find_similar` executor (`P2-W8-F08`, master-plan §3.2
    /// row 5 / §18 Phase 2 Week 8 line 1791).  When present,
    /// `tools/call name == "find_similar"` is dispatched to
    /// [`McpServer::handle_find_similar`], which embeds the inbound
    /// snippet, runs a `LanceDB` `nearest_to` query against the
    /// per-branch `code_chunks` table, and returns the top-N
    /// semantically similar code chunks ranked by similarity score.
    /// When absent, the tool falls through to the phase-1
    /// `_meta.not_yet_implemented: true` stub path so phase-1
    /// invariant #9 stays preserved for the unwired case.
    pub find_similar: Option<Arc<FindSimilarExecutor>>,
    /// Optional G4 (Architecture) source list — the dependency-inversion
    /// seam (`DEC-0008` §4) that backs the three architecture-side MCP
    /// tools `get_architecture` (P3-W10-F16), `trace_dependencies`
    /// (P3-W10-F17), and `blast_radius` (P3-W10-F18).  Master-plan §3.2
    /// rows 8/9/10 + §5.4.
    ///
    /// When present, `tools/call name in {get_architecture,
    /// trace_dependencies, blast_radius}` is dispatched to the
    /// corresponding [`McpServer::handle_get_architecture`] /
    /// [`McpServer::handle_trace_dependencies`] /
    /// [`McpServer::handle_blast_radius`] handler, each of which builds
    /// a [`G4Query`], invokes [`crate::g4::execute_g4`], and projects
    /// the resulting [`G4Outcome`] into a tool-specific JSON envelope.
    /// When absent, control falls through to the phase-1
    /// `_meta.not_yet_implemented: true` stub path so phase-1 invariant
    /// #9 stays preserved (and `WO-0010`'s
    /// `test_all_22_tools_registered` selector remains
    /// wire-compatible — the catalog count is unchanged at 22).
    ///
    /// Production wiring (real `CodeGraphContextG4Source` +
    /// `LSPCallHierarchyG4Source` impls) is deferred to a follow-up
    /// production-wiring WO that bundles G4 into the daemon's startup
    /// orchestrator (WO-0072 / WO-0073 deferral conventions).
    pub g4_sources: Option<Arc<Vec<Arc<dyn G4Source>>>>,
    /// Optional G7 (Quality) source list — the dependency-inversion
    /// seam (`DEC-0008` §4) that backs the `check_quality` MCP tool
    /// (`P3-W11-F10`, master-plan §3.2 row 14 + §5.7).
    ///
    /// When present, `tools/call name == "check_quality"` is dispatched
    /// to [`McpServer::handle_check_quality`], which fans out
    /// [`crate::g7::execute_g7`] in parallel with
    /// [`crate::g8::execute_g8`] (G8 sources are paired through
    /// [`Self::g8_sources`]) and projects the merged outcomes into the
    /// `{ issues[], untested_functions[], meta }` wire shape.  When
    /// absent, control falls through to the phase-1
    /// `_meta.not_yet_implemented: true` stub path so phase-1 invariant
    /// #9 stays preserved.
    ///
    /// Production wiring (real `LspDiagnosticsG7Source` /
    /// `EslintG7Source` / `RuffG7Source` / `SemgrepG7Source` impls) is
    /// deferred to a follow-up production-wiring WO per the
    /// WO-0083 / WO-0085 / WO-0089 deferral conventions.
    pub g7_sources: Option<Arc<Vec<Arc<dyn G7Source + Send + Sync>>>>,
    /// Optional G8 (Testing) source list — the dependency-inversion
    /// seam (`DEC-0008` §4) that pairs with [`Self::g7_sources`] under
    /// the `check_quality` MCP tool dispatch path
    /// (`P3-W11-F10`, master-plan §3.2 row 14 + §5.8).  Used by
    /// [`McpServer::handle_check_quality`] to fan
    /// [`crate::g8::execute_g8`] out in parallel with G7.
    ///
    /// Production wiring (real `ConventionG8Source` / `ImportG8Source`
    /// / `KgRelationsG8Source` impls) is deferred to a follow-up
    /// production-wiring WO per the WO-0089 deferral convention.
    pub g8_sources: Option<Arc<Vec<Arc<dyn G8Source + Send + Sync>>>>,
    /// Optional [`DiagnosticsClient`] — the dependency-inversion seam
    /// (`DEC-0008` §4) that backs the `type_check` MCP tool
    /// (`P3-W11-F15`, master-plan §3.2 row 18).
    ///
    /// When present, `tools/call name == "type_check"` is dispatched
    /// to [`McpServer::handle_type_check`], which calls
    /// [`DiagnosticsClient::diagnostics`] for each input file, filters
    /// the returned diagnostics to type errors only, and projects the
    /// surviving rows into the `{ errors[], meta }` wire shape.  When
    /// absent, control falls through to the phase-1
    /// `_meta.not_yet_implemented: true` stub path so phase-1
    /// invariant #9 stays preserved.
    pub diagnostics_client: Option<Arc<DiagnosticsClient>>,
}

impl std::fmt::Debug for McpServer {
    // Manual `Debug` impl: the [`G4Source`] trait does NOT require
    // `Debug` (master-plan §5.4 + `DEC-0008` §4 keep it minimal so
    // production-side adapter impls can stay slim), so an
    // auto-derived `Debug` would fail to compile on the
    // `Arc<Vec<Arc<dyn G4Source>>>` field.  Surface the source-list
    // length instead — the operator-readable substitute the master
    // plan's tracing-spans pattern (§15.2) already advertises.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer")
            .field("tools", &self.tools)
            .field("kg", &self.kg)
            .field("g2_sources", &self.g2_sources)
            .field("find_similar", &self.find_similar)
            .field("g4_sources", &self.g4_sources.as_ref().map(|s| s.len()))
            .field("g7_sources", &self.g7_sources.as_ref().map(|s| s.len()))
            .field("g8_sources", &self.g8_sources.as_ref().map(|s| s.len()))
            .field(
                "diagnostics_client",
                &self.diagnostics_client.as_ref().map(|_| "<attached>"),
            )
            .finish()
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    /// Construct a server whose catalog is the Phase-1 22-tool set.
    ///
    /// The returned server carries no knowledge-graph handle, so every
    /// `tools/call` — including `find_definition` — falls through to the
    /// `_meta.not_yet_implemented: true` stub response required by
    /// phase-1 invariant #9.  This keeps the WO-0010 acceptance
    /// selector `server::test_all_22_tools_registered` wire-compatible
    /// and is the shape every pre-`P1-W4-F05` call-site expects.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: ucil_tools(),
            kg: None,
            g2_sources: None,
            find_similar: None,
            g4_sources: None,
            g7_sources: None,
            g8_sources: None,
            diagnostics_client: None,
        }
    }

    /// Construct a server that routes `find_definition` (`P1-W4-F05`),
    /// `get_conventions` (`P1-W4-F10`), and `search_code` (`P1-W5-F09`)
    /// to their real handlers backed by the supplied knowledge graph.
    ///
    /// The handle is `Arc<Mutex<_>>` so the caller can keep a second
    /// reference (e.g. the ingest pipeline) and mutate the graph
    /// concurrently; each handler takes the lock for the duration of a
    /// single read and releases it before encoding the response.
    /// `search_code` additionally runs an in-process `ripgrep` walk
    /// (see `ucil-daemon::text_search`) with the KG lock released so
    /// the filesystem scan does not block concurrent KG writes.  Every
    /// tool **other** than `find_definition`, `get_conventions`, and
    /// `search_code` still falls through to the stub path — the
    /// 22-tool catalog is unchanged and phase-1 invariant #9 is
    /// preserved for the remaining 19 tools.
    ///
    /// See master-plan §3.2 row 2 (`find_definition` —
    /// go-to-definition with full context), §3.2 row 7
    /// (`get_conventions` — project coding style, naming conventions,
    /// patterns in use), §3.2 row 4 (`search_code` — hybrid search:
    /// text + structural + semantic), and §18 Phase 1 Week 4 line 1751
    /// ("Implement first working tool: `find_definition`").
    #[must_use]
    pub fn with_knowledge_graph(kg: Arc<Mutex<KnowledgeGraph>>) -> Self {
        Self {
            tools: ucil_tools(),
            kg: Some(kg),
            g2_sources: None,
            find_similar: None,
            g4_sources: None,
            g7_sources: None,
            g8_sources: None,
            diagnostics_client: None,
        }
    }

    /// Attach a G2 source factory so `handle_search_code` additively
    /// emits `_meta.g2_fused` — the master-plan §5.2 G2 fan-out lane
    /// that runs Probe / ripgrep / `LanceDB` in parallel and fuses via
    /// [`ucil_core::fuse_g2_rrf`].
    ///
    /// Builder method: chains off [`Self::new`] or
    /// [`Self::with_knowledge_graph`] so callers (the daemon's startup
    /// orchestrator) can attach the factory after the KG handle.
    /// Per `DEC-0015` D1 the legacy `_meta.{tool, source, count, query,
    /// root, symbol_match_count, text_match_count, results}` shape is
    /// preserved byte-identically; the fused output is purely additive
    /// on a new `_meta.g2_fused` field.
    #[must_use]
    pub fn with_g2_sources(mut self, factory: Arc<G2SourceFactory>) -> Self {
        self.g2_sources = Some(factory);
        self
    }

    /// Attach a [`FindSimilarExecutor`] so `tools/call name ==
    /// "find_similar"` is dispatched to
    /// [`McpServer::handle_find_similar`] (`P2-W8-F08`,
    /// master-plan §3.2 row 5 / §18 Phase 2 Week 8 line 1791).
    ///
    /// Builder method: chains off [`Self::new`],
    /// [`Self::with_knowledge_graph`], or
    /// [`Self::with_g2_sources`] so the daemon's startup
    /// orchestrator can attach the executor after wiring the KG /
    /// G2 layers.  When this builder is **not** called, the
    /// `find_similar` tool falls through to the phase-1
    /// `_meta.not_yet_implemented: true` stub path so phase-1
    /// invariant #9 stays preserved (and `WO-0010`'s
    /// `test_all_22_tools_registered` selector remains
    /// wire-compatible).  See `DEC-0008` for the
    /// `UCIL`-internal-trait boundary the executor's
    /// [`EmbeddingSource`] dependency leans on.
    #[must_use]
    pub fn with_find_similar_executor(mut self, executor: Arc<FindSimilarExecutor>) -> Self {
        self.find_similar = Some(executor);
        self
    }

    /// Attach a G4 (Architecture) source list so `tools/call name in
    /// {get_architecture, trace_dependencies, blast_radius}` is
    /// dispatched to the corresponding handler
    /// (`P3-W10-F16` / `P3-W10-F17` / `P3-W10-F18`, master-plan §3.2
    /// rows 8/9/10 + §5.4).
    ///
    /// Builder method: chains off [`Self::new`],
    /// [`Self::with_knowledge_graph`], [`Self::with_g2_sources`], or
    /// [`Self::with_find_similar_executor`] so the daemon's startup
    /// orchestrator can attach the source list after wiring the KG /
    /// G2 / `find_similar` layers.  When this builder is **not** called,
    /// the three architecture tools fall through to the phase-1
    /// `_meta.not_yet_implemented: true` stub path so phase-1
    /// invariant #9 stays preserved (and `WO-0010`'s
    /// `test_all_22_tools_registered` selector remains
    /// wire-compatible — the catalog count is unchanged at 22).
    ///
    /// Per `DEC-0008` §4 the [`G4Source`] trait is UCIL-internal
    /// (the dependency-inversion seam); the daemon's startup
    /// orchestrator will eventually attach a list consisting of the
    /// real `CodeGraphContextG4Source` (calling the F08 plugin via
    /// `tokio::process::Command`) plus `LSPCallHierarchyG4Source`
    /// (calling the P1-W5-F06 LSP bridge) — production wiring is
    /// deferred to a follow-up WO per WO-0072 / WO-0073 production-
    /// wiring `scope_out` conventions.
    #[must_use]
    pub fn with_g4_sources(mut self, sources: Arc<Vec<Arc<dyn G4Source>>>) -> Self {
        self.g4_sources = Some(sources);
        self
    }

    /// Attach a G7 (Quality) source list so `tools/call name ==
    /// "check_quality"` is dispatched to
    /// [`McpServer::handle_check_quality`] (`P3-W11-F10`, master-plan
    /// §3.2 row 14 + §5.7).  Pairs with [`Self::with_g8_sources`] —
    /// the handler runs both fan-outs in parallel via
    /// [`tokio::join!`] before merging the two outcomes.
    ///
    /// Builder method — chains off any prior `with_*` setter so the
    /// daemon's startup orchestrator can attach the source list after
    /// wiring the KG / G2 / `find_similar` / G4 layers.  When this
    /// builder is **not** called, the `check_quality` tool falls
    /// through to the phase-1 `_meta.not_yet_implemented: true` stub
    /// path so phase-1 invariant #9 stays preserved.  Per `DEC-0008`
    /// §4 the [`G7Source`] trait is UCIL-internal (the dependency-
    /// inversion seam); production wiring of real subprocess clients
    /// (`LspDiagnosticsG7Source`, `EslintG7Source`, `RuffG7Source`,
    /// `SemgrepG7Source`) is deferred to a follow-up production-
    /// wiring WO per the WO-0085 backbone deferral convention.
    #[must_use]
    pub fn with_g7_sources(mut self, sources: Arc<Vec<Arc<dyn G7Source + Send + Sync>>>) -> Self {
        self.g7_sources = Some(sources);
        self
    }

    /// Attach a G8 (Testing) source list — the partner of
    /// [`Self::with_g7_sources`] under the `check_quality` MCP tool
    /// dispatch path (`P3-W11-F10`, master-plan §3.2 row 14 + §5.8).
    ///
    /// Builder method — chains off any prior `with_*` setter.  Per
    /// `DEC-0008` §4 the [`G8Source`] trait is UCIL-internal;
    /// production wiring of real impls (`ConventionG8Source`,
    /// `ImportG8Source`, `KgRelationsG8Source`) is deferred to a
    /// follow-up production-wiring WO per the WO-0089 backbone
    /// deferral convention.
    #[must_use]
    pub fn with_g8_sources(mut self, sources: Arc<Vec<Arc<dyn G8Source + Send + Sync>>>) -> Self {
        self.g8_sources = Some(sources);
        self
    }

    /// Attach a [`DiagnosticsClient`] so `tools/call name ==
    /// "type_check"` is dispatched to
    /// [`McpServer::handle_type_check`] (`P3-W11-F15`, master-plan
    /// §3.2 row 18).  The handler uses the client to issue
    /// `textDocument/diagnostic` requests for each input file and
    /// filters the resulting LSP diagnostics to type errors only.
    ///
    /// Builder method — chains off any prior `with_*` setter so the
    /// daemon's startup orchestrator can attach the client after
    /// wiring the LSP-bridge layer.  When this builder is **not**
    /// called, the `type_check` tool falls through to the phase-1
    /// `_meta.not_yet_implemented: true` stub path so phase-1
    /// invariant #9 stays preserved.
    #[must_use]
    pub fn with_diagnostics_client(mut self, client: Arc<DiagnosticsClient>) -> Self {
        self.diagnostics_client = Some(client);
        self
    }

    /// Serve newline-delimited JSON-RPC 2.0 requests from `reader`,
    /// writing responses to `writer`.
    ///
    /// The loop exits cleanly on EOF of `reader`.  Each `.await` on
    /// the transport is wrapped in a named-const
    /// [`tokio::time::timeout`] so a wedged peer cannot hang the
    /// server forever.
    ///
    /// # Errors
    ///
    /// * [`McpError::Io`] — transport read/write failure.
    /// * [`McpError::ReadTimeout`] — inbound read exceeded
    ///   [`READ_TIMEOUT_MS`].
    /// * [`McpError::WriteTimeout`] — outbound write exceeded
    ///   [`WRITE_TIMEOUT_MS`].
    /// * [`McpError::Encode`] — JSON serialisation of the response
    ///   envelope failed (bug).
    pub async fn serve<R, W>(&self, reader: R, mut writer: W) -> Result<(), McpError>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        loop {
            line.clear();
            let read = timeout(READ_TIMEOUT, reader.read_line(&mut line))
                .await
                .map_err(|_| McpError::ReadTimeout {
                    ms: READ_TIMEOUT_MS,
                })?
                .map_err(McpError::Io)?;
            if read == 0 {
                // Clean EOF — peer closed its write half.
                return Ok(());
            }

            let response = self.handle_line(line.trim_end_matches(['\r', '\n'])).await;
            let encoded = serde_json::to_string(&response).map_err(McpError::Encode)?;

            timeout(WRITE_TIMEOUT, async {
                writer.write_all(encoded.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await
            })
            .await
            .map_err(|_| McpError::WriteTimeout {
                ms: WRITE_TIMEOUT_MS,
            })?
            .map_err(McpError::Io)?;
        }
    }

    /// Parse a single inbound line and return the JSON-RPC 2.0 response
    /// envelope.  Pure-data: no IO.  Extracted so the `tools/list`
    /// acceptance test can (indirectly) exercise the dispatcher via
    /// the real [`Self::serve`] loop, while unit tests can call it
    /// without an in-memory duplex.
    async fn handle_line(&self, line: &str) -> Value {
        if line.trim().is_empty() {
            return jsonrpc_error(&Value::Null, -32600, "empty request");
        }
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return jsonrpc_error(&Value::Null, -32700, &format!("Parse error: {e}"));
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        match method.as_str() {
            "initialize" => Self::handle_initialize(&id),
            "tools/list" => self.handle_tools_list(&id),
            "tools/call" => self.handle_tools_call(&id, &params).await,
            other => jsonrpc_error(&id, -32601, &format!("Method not found: {other}")),
        }
    }

    fn handle_initialize(id: &Value) -> Value {
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id.clone(),
            "result": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "ucil-daemon",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        })
    }

    fn handle_tools_list(&self, id: &Value) -> Value {
        let tools: Vec<Value> = self.tools.iter().map(ToolDescriptor::to_json).collect();
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id.clone(),
            "result": { "tools": tools }
        })
    }

    async fn handle_tools_call(&self, id: &Value, params: &Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        if !self.tools.iter().any(|t| t.name == name) {
            return jsonrpc_error(id, -32602, &format!("Unknown tool: {name}"));
        }

        // Route `find_definition` (P1-W4-F05) and `get_conventions`
        // (P1-W4-F10) to their real handlers when a KG handle is
        // attached; every other tool — and both of these when no KG
        // is attached — falls through to the stub path so phase-1
        // invariant #9 is preserved for the remaining 20 tools of the
        // §3.2 catalog.
        if name == "find_definition" {
            if let Some(kg) = self.kg.as_ref() {
                return Self::handle_find_definition(id, params, kg);
            }
        }
        if name == "get_conventions" {
            if let Some(kg) = self.kg.as_ref() {
                return Self::handle_get_conventions(id, params, kg);
            }
        }
        if name == "search_code" {
            if let Some(kg) = self.kg.as_ref() {
                return Self::handle_search_code(id, params, kg, self.g2_sources.as_ref()).await;
            }
        }
        if name == "understand_code" {
            if let Some(kg) = self.kg.as_ref() {
                return crate::understand_code::handle_understand_code(id, params, kg);
            }
        }
        if name == "find_similar" {
            if let Some(exec) = self.find_similar.as_ref() {
                return Self::handle_find_similar(id, params, exec).await;
            }
        }
        // Architecture-side G4 dispatch — `P3-W10-F16` /
        // `P3-W10-F17` / `P3-W10-F18`, master-plan §3.2 rows 8/9/10 +
        // §5.4.  The three handlers fan out through
        // [`crate::g4::execute_g4`] (the F09 orchestrator) and project
        // the resulting `G4Outcome` into a tool-specific JSON envelope.
        // When `g4_sources` is `None`, control falls through to the
        // phase-1 stub path below so phase-1 invariant #9 is preserved.
        if name == "get_architecture" {
            if let Some(srcs) = self.g4_sources.as_ref() {
                return Self::handle_get_architecture(id, params, srcs).await;
            }
        }
        if name == "trace_dependencies" {
            if let Some(srcs) = self.g4_sources.as_ref() {
                return Self::handle_trace_dependencies(id, params, srcs).await;
            }
        }
        if name == "blast_radius" {
            if let Some(srcs) = self.g4_sources.as_ref() {
                return Self::handle_blast_radius(id, params, srcs).await;
            }
        }
        // `review_changes` (P3-W11-F11) — fans out G4
        // (Architecture/blast-radius), G7 (Quality), and G8 (Testing)
        // backbones in parallel and projects the merged outcomes into
        // a unified, severity-ranked `{ findings[], blast_radius,
        // untested_functions[] }` response.  Master-plan §3.2 row 13
        // / §5.4 / §5.7 / §5.8 / §18 Phase 3 Week 11 item 6.  Placed
        // BEFORE the `check_quality` branch since `review_changes`
        // composes G4 + G7 + G8 (a SUPERSET of `check_quality`'s G7
        // + G8 sources).  When `g4_sources` / `g7_sources` /
        // `g8_sources` are ALL `None` (i.e. no production wiring of
        // any of the three backbones), control falls through to the
        // phase-1 stub path below — preserves phase-1 invariant #9.
        if name == "review_changes"
            && (self.g4_sources.is_some() || self.g7_sources.is_some() || self.g8_sources.is_some())
        {
            return self.handle_review_changes(id, params).await;
        }
        // `check_quality` (P3-W11-F10) — fans out G7 (Quality) and
        // G8 (Testing) backbones in parallel and projects the merged
        // outcomes into a `{ issues[], untested_functions[] }`
        // response.  Master-plan §3.2 row 14 / §5.7 / §5.8 / §18 Phase
        // 3 Week 11 item 6.  When `g7_sources` AND `g8_sources` are
        // both `None` (i.e. no production wiring of either backbone),
        // control falls through to the phase-1 stub path below.
        if name == "check_quality" && (self.g7_sources.is_some() || self.g8_sources.is_some()) {
            return self.handle_check_quality(id, params).await;
        }
        // `type_check` (P3-W11-F15) — filters the LSP diagnostics
        // bridge output to type errors only and projects the surviving
        // rows into a `{ errors[] }` response.  Master-plan §3.2 row
        // 18.  When `diagnostics_client` is `None`, control falls
        // through to the phase-1 stub path below.
        if name == "type_check" && self.diagnostics_client.is_some() {
            return self.handle_type_check(id, params).await;
        }

        // Phase-1 invariant #9: every tool handler is a stub that
        // returns `_meta.not_yet_implemented: true`.  Downstream phases
        // will swap this stub for real dispatch into the group fusion
        // layer.
        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id.clone(),
            "result": {
                "_meta": {
                    "not_yet_implemented": true,
                    "tool": name
                },
                "content": [
                    {
                        "type": "text",
                        "text": format!(
                            "tool `{name}` is registered but its handler is not yet implemented (Phase 1 stub)"
                        )
                    }
                ],
                "isError": false
            }
        })
    }

    /// Handle the `find_definition` MCP tool (`P1-W4-F05`,
    /// master-plan §3.2 row 2 / §18 Phase 1 Week 4 line 1751).
    ///
    /// Extracts `arguments.name` (required, string) and
    /// `arguments.file_path` (optional, string) from `params`, queries
    /// the knowledge graph, and returns an MCP `tools/call` envelope
    /// whose `result._meta` carries the structured payload:
    ///
    /// * `tool`: `"find_definition"`.
    /// * `source`: `"tree-sitter+kg"` — advertises the data lineage so
    ///   downstream G1/G2 fusion layers can merge results from other
    ///   source tools (Serena, LSP) without clobbering the KG path.
    /// * `found`: `true` when resolution succeeded, `false` otherwise.
    /// * `file_path`, `start_line`, `signature`, `doc_comment`,
    ///   `parent_module`: the resolved definition projection (present
    ///   only when `found`).
    /// * `callers`: array of `{qualified_name, file_path, start_line}`
    ///   for every immediate caller (i.e. every `calls`-kind edge whose
    ///   `target_id` is the definition's rowid).  Empty vec when the
    ///   definition has no known callers yet.
    ///
    /// The not-found shape returns `isError: false` with
    /// `_meta.found == false` so Claude Code and other MCP hosts render
    /// a graceful "no definition found" response rather than a
    /// JSON-RPC error envelope — matches master-plan §3.2 UX contract.
    ///
    /// Missing or non-string `arguments.name` → JSON-RPC error `-32602`
    /// (invalid params), the standard code for malformed parameters.
    fn handle_find_definition(
        id: &Value,
        params: &Value,
        kg: &Arc<Mutex<KnowledgeGraph>>,
    ) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let Some(name) = args.get("name").and_then(Value::as_str) else {
            return jsonrpc_error(
                id,
                -32602,
                "find_definition: `arguments.name` is required and must be a string",
            );
        };
        let file_scope = args.get("file_path").and_then(Value::as_str);

        match read_find_definition(kg, name, file_scope) {
            Ok(FindDefinitionPayload::Found {
                resolution,
                callers,
            }) => found_response(id, name, &resolution, &callers),
            Ok(FindDefinitionPayload::NotFound) => not_found_response(id, name),
            Err(e) => jsonrpc_error(id, e.code, &e.message),
        }
    }

    /// Handle the `get_conventions` MCP tool (`P1-W4-F10`,
    /// master-plan §3.2 row 7 / §12.1 lines 1172-1182).
    ///
    /// Extracts `arguments.category` (optional, string) from `params`,
    /// queries [`KnowledgeGraph::list_conventions`], and returns an MCP
    /// `tools/call` envelope whose `result._meta` carries the
    /// structured payload:
    ///
    /// * `tool`: `"get_conventions"`.
    /// * `source`: `"kg"` — advertises the data lineage so downstream
    ///   G3 (conventions) fusion layer can merge results from other
    ///   sources (warm-tier sweep, convention-learner) without
    ///   clobbering the cold-table path.
    /// * `count`: length of the returned `conventions` array.
    /// * `category`: echoes the caller's filter (string when present;
    ///   JSON `null` when absent — the "unfiltered" marker).
    /// * `conventions`: array of per-row JSON objects carrying every
    ///   [`ucil_core::Convention`] column.  Empty vec when the table
    ///   is empty or no rows match the filter — the master-plan §3.2
    ///   row 7 "empty list if none yet extracted" contract.
    ///
    /// Empty result is a **non-error** response (`isError: false`,
    /// `content[0].text == "no conventions yet"`); only a missing-or-
    /// wrong-type `category` argument produces a JSON-RPC error.
    /// Non-string `category` → JSON-RPC error `-32602` (invalid
    /// params).
    ///
    /// Missing `category` (key absent) **and** explicit `null` are
    /// both treated as "no filter" — the master-plan spec says
    /// `category` is optional; MCP hosts that omit the key and hosts
    /// that send `null` are both accepted.
    fn handle_get_conventions(
        id: &Value,
        params: &Value,
        kg: &Arc<Mutex<KnowledgeGraph>>,
    ) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        // Extract `arguments.category`:
        // * missing key / explicit JSON null → Option::None (no filter)
        // * JSON string → Some(s)
        // * any other type (number, bool, array, object) → -32602
        let category: Option<String> = match args.get("category") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => {
                return jsonrpc_error(
                    id,
                    -32602,
                    "get_conventions: `arguments.category` must be a string (or omitted/null)",
                );
            }
        };

        match read_conventions(kg, category.as_deref()) {
            Ok(conventions) => {
                get_conventions_found_response(id, category.as_deref(), &conventions)
            }
            Err(e) => jsonrpc_error(id, e.code, &e.message),
        }
    }

    /// Handle the `search_code` MCP tool (`P1-W5-F09`, master-plan
    /// §3.2 row 4 / §18 Phase 1 Week 5 line 1765, DEC-0009).
    ///
    /// Extracts `arguments.query` (required non-empty string),
    /// `arguments.root` (optional string; defaults to the daemon's
    /// current working directory), and `arguments.max_results`
    /// (optional unsigned integer; defaults to
    /// [`SEARCH_CODE_DEFAULT_MAX_RESULTS`] and is saturating-clamped to
    /// [`SEARCH_CODE_MAX_RESULTS`]) from `params`.  Two reads run back
    /// to back:
    ///
    /// 1. **Symbol** — [`ucil_core::KnowledgeGraph::search_entities_by_name`]
    ///    returns the tree-sitter-indexed entities whose `name` or
    ///    `qualified_name` contain `query` as a substring.  The
    ///    KG mutex is held only for the duration of this call and
    ///    released before the filesystem scan begins.
    /// 2. **Text** — [`crate::text_search::text_search`] walks `root`
    ///    with an `ignore::WalkBuilder`, running the query through an
    ///    in-process `grep_regex` matcher against every `.gitignore`-
    ///    permitted file.  Results stream into a bounded vector capped
    ///    at `max_results`.
    ///
    /// The two match lists are merged by [`merge_search_results`]
    /// (pure function, unit-tested separately) using
    /// `(file_path, line_number)` as the dedup key: symbol hits win
    /// collisions and get `source: "both"` when the same line also
    /// showed up in the text walk.  Symbol-only hits carry `source:
    /// "symbol"`, text-only hits carry `source: "text"`.
    ///
    /// The response envelope's `_meta` carries:
    ///
    /// * `tool`: `"search_code"`.
    /// * `source`: `"tree-sitter+ripgrep"` — advertises the dual data
    ///   lineage so downstream G1/G2 fusion layers know both indices
    ///   have been consulted.
    /// * `count`: length of `results`.
    /// * `query`: echoes the caller's query verbatim.
    /// * `root`: absolute-or-as-supplied path string for the search root.
    /// * `symbol_match_count`: raw number of KG hits (pre-merge, pre-cap).
    /// * `text_match_count`: raw number of ripgrep hits (pre-merge,
    ///   post-cap inside the searcher).
    /// * `results`: the merged array of [`SearchCodeResult`] objects.
    /// * `g2_fused` (optional, additive per `DEC-0015` D1): when the
    ///   server was built with [`Self::with_g2_sources`], the handler
    ///   fans out to Probe / ripgrep / `LanceDB` in parallel, fuses
    ///   the per-source ranked outputs via [`ucil_core::fuse_g2_rrf`]
    ///   (weights `Probe×2.0` / `Ripgrep×1.5` / `Lancedb×1.5`,
    ///   `k = 60`) and surfaces the resulting [`G2FusedOutcome`] on
    ///   this field.  When the factory is absent, the field is omitted
    ///   so the legacy `P1-W5-F09` envelope shape is preserved
    ///   byte-identically.
    ///
    /// Empty result is a non-error response (`isError: false`,
    /// `content[0].text == "no matches for …"`).
    ///
    /// # Examples
    ///
    /// A 3-source fan-out at `(util.rs, 10, 20)` where Probe and
    /// `Ripgrep` both rank the location at 1: the merged `g2_fused.hits[0]`
    /// carries `contributing_sources == [Probe, Ripgrep]` (descending
    /// `rrf_weight`), `fused_score == 2.0/61 + 1.5/61 ≈ 0.05738`, and
    /// `snippet` from the `Probe` row (highest weight).  The legacy
    /// `_meta.results` array still carries the `P1-W5-F09` KG+ripgrep
    /// merge unchanged.
    ///
    /// # Error codes
    ///
    /// * `-32602` — missing/non-string `query`, empty `query`,
    ///   non-string `root`, non-u64 `max_results`, nonexistent-or-
    ///   non-directory `root`, or a regex-compilation failure inside
    ///   the text searcher (the caller supplied an invalid regex).
    /// * `-32603` — internal KG or I/O failure (mutex poisoned,
    ///   per-file I/O errors aggregating past the walker).
    #[tracing::instrument(
        name = "ucil.group.search",
        level = "debug",
        skip(id, params, kg, g2_sources)
    )]
    async fn handle_search_code(
        id: &Value,
        params: &Value,
        kg: &Arc<Mutex<KnowledgeGraph>>,
        g2_sources: Option<&Arc<G2SourceFactory>>,
    ) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let SearchCodeArgs {
            query,
            root,
            max_results,
        } = match parse_search_code_args(&args) {
            Ok(a) => a,
            Err(e) => return jsonrpc_error(id, e.code, &e.message),
        };

        // ── Symbol half ─────────────────────────────────────────────
        let symbols = match read_symbol_matches(kg, &query, max_results) {
            Ok(rows) => rows,
            Err(e) => return jsonrpc_error(id, e.code, &e.message),
        };

        // ── Text half (KG mutex already released inside the helper) ──
        let text_hits = match text_search::text_search(&root, &query, max_results) {
            Ok(hits) => hits,
            Err(TextSearchError::BuildMatcher(e)) => {
                return jsonrpc_error(
                    id,
                    -32602,
                    &format!("search_code: invalid query regex: {e}"),
                );
            }
            Err(e) => {
                tracing::error!("search_code: text_search failed: {e}");
                return jsonrpc_error(id, -32603, &format!("search_code: text search failed: {e}"));
            }
        };

        let symbol_match_count = symbols.len();
        let text_match_count = text_hits.len();
        let merged = merge_search_results(&symbols, &text_hits, max_results);
        let mut envelope = search_code_response(
            id,
            &query,
            &root,
            symbol_match_count,
            text_match_count,
            &merged,
        );

        // ── G2 fan-out half (additive per DEC-0015 D1) ──────────────
        if let Some(factory) = g2_sources {
            let fused = run_g2_fan_out(factory, &query, &root, max_results).await;
            let fused_value = serde_json::to_value(&fused).expect("G2FusedOutcome serializes");
            if let Some(meta) = envelope
                .get_mut("result")
                .and_then(|r| r.get_mut("_meta"))
                .and_then(Value::as_object_mut)
            {
                meta.insert("g2_fused".to_owned(), fused_value);
            }
        }

        envelope
    }

    /// Handle the `find_similar` MCP tool (`P2-W8-F08`,
    /// master-plan §3.2 row 5 / §18 Phase 2 Week 8 line 1791).
    ///
    /// Embeds the inbound `arguments.snippet` via the injected
    /// [`EmbeddingSource`], opens the per-branch `code_chunks`
    /// `LanceDB` table (master-plan §12.2 lines 1321-1346) via
    /// [`BranchManager::branch_vectors_dir`] (the same connect /
    /// `open_table` pattern `LancedbChunkIndexer::index_paths`
    /// uses at `lancedb_indexer.rs:660-672` per `WO-0064`), runs
    /// `Table::query().nearest_to(query_vec).limit(N).execute()`
    /// (the `WO-0065` bench precedent at
    /// `crates/ucil-embeddings/benches/vector_query.rs:300-307`),
    /// drains the result `RecordBatchStream`, and projects the
    /// 12-column rows + the synthetic `_distance` column onto
    /// `_meta.hits[]` ranked by similarity.
    ///
    /// Per `DEC-0008` the [`EmbeddingSource`] is a UCIL-internal
    /// trait seam; `LanceDB` is exercised end-to-end (no mocking).
    ///
    /// # Arguments
    ///
    /// * `arguments.snippet` — REQUIRED string.  Missing / non-string
    ///   yields JSON-RPC `-32602` (Invalid params).
    /// * `arguments.max_results` — OPTIONAL `u64`, default
    ///   [`FIND_SIMILAR_DEFAULT_MAX_RESULTS`] (10), clamped to
    ///   `[1, FIND_SIMILAR_MAX_RESULTS_CAP]`.
    /// * `arguments.branch` — OPTIONAL string, defaults to the
    ///   executor's [`FindSimilarExecutor::default_branch`].
    ///
    /// # Result envelope
    ///
    /// `result.isError == false` on the happy path.  `result._meta`
    /// carries `tool == "find_similar"`, `source ==
    /// "lancedb+coderankembed"`, `branch`, `query_dim`,
    /// `hits_count`, and `hits[]` sorted by `similarity_score`
    /// descending.  Each hit projects 8 fields:
    /// `{file_path, start_line, end_line, content, language,
    /// symbol_name, symbol_kind, similarity_score}`.
    ///
    /// # Error envelopes
    ///
    /// Runtime failures surface as `result.isError == true` with
    /// `_meta.error_kind` per master-plan §3.2 UX contract:
    ///
    /// * `embedding_failed` — the injected source returned an
    ///   error.
    /// * `dim_mismatch` — the returned vector's length disagreed
    ///   with the source's declared dimension.
    /// * `branch_not_found` — the per-branch `vectors/` directory
    ///   was missing or `lancedb::connect` failed.
    /// * `table_not_found` — `code_chunks` table did not exist on
    ///   the branch.
    /// * `query_failed` — the `LanceDB` `nearest_to` query or
    ///   stream drain failed.
    /// * `query_timeout` — exceeded
    ///   [`FIND_SIMILAR_QUERY_TIMEOUT`].
    ///
    /// Protocol violations (missing `arguments.snippet`, non-string
    /// `arguments.snippet`, non-string `arguments.branch`) surface
    /// as JSON-RPC `error.code == -32602`.
    #[tracing::instrument(
        name = "ucil.daemon.find_similar",
        level = "debug",
        skip(id, params, executor)
    )]
    async fn handle_find_similar(
        id: &Value,
        params: &Value,
        executor: &FindSimilarExecutor,
    ) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let snippet = match args.get("snippet") {
            Some(Value::String(s)) => s.clone(),
            Some(_) => {
                return jsonrpc_error(
                    id,
                    -32602,
                    "find_similar: `arguments.snippet` must be a string",
                );
            }
            None => {
                return jsonrpc_error(
                    id,
                    -32602,
                    "find_similar: `arguments.snippet` is required and must be a string",
                );
            }
        };

        let max_results = parse_find_similar_max_results(&args);
        let branch = match parse_find_similar_branch(&args, executor.default_branch()) {
            Ok(b) => b,
            Err(e) => return jsonrpc_error(id, e.code, &e.message),
        };

        let span = tracing::info_span!(
            "ucil.daemon.find_similar",
            branch = %branch,
            max_results = max_results,
            query_dim = executor.embedding_source.dim(),
        );
        let _enter = span.enter();

        let Ok(outcome) = timeout(
            FIND_SIMILAR_QUERY_TIMEOUT,
            execute_find_similar(&snippet, &branch, max_results, executor),
        )
        .await
        else {
            tracing::warn!(
                branch = %branch,
                error_kind = "query_timeout",
                "find_similar timed out after {:?}",
                FIND_SIMILAR_QUERY_TIMEOUT,
            );
            return find_similar_error_envelope(
                id,
                &branch,
                executor.embedding_source.dim(),
                "query_timeout",
                &format!(
                    "find_similar exceeded {} ms",
                    FIND_SIMILAR_QUERY_TIMEOUT.as_millis()
                ),
            );
        };

        match outcome {
            Ok(hits) => {
                find_similar_success_envelope(id, &branch, executor.embedding_source.dim(), hits)
            }
            Err(err) => {
                tracing::warn!(
                    branch = %branch,
                    error_kind = err.kind,
                    "find_similar failed: {}",
                    err.message,
                );
                find_similar_error_envelope(
                    id,
                    &branch,
                    executor.embedding_source.dim(),
                    err.kind,
                    &err.message,
                )
            }
        }
    }

    /// Handle the `get_architecture` MCP tool (`P3-W10-F16`,
    /// master-plan §3.2 row 8 / §5.4 lines 483-500 / §18 Phase 3 Week
    /// 10 line 1812).
    ///
    /// Reads MCP `arguments`:
    ///
    /// * `target` (optional, string) — seed module/file/symbol; if
    ///   absent, treated as a project-root scan (empty seed).
    /// * `max_depth` (optional, `u32`, default `3`, clamped to
    ///   `[0, 8]`).
    /// * `max_edges` (optional, `usize`, default `256`, clamped to
    ///   `[0, 4096]`).
    ///
    /// Out-of-range values (e.g. `max_depth > 8`) are silently
    /// clamped to the spec'd cap per master-plan §3.2 + WO-0035
    /// (`search_code` / `max_results`) precedent — clamping is the
    /// canonical UX, NOT an error envelope.
    ///
    /// Constructs a [`G4Query`] from those arguments and invokes
    /// [`crate::g4::execute_g4`] (the F09 architecture orchestrator,
    /// WO-0073) over the supplied [`G4Source`] list.  The unioned
    /// outcome is projected into the MCP `_meta` envelope:
    ///
    /// * `_meta.tool == "get_architecture"`
    /// * `_meta.source == "g4-architecture-fanout"`
    /// * `_meta.modules` — sorted unique source/target node names from
    ///   the unioned edges (the "architecture surface").
    /// * `_meta.edges` — the unified edge list projected as
    ///   `{source, target, edge_kind, contributing_source_ids,
    ///   coupling_weight}`.
    /// * `_meta.master_timed_out` — boolean.
    /// * `_meta.source_results` — per-source `{source_id, status,
    ///   edge_count, elapsed_ms}` tuples.
    /// * `_meta.token_count` — advisory only (NOT enforced — the host
    ///   adapter trims if it must).
    ///
    /// Per `DEC-0008` §4 dependency-inversion seam: the [`G4Source`]
    /// trait is UCIL-internal so production-wiring (real
    /// `CodeGraphContextG4Source` + `LSPCallHierarchyG4Source` impls)
    /// is decoupled from MCP-tool dispatch and lands in a follow-up
    /// production-wiring WO (WO-0072 / WO-0073 deferral conventions).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // params:
    /// // {"name": "get_architecture", "arguments": {
    /// //   "target": "TaskManager", "max_depth": 4, "max_edges": 256
    /// // }}
    /// //
    /// // response (abridged):
    /// // {"result": {"_meta": {
    /// //   "tool": "get_architecture",
    /// //   "source": "g4-architecture-fanout",
    /// //   "modules": ["A", "B", "C", "D"],
    /// //   "edges": [{"source": "A", "target": "B", "edge_kind": "Import",
    /// //              "contributing_source_ids": ["test-g4-source"],
    /// //              "coupling_weight": 0.9}, ...],
    /// //   "master_timed_out": false,
    /// //   "source_results": [{"source_id": "test-g4-source",
    /// //                       "status": "available",
    /// //                       "edge_count": 4, "elapsed_ms": 0}]
    /// // }}}
    /// ```
    #[tracing::instrument(
        name = "ucil.tool.get_architecture",
        level = "debug",
        skip(id, params, sources)
    )]
    async fn handle_get_architecture(
        id: &Value,
        params: &Value,
        sources: &Arc<Vec<Arc<dyn G4Source>>>,
    ) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let target = args
            .get("target")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let max_depth = parse_g4_max_depth(&args, GET_ARCHITECTURE_DEFAULT_MAX_DEPTH);
        let max_edges = parse_g4_max_edges(&args, GET_ARCHITECTURE_DEFAULT_MAX_EDGES);

        let changed_nodes = match target {
            Some(t) if !t.is_empty() => vec![t],
            _ => Vec::new(),
        };
        let query = G4Query {
            changed_nodes,
            max_blast_depth: max_depth,
            max_edges,
        };

        let boxed = boxed_g4_sources(sources);
        let outcome = execute_g4(query.clone(), boxed, G4_MASTER_DEADLINE).await;
        let merged = crate::g4::merge_g4_dependency_union(&outcome.results, &query);

        let modules = collect_modules(&merged.unified_edges);
        let edges_json = project_unified_edges(&merged.unified_edges);
        let source_results_json = project_source_results(&outcome.results);
        let summary = format!(
            "architecture overview: {} modules, {} edges from {} sources",
            modules.len(),
            merged.unified_edges.len(),
            merged.sources_contributing,
        );
        let token_count = estimate_token_count(&summary, merged.unified_edges.len());

        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id.clone(),
            "result": {
                "_meta": {
                    "tool": "get_architecture",
                    "source": "g4-architecture-fanout",
                    "modules": modules,
                    "edges": edges_json,
                    "master_timed_out": outcome.master_timed_out,
                    "source_results": source_results_json,
                    "token_count": token_count,
                },
                "content": [
                    {
                        "type": "text",
                        "text": summary,
                    }
                ],
                "isError": false
            }
        })
    }

    /// Handle the `trace_dependencies` MCP tool (`P3-W10-F17`,
    /// master-plan §3.2 row 9 / §5.4).
    ///
    /// Reads MCP `arguments`:
    ///
    /// * `target` (required, string) — symbol/file whose dependency
    ///   chain to trace.  Missing/non-string → JSON-RPC `-32602`.
    /// * `direction` (optional, string, default `"both"`) — one of
    ///   `"upstream"` / `"downstream"` / `"both"`.
    /// * `max_depth` (optional, `u32`, default `3`, clamped `[0, 8]`).
    ///
    /// Constructs a [`G4Query`] with `changed_nodes = [target]`,
    /// invokes [`crate::g4::execute_g4`], merges via
    /// [`crate::g4::merge_g4_dependency_union`].  Then runs TWO
    /// directional BFS passes over `unioned.unified_edges`:
    ///
    /// * **Downstream** — BFS following edges where `target` is the
    ///   `source` (i.e. who-do-I-depend-on chains spread outward).
    /// * **Upstream** — symmetric BFS over reversed edges (i.e.
    ///   who-depends-on-me chains spread outward).
    ///
    /// Each chain is depth-capped at `max_depth`.  Each output entry
    /// carries `{node, depth}` sorted by `(depth, node)` for
    /// deterministic output.  `direction == "upstream"` omits the
    /// `_meta.downstream` key entirely (and vice versa) so the MCP
    /// response shape mirrors the caller's intent — direction
    /// filtering is load-bearing per `scope_in` #4.
    ///
    /// Master-plan §3.2 row 9 + `DEC-0008` §4 dependency-inversion
    /// seam.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // params:
    /// // {"name": "trace_dependencies", "arguments": {
    /// //   "target": "B", "direction": "both", "max_depth": 3
    /// // }}
    /// //
    /// // response (abridged):
    /// // {"result": {"_meta": {
    /// //   "tool": "trace_dependencies",
    /// //   "source": "g4-architecture-fanout",
    /// //   "target": "B",
    /// //   "direction": "both",
    /// //   "upstream":   [{"node": "A", "depth": 1}, {"node": "E", "depth": 1}],
    /// //   "downstream": [{"node": "C", "depth": 1}, {"node": "D", "depth": 2}],
    /// //   "master_timed_out": false,
    /// //   "source_results": [...]
    /// // }}}
    /// ```
    #[tracing::instrument(
        name = "ucil.tool.trace_dependencies",
        level = "debug",
        skip(id, params, sources)
    )]
    async fn handle_trace_dependencies(
        id: &Value,
        params: &Value,
        sources: &Arc<Vec<Arc<dyn G4Source>>>,
    ) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let Some(target) = args.get("target").and_then(Value::as_str) else {
            return jsonrpc_error(
                id,
                -32602,
                "trace_dependencies: `arguments.target` is required and must be a string",
            );
        };
        let target = target.to_owned();
        let direction = parse_trace_direction(&args);
        let max_depth = parse_g4_max_depth(&args, TRACE_DEPENDENCIES_DEFAULT_MAX_DEPTH);

        let query = G4Query {
            changed_nodes: vec![target.clone()],
            max_blast_depth: max_depth,
            max_edges: TRACE_DEPENDENCIES_DEFAULT_MAX_EDGES,
        };

        let boxed = boxed_g4_sources(sources);
        let outcome = execute_g4(query.clone(), boxed, G4_MASTER_DEADLINE).await;
        let merged = crate::g4::merge_g4_dependency_union(&outcome.results, &query);

        let upstream = (direction != TraceDirection::Downstream).then(|| {
            directional_bfs(
                &merged.unified_edges,
                &target,
                max_depth,
                BfsDirection::Upstream,
            )
        });
        let downstream = (direction != TraceDirection::Upstream).then(|| {
            directional_bfs(
                &merged.unified_edges,
                &target,
                max_depth,
                BfsDirection::Downstream,
            )
        });

        let source_results_json = project_source_results(&outcome.results);
        let upstream_count = upstream.as_ref().map_or(0, Vec::len);
        let downstream_count = downstream.as_ref().map_or(0, Vec::len);
        let summary = format!(
            "trace_dependencies: {upstream_count} upstream + {downstream_count} downstream nodes \
             (target='{target}', max_depth={max_depth})"
        );
        let token_count = estimate_token_count(&summary, upstream_count + downstream_count);

        let mut meta = serde_json::Map::new();
        meta.insert("tool".to_owned(), json!("trace_dependencies"));
        meta.insert("source".to_owned(), json!("g4-architecture-fanout"));
        meta.insert("target".to_owned(), json!(target));
        meta.insert("direction".to_owned(), json!(direction.as_str()));
        if let Some(u) = upstream {
            meta.insert("upstream".to_owned(), project_bfs_chain(&u));
        }
        if let Some(d) = downstream {
            meta.insert("downstream".to_owned(), project_bfs_chain(&d));
        }
        meta.insert(
            "master_timed_out".to_owned(),
            json!(outcome.master_timed_out),
        );
        meta.insert("source_results".to_owned(), source_results_json);
        meta.insert("token_count".to_owned(), json!(token_count));

        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id.clone(),
            "result": {
                "_meta": Value::Object(meta),
                "content": [
                    {
                        "type": "text",
                        "text": summary,
                    }
                ],
                "isError": false
            }
        })
    }

    /// Handle the `blast_radius` MCP tool (`P3-W10-F18`, master-plan
    /// §3.2 row 10 / §5.4 line 495).
    ///
    /// Reads MCP `arguments`:
    ///
    /// * `target` (required, string OR array-of-strings) — single
    ///   string is lifted into a one-element array; an array is taken
    ///   verbatim.
    /// * `max_depth` (optional, `u32`, default `3`, clamped `[0, 8]`).
    /// * `max_edges` (optional, `usize`, default `1024`, clamped
    ///   `[0, 4096]`).
    ///
    /// Constructs a [`G4Query`] with `changed_nodes = target`, invokes
    /// [`crate::g4::execute_g4`], merges via
    /// [`crate::g4::merge_g4_dependency_union`] (which runs the
    /// bidirectional BFS with multiplicative coupling-weight
    /// attenuation per master-plan §5.4 line 495 "BFS from changed
    /// nodes, weight by coupling strength").
    ///
    /// The merger's `unioned.blast_radius` carries every reachable
    /// node (including the seeds themselves at `depth = 0`).  We
    /// project only the truly-impacted nodes (depth > 0) into
    /// `_meta.impacted`, sorted by `(path_weight desc, depth asc, node
    /// asc)` — coupling-weighted ranking is the canonical UX per
    /// master-plan §5.4 + `scope_in` #5.
    ///
    /// Master-plan §3.2 row 10 + `DEC-0008` §4 dependency-inversion
    /// seam.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // params:
    /// // {"name": "blast_radius", "arguments": {
    /// //   "target": "A", "max_depth": 3, "max_edges": 256
    /// // }}
    /// //
    /// // response (abridged):
    /// // {"result": {"_meta": {
    /// //   "tool": "blast_radius",
    /// //   "source": "g4-architecture-fanout",
    /// //   "target": ["A"],
    /// //   "max_depth": 3,
    /// //   "impacted": [
    /// //     {"node": "B", "depth": 1, "path_weight": 0.9},
    /// //     {"node": "D", "depth": 2, "path_weight": 0.72},
    /// //     {"node": "C", "depth": 1, "path_weight": 0.5},
    /// //     {"node": "E", "depth": 2, "path_weight": 0.2}
    /// //   ],
    /// //   "dependency_chain": ["A -> B", "A -> B -> D", ...],
    /// //   "master_timed_out": false,
    /// //   "source_results": [...]
    /// // }}}
    /// ```
    #[tracing::instrument(
        name = "ucil.tool.blast_radius",
        level = "debug",
        skip(id, params, sources)
    )]
    async fn handle_blast_radius(
        id: &Value,
        params: &Value,
        sources: &Arc<Vec<Arc<dyn G4Source>>>,
    ) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let target_seeds = match parse_blast_radius_target(&args) {
            Ok(t) => t,
            Err(e) => return jsonrpc_error(id, e.code, &e.message),
        };
        let max_depth = parse_g4_max_depth(&args, BLAST_RADIUS_DEFAULT_MAX_DEPTH);
        let max_edges = parse_g4_max_edges(&args, BLAST_RADIUS_DEFAULT_MAX_EDGES);

        let query = G4Query {
            changed_nodes: target_seeds.clone(),
            max_blast_depth: max_depth,
            max_edges,
        };

        let boxed = boxed_g4_sources(sources);
        let outcome = execute_g4(query.clone(), boxed, G4_MASTER_DEADLINE).await;
        let merged = crate::g4::merge_g4_dependency_union(&outcome.results, &query);

        let impacted = project_blast_radius_impacted(&merged, &target_seeds);
        let dependency_chain = build_dependency_chains(&merged, &target_seeds);
        let source_results_json = project_source_results(&outcome.results);
        let top_weight = impacted
            .first()
            .and_then(|v| v.get("path_weight").and_then(Value::as_f64))
            .unwrap_or(0.0);
        let top_depth = impacted
            .first()
            .and_then(|v| v.get("depth").and_then(Value::as_u64))
            .unwrap_or(0);
        let summary = format!(
            "blast_radius: {} impacted nodes from {} seed(s), top weight={:.4} (depth={})",
            impacted.len(),
            target_seeds.len(),
            top_weight,
            top_depth,
        );
        let token_count = estimate_token_count(&summary, impacted.len());

        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id.clone(),
            "result": {
                "_meta": {
                    "tool": "blast_radius",
                    "source": "g4-architecture-fanout",
                    "target": target_seeds,
                    "max_depth": max_depth,
                    "impacted": impacted,
                    "dependency_chain": dependency_chain,
                    "master_timed_out": outcome.master_timed_out,
                    "source_results": source_results_json,
                    "token_count": token_count,
                },
                "content": [
                    {
                        "type": "text",
                        "text": summary,
                    }
                ],
                "isError": false
            }
        })
    }

    /// Handle the `check_quality` MCP tool (`P3-W11-F10`,
    /// master-plan §3.2 row 14 + §5.7 + §5.8 + §18 Phase 3 Week 11
    /// item 6).
    ///
    /// Reads MCP `arguments`:
    ///
    /// * `target` (required, string) — file path or symbol the
    ///   quality query is anchored on.  Missing/non-string →
    ///   JSON-RPC `-32602`.
    /// * `reason` (required per CEQP — master-plan §8.2) —
    ///   operator-readable rationale for the call.  Currently
    ///   accepted but not surfaced in the response.
    /// * `current_task` / `files_in_context` / `token_budget` — CEQP
    ///   universal parameters; accepted but not surfaced.
    ///
    /// Builds a [`G7Query`] AND a [`G8Query`] from `target`, then
    /// fans out [`crate::g7::execute_g7`] and [`crate::g8::execute_g8`]
    /// IN PARALLEL via [`tokio::join!`] (master-plan §5.7 + §5.8
    /// "concurrently, then merge"), runs the severity-weighted G7
    /// merge ([`crate::g7::merge_g7_by_severity`]) and the
    /// dedup-by-test-path G8 merge
    /// ([`crate::g8::merge_g8_test_discoveries`]) on the outputs,
    /// and projects the merged data into the canonical
    /// `{ issues[], untested_functions[], meta }` wire shape.
    ///
    /// The handler emits a `tracing::Span::current().record("target",
    /// ...)` field after argument parsing so the §15.2
    /// `ucil.tool.check_quality` span carries the operator-readable
    /// target verbatim.
    ///
    /// On empty G7 / G8 source lists the handler returns the same
    /// envelope shape with empty `issues[]` / `untested_functions[]`
    /// arrays — never panics.
    #[tracing::instrument(name = "ucil.tool.check_quality")]
    #[allow(clippy::too_many_lines)]
    async fn handle_check_quality(&self, id: &Value, params: &Value) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let Some(target) = args.get("target").and_then(Value::as_str) else {
            return jsonrpc_error(
                id,
                -32602,
                "check_quality: `arguments.target` is required and must be a string",
            );
        };
        let target = target.to_owned();
        if args.get("reason").and_then(Value::as_str).is_none() {
            return jsonrpc_error(
                id,
                -32602,
                "check_quality: `arguments.reason` is required and must be a string (CEQP)",
            );
        }
        tracing::Span::current().record("target", target.as_str());

        let g7_query = G7Query {
            target: target.clone(),
            categories: vec![],
        };
        let g8_query = G8Query {
            changed_files: vec![PathBuf::from(target.clone())],
        };

        let g7_boxed = boxed_g7_sources(self.g7_sources.as_ref());
        let g8_boxed = boxed_g8_sources(self.g8_sources.as_ref());

        let start = std::time::Instant::now();
        // PARALLEL fan-out via `tokio::join!(execute_g7(...), execute_g8(...))`
        // — G7 (Quality) and G8 (Testing) run concurrently per
        // master-plan §5.7 + §5.8 + §6.1 line 606.  Sequential awaits
        // would compound the per-group masters and exceed the §6.1
        // 600 ms p50 latency budget for the `check_quality` MCP tool —
        // see `scope_in` #1 + the SA6 wall-clock canary in the frozen
        // test.
        let (g7_outcome, g8_outcome) = tokio::join!(
            execute_g7(g7_boxed, g7_query, G7_DEFAULT_MASTER_DEADLINE),
            execute_g8(g8_query, g8_boxed, G8_DEFAULT_MASTER_DEADLINE),
        );
        let wall_elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        let all_g7_issues: Vec<_> = g7_outcome
            .results
            .iter()
            .filter(|r| matches!(r.status, crate::g7::G7SourceStatus::Available))
            .flat_map(|r| r.issues.clone())
            .collect();
        let merged_issues = merge_g7_by_severity(&all_g7_issues);
        let merged_candidates = merge_g8_test_discoveries(&g8_outcome);

        let issues_json: Vec<Value> = merged_issues
            .iter()
            .map(|m| {
                json!({
                    "severity": m.severity.as_str(),
                    "category": m.category,
                    "file": m.file_path,
                    "line": m.line_start,
                    "fix_suggestion": m.fix_suggestions.first(),
                    "source_tools": m.source_tools,
                    "message": m.message,
                    "rule_ids": m.rule_ids,
                })
            })
            .collect();
        let untested_json: Vec<Value> = merged_candidates
            .iter()
            .map(|m| {
                let methods_found_by: Vec<&'static str> = m
                    .methods_found_by
                    .iter()
                    .map(|method| match method {
                        crate::g8::TestDiscoveryMethod::Convention => "convention",
                        crate::g8::TestDiscoveryMethod::Import => "import",
                        crate::g8::TestDiscoveryMethod::KgRelations => "kg_relations",
                    })
                    .collect();
                let source_path: Option<String> = m
                    .source_paths
                    .first()
                    .map(|p| p.to_string_lossy().into_owned());
                let source_paths: Vec<String> = m
                    .source_paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                json!({
                    "test_path": m.test_path.to_string_lossy(),
                    "source_path": source_path,
                    "source_paths": source_paths,
                    "methods_found_by": methods_found_by,
                    "max_confidence": m.max_confidence,
                })
            })
            .collect();

        let master_timed_out = g7_outcome.master_timed_out || g8_outcome.master_timed_out;
        let payload = json!({
            "issues": issues_json,
            "untested_functions": untested_json,
            "meta": {
                "master_timed_out": master_timed_out,
                "wall_elapsed_ms": wall_elapsed_ms,
            }
        });
        let summary = format!(
            "check_quality: {} issues, {} untested functions for `{}`",
            merged_issues.len(),
            merged_candidates.len(),
            target,
        );

        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id.clone(),
            "result": {
                "_meta": {
                    "tool": "check_quality",
                    "source": "g7+g8-parallel",
                    "master_timed_out": master_timed_out,
                    "wall_elapsed_ms": wall_elapsed_ms,
                },
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string(&payload)
                            .unwrap_or_else(|_| summary.clone())
                    }
                ],
                "isError": false
            }
        })
    }

    /// Handle the `review_changes` MCP tool (`P3-W11-F11`,
    /// master-plan §3.2 row 13 + §5.4 + §5.7 + §5.8 + §18 Phase 3
    /// Week 11 item 6).
    ///
    /// Reads MCP `arguments`:
    ///
    /// * `changed_files` (required, array of strings) — list of
    ///   file paths the diff/PR touched.  Missing/non-array →
    ///   JSON-RPC `-32602`.
    /// * `reason` (required per CEQP — master-plan §8.2) —
    ///   operator-readable rationale for the call.  Currently
    ///   accepted but not surfaced in the response.
    /// * `current_task` / `files_in_context` / `token_budget` — CEQP
    ///   universal parameters; accepted but not surfaced.
    ///
    /// Builds a [`G4Query`], a [`G7Query`] AND a [`G8Query`] from the
    /// `changed_files` list, then fans out [`crate::g4::execute_g4`]
    /// + [`crate::g7::execute_g7`] + [`crate::g8::execute_g8`] IN
    /// PARALLEL via [`tokio::join!`] (master-plan §5.4 + §5.7 + §5.8
    /// "concurrently, then merge"), runs the architecture
    /// dependency-union merge ([`crate::g4::merge_g4_dependency_union`]),
    /// the severity-weighted G7 merge ([`crate::g7::merge_g7_by_severity`])
    /// and the dedup-by-test-path G8 merge
    /// ([`crate::g8::merge_g8_test_discoveries`]) on the outputs,
    /// and projects the merged data into the canonical
    /// `{ findings[], blast_radius, untested_functions[], meta }`
    /// wire shape.
    ///
    /// `findings[]` is the union of the merged G7 quality issues
    /// (each carrying its native severity + category and
    /// `source_group: "quality"`) and the merged G4 blast-radius
    /// nodes (each projected as a `Medium`-severity finding with
    /// `category: "blast_radius"` and `source_group: "architecture"`),
    /// sorted descending by severity weight (Critical=4, High=3,
    /// Medium=2, Low=1, Info=0; ties broken by `source_group` then
    /// by `file`).
    ///
    /// The handler emits
    /// `tracing::Span::current().record("changed_files_count", …)`
    /// AFTER argument parsing so the §15.2
    /// `ucil.tool.review_changes` span carries the parsed file
    /// count without inflating field cardinality with the file
    /// names themselves.
    ///
    /// On empty G4 / G7 / G8 source lists the handler returns the
    /// same envelope shape with empty `findings[]` /
    /// `untested_functions[]` / `blast_radius.impacted[]` arrays —
    /// never panics.
    ///
    /// # Panics
    ///
    /// This function never panics on caller-supplied inputs.  The
    /// `serde_json::to_string(&payload)` call uses
    /// `unwrap_or_else(|_| summary.clone())` to fall back to the
    /// textual summary on any (theoretical) serialization failure
    /// per WO-0090 §executor lesson on production-side
    /// degraded-textual-fallback for `tools/call` JSON-RPC
    /// envelopes.
    #[tracing::instrument(name = "ucil.tool.review_changes")]
    #[allow(clippy::too_many_lines)]
    async fn handle_review_changes(&self, id: &Value, params: &Value) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let Some(files_arr) = args.get("changed_files").and_then(Value::as_array) else {
            return jsonrpc_error(
                id,
                -32602,
                "review_changes: `arguments.changed_files` is required and must be an array of strings",
            );
        };
        let changed_files: Vec<String> = files_arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        if changed_files.is_empty() {
            return jsonrpc_error(
                id,
                -32602,
                "review_changes: `arguments.changed_files` must contain at least one string entry",
            );
        }
        if args.get("reason").and_then(Value::as_str).is_none() {
            return jsonrpc_error(
                id,
                -32602,
                "review_changes: `arguments.reason` is required and must be a string (CEQP)",
            );
        }
        // Span field cardinality canary: record only the count, not
        // the file names themselves, per master-plan §15.2 line 1519
        // ("bounded fields") + WO-0085 §planner lesson on numeric-cast
        // tracing fields.
        tracing::Span::current().record(
            "changed_files_count",
            i64::try_from(changed_files.len()).unwrap_or(i64::MAX),
        );

        // Build per-G-source queries from the same `changed_files`
        // list — G4 takes the file-path strings as its
        // `changed_nodes` seed list (the BFS traversal is symbolic
        // so node-name and file-path overlap when callers seed with
        // file paths), G7 takes the FIRST file as its `target`
        // anchor (per scope_in #1.b — `handle_check_quality` shape),
        // and G8 takes the FULL list as its `changed_files`
        // dedup-by-test-path query.
        let g4_query = G4Query {
            changed_nodes: changed_files.clone(),
            max_blast_depth: BLAST_RADIUS_DEFAULT_MAX_DEPTH,
            max_edges: BLAST_RADIUS_DEFAULT_MAX_EDGES,
        };
        // G7 anchor: first file in the changed-files list — G7
        // sources are SYMBOL-scoped so we anchor on the first file
        // and let per-source impls walk outward.  Documented inline
        // per scope_in #1.b.
        let g7_query = G7Query {
            target: changed_files.first().cloned().unwrap_or_default(),
            categories: vec![],
        };
        let g8_query = G8Query {
            changed_files: changed_files.iter().map(PathBuf::from).collect(),
        };

        let g4_boxed = self
            .g4_sources
            .as_ref()
            .map(boxed_g4_sources)
            .unwrap_or_default();
        let g7_boxed = boxed_g7_sources(self.g7_sources.as_ref());
        let g8_boxed = boxed_g8_sources(self.g8_sources.as_ref());

        let start = std::time::Instant::now();
        // PARALLEL fan-out via 3-arity `tokio::join!` —
        // [`crate::g4::execute_g4`] (Architecture),
        // [`crate::g7::execute_g7`] (Quality), and
        // [`crate::g8::execute_g8`] (Testing) run concurrently per
        // master-plan §5.4 + §5.7 + §5.8 + §6.1 line 606.
        // Sequential awaits would compound the per-group masters
        // (G4=12 s, G7=5.5 s, G8=5 s) past the §6.1 wall-clock
        // budget for the `review_changes` MCP tool — see scope_in
        // #1.c + the SA7 wall-clock canary in the frozen test.
        let (g4_outcome, g7_outcome, g8_outcome) = tokio::join!(
            execute_g4(g4_query.clone(), g4_boxed, G4_MASTER_DEADLINE),
            execute_g7(g7_boxed, g7_query, G7_DEFAULT_MASTER_DEADLINE),
            execute_g8(g8_query, g8_boxed, G8_DEFAULT_MASTER_DEADLINE),
        );
        let wall_elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        let merged_g4 = crate::g4::merge_g4_dependency_union(&g4_outcome.results, &g4_query);
        let all_g7_issues: Vec<_> = g7_outcome
            .results
            .iter()
            .filter(|r| matches!(r.status, crate::g7::G7SourceStatus::Available))
            .flat_map(|r| r.issues.clone())
            .collect();
        let merged_issues = merge_g7_by_severity(&all_g7_issues);
        let merged_candidates = merge_g8_test_discoveries(&g8_outcome);

        // Project the merged G4 blast-radius nodes into the
        // `blast_radius` sub-object — REUSE the
        // `project_blast_radius_impacted` + `build_dependency_chains`
        // helpers from `handle_blast_radius` per scope_in #1.g.
        let impacted = project_blast_radius_impacted(&merged_g4, &changed_files);
        let dependency_chain = build_dependency_chains(&merged_g4, &changed_files);

        // Project the merged G7 quality issues into the unified
        // `findings[]` array, retaining native severity/category and
        // tagging `source_group: "quality"`.
        let mut findings: Vec<Value> = merged_issues
            .iter()
            .map(|m| {
                json!({
                    "severity": m.severity.as_str(),
                    "category": m.category,
                    "source_group": "quality",
                    "file": m.file_path,
                    "line": m.line_start,
                    "message": m.message,
                })
            })
            .collect();

        // Project the merged G4 blast-radius nodes (depth > 0,
        // seed-excluded — see `project_blast_radius_impacted`) into
        // additional `findings[]` entries with `severity:
        // "medium"`, `category: "blast_radius"`, `source_group:
        // "architecture"`.  The G4 entries are appended AFTER the
        // G7 entries, so the unsorted concat puts a Medium G7 issue
        // (when present) in front of any Critical-or-higher G7
        // issue ONLY if `merge_g7_by_severity` had already sorted
        // by severity.  The M4 mutation (sort_by → no-op compare)
        // exploits the absence of an explicit severity-rank sort
        // here to flip SA2.
        for entry in &impacted {
            let node = entry.get("node").and_then(Value::as_str).unwrap_or("");
            findings.push(json!({
                "severity": "medium",
                "category": "blast_radius",
                "source_group": "architecture",
                "file": node,
                "line": Value::Null,
                "message": format!(
                    "Blast-radius node `{node}` impacted via dependency chain"
                ),
            }));
        }

        // Severity-weight ladder — descending sort key.  Critical=4,
        // High=3, Medium=2, Low=1, Info=0, unknown=-1.  Documented
        // at point-of-use per scope_in #1.f.
        const fn severity_weight(s: &str) -> i8 {
            match s.as_bytes() {
                b"critical" => 4,
                b"high" => 3,
                b"medium" => 2,
                b"low" => 1,
                b"info" => 0,
                _ => -1,
            }
        }
        // ── M4 mutation site ────────────────────────────────────
        // The verifier's M4 mutation flips this comparator to a
        // no-op `Ordering::Equal` — under the mutation the unsorted
        // concat order is preserved.  Because `merge_g7_by_severity`
        // does NOT sort by severity (its merge groups by
        // `(file, line, category)` keys with alphabetical tie-break),
        // the unsorted concat can land a `medium` Ruff finding at
        // index 0 ahead of the `critical` rust-analyzer finding,
        // failing the `findings[0].severity == "critical"` SA2
        // canary.
        findings.sort_by(|a, b| {
            let wa = severity_weight(a.get("severity").and_then(Value::as_str).unwrap_or(""));
            let wb = severity_weight(b.get("severity").and_then(Value::as_str).unwrap_or(""));
            wb.cmp(&wa)
                .then_with(|| {
                    let sga = a.get("source_group").and_then(Value::as_str).unwrap_or("");
                    let sgb = b.get("source_group").and_then(Value::as_str).unwrap_or("");
                    sga.cmp(sgb)
                })
                .then_with(|| {
                    let fa = a.get("file").and_then(Value::as_str).unwrap_or("");
                    let fb = b.get("file").and_then(Value::as_str).unwrap_or("");
                    fa.cmp(fb)
                })
        });

        let untested_json: Vec<Value> = merged_candidates
            .iter()
            .map(|m| {
                let methods_found_by: Vec<&'static str> = m
                    .methods_found_by
                    .iter()
                    .map(|method| match method {
                        crate::g8::TestDiscoveryMethod::Convention => "convention",
                        crate::g8::TestDiscoveryMethod::Import => "import",
                        crate::g8::TestDiscoveryMethod::KgRelations => "kg_relations",
                    })
                    .collect();
                let source_path: Option<String> = m
                    .source_paths
                    .first()
                    .map(|p| p.to_string_lossy().into_owned());
                json!({
                    "test_path": m.test_path.to_string_lossy(),
                    "source_path": source_path,
                    "methods_found_by": methods_found_by,
                    "max_confidence": m.max_confidence,
                })
            })
            .collect();

        let master_timed_out = g4_outcome.master_timed_out
            || g7_outcome.master_timed_out
            || g8_outcome.master_timed_out;
        let payload = json!({
            "findings": findings,
            "blast_radius": {
                "impacted": impacted,
                "dependency_chain": dependency_chain,
            },
            "untested_functions": untested_json,
            "meta": {
                "master_timed_out": master_timed_out,
                "wall_elapsed_ms": wall_elapsed_ms,
            }
        });
        let summary = format!(
            "review_changes: {} findings, {} untested functions, {} blast-radius nodes for {} changed files",
            findings.len(),
            merged_candidates.len(),
            impacted.len(),
            changed_files.len(),
        );

        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id.clone(),
            "result": {
                "_meta": {
                    "tool": "review_changes",
                    "source": "g4+g7+g8-parallel",
                    "master_timed_out": master_timed_out,
                    "wall_elapsed_ms": wall_elapsed_ms,
                },
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string(&payload)
                            .unwrap_or_else(|_| summary.clone())
                    }
                ],
                "isError": false
            }
        })
    }

    /// Handle the `type_check` MCP tool (`P3-W11-F15`, master-plan
    /// §3.2 row 18).
    ///
    /// Reads MCP `arguments`:
    ///
    /// * `files` (required, array of strings) — file paths to type-
    ///   check.  Missing/non-array → JSON-RPC `-32602`.
    /// * `reason` (required per CEQP — master-plan §8.2) —
    ///   operator-readable rationale.  Accepted but not surfaced.
    /// * `current_task` / `files_in_context` / `token_budget` — CEQP
    ///   universal parameters; accepted but not surfaced.
    ///
    /// For each input file, the handler resolves its [`Language`]
    /// from the path extension, converts the path to a `file://`
    /// URL, and calls [`DiagnosticsClient::diagnostics`] to fetch
    /// the LSP `textDocument/diagnostic` results.  Each returned
    /// diagnostic is run through [`is_type_error_diagnostic`]: only
    /// rows with `severity == DiagnosticSeverity::ERROR` AND a
    /// type-checker source (`rust-analyzer`, `pyright`, `tsserver`,
    /// `typescript`, `mypy`) OR a language-specific type-error code
    /// prefix (Rust `E…`, TypeScript `TS2xxx`/`TS7xxx`, Python
    /// `report…`) survive the filter.  Lint warnings (Warning
    /// severity) and lint errors with non-type-checker sources (e.g.
    /// `clippy`) are dropped.
    ///
    /// Files lacking an LSP-supported language extension are
    /// counted in `meta.files_skipped` rather than emitted as an
    /// error, so a polyglot caller can hand the handler a mixed
    /// list without per-file pre-filtering.
    ///
    /// The handler emits a `tracing::Span::current().record("files",
    /// ...)` field after argument parsing so the §15.2
    /// `ucil.tool.type_check` span carries the operator-readable
    /// file count.
    #[tracing::instrument(name = "ucil.tool.type_check")]
    #[allow(clippy::too_many_lines)]
    async fn handle_type_check(&self, id: &Value, params: &Value) -> Value {
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let Some(files_arr) = args.get("files").and_then(Value::as_array) else {
            return jsonrpc_error(
                id,
                -32602,
                "type_check: `arguments.files` is required and must be an array of strings",
            );
        };
        let files: Vec<String> = files_arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        if args.get("reason").and_then(Value::as_str).is_none() {
            return jsonrpc_error(
                id,
                -32602,
                "type_check: `arguments.reason` is required and must be a string (CEQP)",
            );
        }
        tracing::Span::current().record("files", files.len());

        let Some(client) = self.diagnostics_client.as_ref() else {
            return jsonrpc_error(
                id,
                -32603,
                "type_check: no DiagnosticsClient attached to McpServer",
            );
        };

        let mut errors: Vec<Value> = Vec::new();
        let mut files_checked: usize = 0;
        let mut files_skipped: usize = 0;

        for file in &files {
            let Some(language) = language_from_path(file) else {
                files_skipped += 1;
                continue;
            };
            let Some(url) = path_to_file_url(file) else {
                files_skipped += 1;
                continue;
            };
            files_checked += 1;
            let Ok(raw) = client.diagnostics(url).await else {
                continue;
            };
            // Filter to type errors only — Error severity AND a
            // type-checker source / type-error code prefix.  The M3
            // mutation drops this filter (`.filter(|_| true)`); when
            // the filter is dropped, lint warnings + non-type-error
            // sources leak through and the SA1 length assertion
            // panics.
            let kept: Vec<lsp_types::Diagnostic> =
                raw.into_iter().filter(is_type_error_diagnostic).collect();
            for diag in kept {
                let line = diag.range.start.line + 1;
                let severity_str = match diag.severity {
                    Some(lsp_types::DiagnosticSeverity::ERROR) => "error",
                    Some(lsp_types::DiagnosticSeverity::WARNING) => "warning",
                    Some(lsp_types::DiagnosticSeverity::INFORMATION) => "information",
                    _ => "hint",
                };
                errors.push(json!({
                    "file": file,
                    "line": line,
                    "message": diag.message,
                    "language": language_to_str(language),
                    "severity": severity_str,
                    "source": diag.source,
                }));
            }
        }

        let payload = json!({
            "errors": errors,
            "meta": {
                "files_checked": files_checked,
                "files_skipped": files_skipped,
            }
        });
        let summary = format!(
            "type_check: {} type error(s) across {} file(s) ({} skipped)",
            payload
                .pointer("/errors")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            files_checked,
            files_skipped,
        );

        json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id.clone(),
            "result": {
                "_meta": {
                    "tool": "type_check",
                    "source": "lsp-diagnostics-bridge",
                    "files_checked": files_checked,
                    "files_skipped": files_skipped,
                },
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string(&payload)
                            .unwrap_or_else(|_| summary.clone())
                    }
                ],
                "isError": false
            }
        })
    }
}

// ── G4 architecture handler helpers (P3-W10-F16/F17/F18) ────────────────────

/// Default `max_depth` for the `get_architecture` tool envelope.
///
/// Master-plan §3.2 row 8 + `scope_in` #3 of WO-0083: "optional
/// `max_depth` (u32, default 3, clamped to [0, 8])".  Kept as a
/// named const so the verifier's M1 mutation contract reads cleanly
/// (the constant is not the mutation target — the projection is).
const GET_ARCHITECTURE_DEFAULT_MAX_DEPTH: u32 = 3;

/// Default `max_edges` for the `get_architecture` tool envelope.
const GET_ARCHITECTURE_DEFAULT_MAX_EDGES: usize = 256;

/// Default `max_depth` for the `trace_dependencies` tool envelope.
const TRACE_DEPENDENCIES_DEFAULT_MAX_DEPTH: u32 = 3;

/// Default `max_edges` for the `trace_dependencies` tool envelope —
/// held as a named const since the spec'd UX is "no client-side
/// `max_edges` knob, just trace the chain"; we still cap so a
/// pathological `unified_edges` set cannot blow up the BFS.
const TRACE_DEPENDENCIES_DEFAULT_MAX_EDGES: usize = 1024;

/// Default `max_depth` for the `blast_radius` tool envelope.
const BLAST_RADIUS_DEFAULT_MAX_DEPTH: u32 = 3;

/// Default `max_edges` for the `blast_radius` tool envelope.
const BLAST_RADIUS_DEFAULT_MAX_EDGES: usize = 1024;

/// Saturating cap on `max_depth` for any G4-backed MCP tool envelope.
///
/// Master-plan §3.2 + `scope_in` #3 of WO-0083: "clamped to [0, 8]".
const G4_MAX_DEPTH_CAP: u32 = 8;

/// Saturating cap on `max_edges` for any G4-backed MCP tool envelope.
///
/// Master-plan §3.2 + `scope_in` #3 of WO-0083: "clamped to [0, 4096]".
const G4_MAX_EDGES_CAP: usize = 4096;

/// Top-K cap on the `dependency_chain` array surfaced by the
/// `blast_radius` tool envelope per `scope_in` #5 ("top-K=10 paths from
/// each seed to a leaf in the BFS tree").
const BLAST_RADIUS_DEPENDENCY_CHAIN_TOP_K: usize = 10;

/// Direction filter for the `trace_dependencies` MCP tool
/// (`scope_in` #4 — `direction in {"upstream", "downstream", "both"}`,
/// default `"both"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraceDirection {
    Upstream,
    Downstream,
    Both,
}

impl TraceDirection {
    /// Echo-back string the MCP envelope advertises as
    /// `_meta.direction`.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
            Self::Both => "both",
        }
    }
}

/// Direction selector for [`directional_bfs`] — `Upstream` follows
/// edges in reverse (`target -> source`), `Downstream` follows edges
/// forward (`source -> target`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BfsDirection {
    Upstream,
    Downstream,
}

/// Adapter that re-implements [`G4Source`] for an `Arc<dyn G4Source>`
/// so a borrow-only handle can be coerced into the `Vec<Box<dyn
/// G4Source + Send + Sync + 'static>>` shape that
/// [`crate::g4::execute_g4`] consumes by value.
///
/// The adapter is a thin delegate — every call forwards to the
/// inner `Arc`'s impl — so it does not introduce any extra latency
/// or allocation beyond a single `Arc::clone`.  Per `DEC-0008` §4 the
/// `G4Source` trait is UCIL-internal so this pattern does not
/// shadow any external wire format.
struct G4SourceArcAdapter {
    inner: Arc<dyn G4Source>,
}

#[async_trait::async_trait]
impl G4Source for G4SourceArcAdapter {
    fn source_id(&self) -> &str {
        self.inner.source_id()
    }

    async fn execute(&self, query: &G4Query) -> G4SourceOutput {
        self.inner.execute(query).await
    }
}

/// Build an owned `Vec<Box<dyn G4Source ...>>` from a borrowed
/// `Arc<Vec<Arc<dyn G4Source>>>` so it can be passed by value to
/// [`crate::g4::execute_g4`] without taking ownership of the source
/// list (the daemon's startup orchestrator keeps a long-lived
/// `Arc<Vec<...>>` and per-call dispatches share it via `Arc::clone`).
fn boxed_g4_sources(
    sources: &Arc<Vec<Arc<dyn G4Source>>>,
) -> Vec<Box<dyn G4Source + Send + Sync + 'static>> {
    sources
        .iter()
        .map(|s| {
            Box::new(G4SourceArcAdapter {
                inner: Arc::clone(s),
            }) as Box<dyn G4Source + Send + Sync + 'static>
        })
        .collect()
}

// ── G7 / G8 / DiagnosticsClient helper plumbing (P3-W11-F10/F15) ─────────────
//
// Same shape as the `G4SourceArcAdapter` + `boxed_g4_sources` pair
// above — `crate::g7::execute_g7` and `crate::g8::execute_g8` both
// consume `Vec<Box<dyn G7Source + Send + Sync + 'static>>` /
// `Vec<Box<dyn G8Source + Send + Sync + 'static>>` by value, but the
// daemon's startup orchestrator (and the F10 frozen test) keep a
// long-lived `Arc<Vec<Arc<dyn G7Source ...>>>` so per-call dispatches
// share the source list via `Arc::clone`.

/// Adapter delegating to an `Arc<dyn G7Source + Send + Sync>` so the
/// `boxed_g7_sources` helper can produce the
/// `Vec<Box<dyn G7Source + Send + Sync + 'static>>` shape that
/// [`crate::g7::execute_g7`] consumes by value.
///
/// Per `DEC-0008` §4 the [`G7Source`] trait is UCIL-internal so this
/// pattern does not shadow any external wire format.
struct G7SourceArcAdapter {
    inner: Arc<dyn G7Source + Send + Sync>,
}

#[async_trait::async_trait]
impl G7Source for G7SourceArcAdapter {
    fn source_id(&self) -> &str {
        self.inner.source_id()
    }

    async fn execute(&self, query: &G7Query) -> crate::g7::G7SourceOutput {
        self.inner.execute(query).await
    }
}

/// Build an owned `Vec<Box<dyn G7Source ...>>` from an optional
/// `Arc<Vec<Arc<dyn G7Source ...>>>`; an absent list yields an empty
/// vec so [`crate::g7::execute_g7`] still runs end-to-end (with zero
/// per-source results) on a no-G7-attached `McpServer`.
fn boxed_g7_sources(
    sources: Option<&Arc<Vec<Arc<dyn G7Source + Send + Sync>>>>,
) -> Vec<Box<dyn G7Source + Send + Sync + 'static>> {
    sources.map_or_else(Vec::new, |srcs| {
        srcs.iter()
            .map(|s| {
                Box::new(G7SourceArcAdapter {
                    inner: Arc::clone(s),
                }) as Box<dyn G7Source + Send + Sync + 'static>
            })
            .collect()
    })
}

/// Adapter delegating to an `Arc<dyn G8Source + Send + Sync>` —
/// mirrors [`G7SourceArcAdapter`] for the G8 (Testing) lane.
struct G8SourceArcAdapter {
    inner: Arc<dyn G8Source + Send + Sync>,
}

#[async_trait::async_trait]
impl G8Source for G8SourceArcAdapter {
    fn source_id(&self) -> String {
        self.inner.source_id()
    }

    fn method(&self) -> crate::g8::TestDiscoveryMethod {
        self.inner.method()
    }

    async fn execute(&self, query: &G8Query) -> Result<Vec<crate::g8::G8TestCandidate>, String> {
        self.inner.execute(query).await
    }
}

/// Build an owned `Vec<Box<dyn G8Source ...>>` from an optional
/// `Arc<Vec<Arc<dyn G8Source ...>>>`; an absent list yields an empty
/// vec so [`crate::g8::execute_g8`] still runs end-to-end on a
/// no-G8-attached `McpServer`.
fn boxed_g8_sources(
    sources: Option<&Arc<Vec<Arc<dyn G8Source + Send + Sync>>>>,
) -> Vec<Box<dyn G8Source + Send + Sync + 'static>> {
    sources.map_or_else(Vec::new, |srcs| {
        srcs.iter()
            .map(|s| {
                Box::new(G8SourceArcAdapter {
                    inner: Arc::clone(s),
                }) as Box<dyn G8Source + Send + Sync + 'static>
            })
            .collect()
    })
}

/// Resolve a [`Language`] from a file path's extension.
///
/// Returns `None` for files whose extension does not map to one of
/// the LSP-supported languages — `handle_type_check` counts these in
/// `meta.files_skipped` rather than emitting an error.
fn language_from_path(path: &str) -> Option<Language> {
    let ext = std::path::Path::new(path).extension()?.to_str()?;
    match ext.to_ascii_lowercase().as_str() {
        "rs" => Some(Language::Rust),
        "py" | "pyi" => Some(Language::Python),
        "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" => Some(Language::TypeScript),
        "go" => Some(Language::Go),
        "java" => Some(Language::Java),
        "c" | "h" => Some(Language::C),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(Language::Cpp),
        _ => None,
    }
}

/// Project a [`Language`] back to its lowercase wire-string for the
/// `type_check` response's `errors[].language` field.
const fn language_to_str(language: Language) -> &'static str {
    match language {
        Language::Python => "python",
        Language::Rust => "rust",
        Language::TypeScript => "typescript",
        Language::Go => "go",
        Language::Java => "java",
        Language::C => "c",
        Language::Cpp => "cpp",
    }
}

/// Convert an absolute filesystem path to an `lsp_types::Url` for
/// dispatch through [`DiagnosticsClient::diagnostics`].
///
/// Returns `None` for paths that `Url::from_file_path` rejects (e.g.
/// non-absolute paths on platforms that require absolute URI bases);
/// `handle_type_check` counts these in `meta.files_skipped` rather
/// than emitting an error.
fn path_to_file_url(path: &str) -> Option<lsp_types::Url> {
    lsp_types::Url::from_file_path(path).ok()
}

/// Predicate filter that keeps an LSP [`lsp_types::Diagnostic`] iff
/// it is a TYPE error per master-plan §3.2 row 18 spec.
///
/// A diagnostic is a type error when:
///
/// 1. `severity == DiagnosticSeverity::ERROR` — Warning / Information
///    / Hint diagnostics never qualify regardless of source/code.
/// 2. EITHER `source` is in the type-checker allow-list
///    (`rust-analyzer`, `pyright`, `tsserver`, `typescript`, `mypy`)
///    OR `code` matches one of the language-specific type-error code
///    prefixes (Rust `E…`, TypeScript `TS2xxx`/`TS7xxx`, Python
///    `report…`).
///
/// Lint errors (Error severity but source = `clippy` / `eslint` /
/// `ruff` / `semgrep`) are filtered OUT — they belong to the
/// `check_quality` (G7) lane.
///
/// The M3 mutation contract drops this filter entirely
/// (`.filter(|_| true)`); when the filter is dropped lint errors +
/// warnings leak into the `errors[]` array and the SA1 length
/// assertion in `test_type_check_tool` panics.
fn is_type_error_diagnostic(diag: &lsp_types::Diagnostic) -> bool {
    if diag.severity != Some(lsp_types::DiagnosticSeverity::ERROR) {
        return false;
    }
    let source_ok = matches!(
        diag.source.as_deref(),
        Some("rust-analyzer" | "pyright" | "tsserver" | "typescript" | "mypy"),
    );
    let code_ok = matches!(&diag.code, Some(lsp_types::NumberOrString::String(s))
        if s.starts_with('E')
            || s.starts_with("TS2")
            || s.starts_with("TS7")
            || s.starts_with("report"));
    source_ok || code_ok
}

/// Parse `arguments.max_depth` as a `u32` clamped to `[0, 8]`.  Falls
/// back to `default` if the argument is missing or malformed.  Out-of-
/// range values clamp silently per `scope_in` #24 (clamping is the
/// canonical UX, NOT an error envelope).
fn parse_g4_max_depth(args: &Value, default: u32) -> u32 {
    let raw = args
        .get("max_depth")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(default);
    raw.min(G4_MAX_DEPTH_CAP)
}

/// Parse `arguments.max_edges` as a `usize` clamped to `[0, 4096]`.
/// Same fall-back + clamp semantics as [`parse_g4_max_depth`].
fn parse_g4_max_edges(args: &Value, default: usize) -> usize {
    let raw = args
        .get("max_edges")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(default);
    raw.min(G4_MAX_EDGES_CAP)
}

/// Parse `arguments.direction` for the `trace_dependencies` tool
/// envelope.  Missing/unrecognised → [`TraceDirection::Both`] per
/// `scope_in` #4 default.  Recognised case-insensitively.
fn parse_trace_direction(args: &Value) -> TraceDirection {
    args.get("direction")
        .and_then(Value::as_str)
        .map_or(TraceDirection::Both, |s| {
            match s.to_ascii_lowercase().as_str() {
                "upstream" => TraceDirection::Upstream,
                "downstream" => TraceDirection::Downstream,
                _ => TraceDirection::Both,
            }
        })
}

/// Internal error shape for the `blast_radius` argument-parsing
/// stage — for the JSON-RPC `error` envelope (protocol violations
/// only).
#[derive(Debug)]
struct BlastRadiusArgError {
    code: i64,
    message: String,
}

/// Parse `arguments.target` for the `blast_radius` tool.  Accepts
/// EITHER a single string OR an array of strings; a single string is
/// lifted into a one-element `Vec<String>`.  Missing or malformed →
/// JSON-RPC `-32602`.
fn parse_blast_radius_target(args: &Value) -> Result<Vec<String>, BlastRadiusArgError> {
    match args.get("target") {
        Some(Value::String(s)) if !s.is_empty() => Ok(vec![s.clone()]),
        Some(Value::Array(arr)) if !arr.is_empty() => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                match v.as_str() {
                    Some(s) if !s.is_empty() => out.push(s.to_owned()),
                    _ => {
                        return Err(BlastRadiusArgError {
                            code: -32602,
                            message:
                                "blast_radius: every entry of `arguments.target` array must be a non-empty string"
                                    .to_owned(),
                        });
                    }
                }
            }
            Ok(out)
        }
        Some(Value::String(_) | Value::Array(_)) => Err(BlastRadiusArgError {
            code: -32602,
            message:
                "blast_radius: `arguments.target` must be a non-empty string or non-empty array of strings"
                    .to_owned(),
        }),
        Some(_) => Err(BlastRadiusArgError {
            code: -32602,
            message:
                "blast_radius: `arguments.target` must be a string or array of strings"
                    .to_owned(),
        }),
        None => Err(BlastRadiusArgError {
            code: -32602,
            message: "blast_radius: `arguments.target` is required".to_owned(),
        }),
    }
}

/// Collect the sorted-unique node names from the unified edge list —
/// the architecture surface advertised in `_meta.modules` of the
/// `get_architecture` tool envelope.
fn collect_modules(unified_edges: &[G4UnifiedEdge]) -> Vec<String> {
    let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for ue in unified_edges {
        set.insert(ue.edge.source.clone());
        set.insert(ue.edge.target.clone());
    }
    set.into_iter().collect()
}

/// Project the unioned edge list into the JSON shape advertised on
/// `_meta.edges` of the `get_architecture` tool envelope.
fn project_unified_edges(unified_edges: &[G4UnifiedEdge]) -> Vec<Value> {
    unified_edges
        .iter()
        .map(|ue| {
            json!({
                "source": ue.edge.source,
                "target": ue.edge.target,
                "edge_kind": format_edge_kind(&ue.edge.edge_kind),
                "contributing_source_ids": ue.contributing_source_ids,
                "coupling_weight": ue.edge.coupling_weight,
            })
        })
        .collect()
}

/// Format a [`crate::g4::G4EdgeKind`] as a stable JSON-friendly
/// string.  Mirrors the `serde_plain`-style discriminant naming the
/// rest of the daemon's MCP envelopes use.
fn format_edge_kind(kind: &crate::g4::G4EdgeKind) -> String {
    match kind {
        crate::g4::G4EdgeKind::Import => "Import".to_owned(),
        crate::g4::G4EdgeKind::Call => "Call".to_owned(),
        crate::g4::G4EdgeKind::Inherits => "Inherits".to_owned(),
        crate::g4::G4EdgeKind::Implements => "Implements".to_owned(),
        crate::g4::G4EdgeKind::Other(s) => format!("Other({s})"),
    }
}

/// Project the per-source [`G4SourceOutput`] list into the JSON shape
/// advertised on `_meta.source_results` of every G4-backed MCP tool
/// envelope.
fn project_source_results(results: &[G4SourceOutput]) -> Value {
    let arr: Vec<Value> = results
        .iter()
        .map(|out| {
            json!({
                "source_id": out.source_id,
                "status": format_g4_source_status(out.status),
                "edge_count": out.edges.len(),
                "elapsed_ms": out.elapsed_ms,
            })
        })
        .collect();
    Value::Array(arr)
}

/// Format a [`G4SourceStatus`] as a lowercase JSON-friendly string.
const fn format_g4_source_status(status: G4SourceStatus) -> &'static str {
    match status {
        G4SourceStatus::Available => "available",
        G4SourceStatus::TimedOut => "timed_out",
        G4SourceStatus::Errored => "errored",
    }
}

/// Estimate the token count of an MCP envelope as a coarse
/// `summary.len() / 4 + per_entry * 8` heuristic.  Advisory only
/// per `scope_out` #10 — the host adapter trims if it must.
const fn estimate_token_count(summary: &str, entries: usize) -> usize {
    summary.len() / 4 + entries.saturating_mul(8)
}

/// Bidirectional-or-directional BFS state entry — `(node, depth)`.
type BfsChainEntry = (String, u32);

/// Run a depth-capped directional BFS over `unified_edges` starting
/// at `seed`, following edges either forward (`Downstream` —
/// `source -> target`) or in reverse (`Upstream` — `target ->
/// source`).
///
/// The seed itself is excluded from the output (only its dependents
/// or dependencies appear).  Output is sorted by `(depth, node)` for
/// deterministic downstream consumption.
fn directional_bfs(
    unified_edges: &[G4UnifiedEdge],
    seed: &str,
    max_depth: u32,
    direction: BfsDirection,
) -> Vec<BfsChainEntry> {
    use std::collections::{BTreeMap, VecDeque};

    // Build a directional adjacency map keyed by current-node →
    // neighbour-node.  `Downstream` follows `source → target`;
    // `Upstream` follows `target → source` (i.e. reversed edges).
    let mut adj: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ue in unified_edges {
        match direction {
            BfsDirection::Downstream => adj
                .entry(ue.edge.source.clone())
                .or_default()
                .push(ue.edge.target.clone()),
            BfsDirection::Upstream => adj
                .entry(ue.edge.target.clone())
                .or_default()
                .push(ue.edge.source.clone()),
        }
    }

    let mut visited: BTreeMap<String, u32> = BTreeMap::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    visited.insert(seed.to_owned(), 0);
    queue.push_back(seed.to_owned());

    while let Some(current) = queue.pop_front() {
        let current_depth = visited[&current];
        let next_depth = current_depth + 1;
        if next_depth > max_depth {
            continue;
        }
        if let Some(neighbours) = adj.get(&current) {
            for nb in neighbours {
                if visited.contains_key(nb) {
                    continue;
                }
                visited.insert(nb.clone(), next_depth);
                queue.push_back(nb.clone());
            }
        }
    }

    visited.remove(seed);
    let mut out: Vec<BfsChainEntry> = visited.into_iter().collect();
    out.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
    out
}

/// Project a directional BFS chain into the JSON array advertised
/// on `_meta.upstream` / `_meta.downstream` of the
/// `trace_dependencies` tool envelope.
fn project_bfs_chain(chain: &[BfsChainEntry]) -> Value {
    let arr: Vec<Value> = chain
        .iter()
        .map(|(node, depth)| {
            json!({
                "node": node,
                "depth": depth,
            })
        })
        .collect();
    Value::Array(arr)
}

/// Project the `unioned.blast_radius` BFS output into the JSON array
/// advertised on `_meta.impacted` of the `blast_radius` tool envelope.
///
/// Excludes the seed nodes themselves (depth = 0) so `_meta.impacted`
/// carries only the truly-impacted neighbour set.  Sorted by
/// `(path_weight desc, depth asc, node asc)` per `scope_in` #5 +
/// master-plan §5.4 line 495 coupling-weighted ranking.
fn project_blast_radius_impacted(merged: &G4UnionOutcome, seeds: &[String]) -> Vec<Value> {
    let seed_set: std::collections::BTreeSet<&String> = seeds.iter().collect();
    let mut entries: Vec<&crate::g4::G4BlastRadiusEntry> = merged
        .blast_radius
        .iter()
        .filter(|e| e.depth > 0 && !seed_set.contains(&e.node))
        .collect();
    // ── M3 mutation site ─────────────────────────────────────────
    // The verifier's M3 mutation flips the `b.cumulative_coupling`
    // / `a.cumulative_coupling` order (descending → ascending).
    // Under the mutation, the lowest-weighted node lands at index 0
    // → the SA5 descending-sort invariant
    // (`impacted[0].path_weight >= impacted[1].path_weight`)
    // panics.
    entries.sort_by(|a, b| {
        b.cumulative_coupling
            .partial_cmp(&a.cumulative_coupling)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.depth.cmp(&b.depth))
            .then_with(|| a.node.cmp(&b.node))
    });
    entries
        .into_iter()
        .map(|e| {
            json!({
                "node": e.node,
                "depth": e.depth,
                "path_weight": e.cumulative_coupling,
            })
        })
        .collect()
}

/// Build the `_meta.dependency_chain` array advertised by the
/// `blast_radius` tool envelope: top-K `seed -> hop1 -> ... -> leaf`
/// strings derived from `unioned.blast_radius`'s
/// `contributing_edges` parent-pointer trail.
fn build_dependency_chains(merged: &G4UnionOutcome, seeds: &[String]) -> Vec<String> {
    use std::collections::BTreeMap;

    // Index every blast-radius entry by node name so we can walk the
    // parent-pointer chain.
    let by_node: BTreeMap<&String, &crate::g4::G4BlastRadiusEntry> =
        merged.blast_radius.iter().map(|e| (&e.node, e)).collect();
    let seed_set: std::collections::BTreeSet<&String> = seeds.iter().collect();

    // Sort entries by (cumulative_coupling desc, depth asc, node
    // asc) so the top-K projection picks the most-coupled chains
    // first.
    let mut ranked: Vec<&crate::g4::G4BlastRadiusEntry> = merged
        .blast_radius
        .iter()
        .filter(|e| e.depth > 0 && !seed_set.contains(&e.node))
        .collect();
    ranked.sort_by(|a, b| {
        b.cumulative_coupling
            .partial_cmp(&a.cumulative_coupling)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.depth.cmp(&b.depth))
            .then_with(|| a.node.cmp(&b.node))
    });

    let mut chains: Vec<String> = Vec::new();
    for entry in ranked.into_iter().take(BLAST_RADIUS_DEPENDENCY_CHAIN_TOP_K) {
        let mut path: Vec<String> = vec![entry.node.clone()];
        let mut current: &crate::g4::G4BlastRadiusEntry = entry;
        while let Some(parent_pair) = current.contributing_edges.first() {
            // The contributing_edges entry carries the
            // `(source, target)` of the unified edge that brought
            // `current` into the radius.  The OTHER end of that edge
            // is `current`'s BFS parent — pick whichever endpoint is
            // not `current.node`.
            let parent_name = if parent_pair.0 == current.node {
                &parent_pair.1
            } else {
                &parent_pair.0
            };
            path.push(parent_name.clone());
            if seed_set.contains(parent_name) {
                break;
            }
            // Continue walking up the BFS tree by indexing the
            // parent's blast-radius entry.
            match by_node.get(parent_name) {
                Some(p) => current = p,
                None => break,
            }
        }
        path.reverse();
        chains.push(path.join(" -> "));
    }
    chains
}

// ── find_similar helpers (P2-W8-F08) ────────────────────────────────────────

/// Projection of a single `LanceDB` row onto the JSON shape the
/// `find_similar` MCP envelope advertises in `_meta.hits[]`.
///
/// The 12-column `code_chunks` table (master-plan §12.2 lines
/// 1321-1346) is reduced to 8 user-facing fields plus a derived
/// `similarity_score`.  The raw `embedding` / `token_count` /
/// `file_hash` / `indexed_at` columns are storage-internal and
/// dropped from the projection.
#[derive(Debug)]
struct FindSimilarHit {
    file_path: String,
    start_line: i32,
    end_line: i32,
    content: String,
    language: String,
    symbol_name: Option<String>,
    symbol_kind: Option<String>,
    similarity_score: f64,
}

/// Internal failure shape for [`execute_find_similar`].  Threads the
/// human-readable error message + the stable `error_kind` discriminant
/// up to the handler so the envelope encoder can build a typed
/// `result.isError = true` response without re-classifying the cause.
#[derive(Debug)]
struct FindSimilarError {
    kind: &'static str,
    message: String,
}

/// Parse `arguments.max_results` for `find_similar`.
///
/// Optional `u64`, defaults to [`FIND_SIMILAR_DEFAULT_MAX_RESULTS`].
/// Clamped to `[1, FIND_SIMILAR_MAX_RESULTS_CAP]` so a pathological
/// request cannot drain the `LanceDB` query; out-of-range or
/// non-numeric values fall back to the default.
fn parse_find_similar_max_results(args: &Value) -> u64 {
    let raw = args
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(FIND_SIMILAR_DEFAULT_MAX_RESULTS);
    raw.clamp(1, FIND_SIMILAR_MAX_RESULTS_CAP)
}

/// Internal error shape for the `find_similar` argument-parsing
/// stage — for the JSON-RPC `error` envelope (protocol violations
/// only; runtime failures use [`FindSimilarError`] +
/// `result.isError` instead).
#[derive(Debug)]
struct FindSimilarArgError {
    code: i64,
    message: String,
}

/// Parse `arguments.branch` for `find_similar`.
///
/// Optional string, defaults to the executor's
/// [`FindSimilarExecutor::default_branch`].  Non-string yields
/// JSON-RPC `-32602`.
fn parse_find_similar_branch(
    args: &Value,
    default_branch: &str,
) -> Result<String, FindSimilarArgError> {
    match args.get("branch") {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Null) | None => Ok(default_branch.to_owned()),
        Some(_) => Err(FindSimilarArgError {
            code: -32602,
            message: "find_similar: `arguments.branch` must be a string".to_owned(),
        }),
    }
}

/// Embed the snippet, open the per-branch `code_chunks` table, run
/// `nearest_to(...).limit(N).execute()`, drain the
/// `RecordBatchStream`, and project rows onto [`FindSimilarHit`].
///
/// All `.await`s are bounded by [`FIND_SIMILAR_QUERY_TIMEOUT`] via
/// the outer `tokio::time::timeout` wrapper in
/// [`McpServer::handle_find_similar`] per language-agnostic
/// invariant "every async IO await is wrapped in
/// `tokio::time::timeout`".
async fn execute_find_similar(
    snippet: &str,
    branch: &str,
    max_results: u64,
    executor: &FindSimilarExecutor,
) -> Result<Vec<FindSimilarHit>, FindSimilarError> {
    use futures::TryStreamExt as _;
    use lancedb::query::{ExecutableQuery as _, QueryBase as _};

    // ── (a) embed snippet ──────────────────────────────────────────
    let query_vec = match executor.embedding_source.embed(snippet).await {
        Ok(v) => v,
        Err(e) => {
            return Err(FindSimilarError {
                kind: "embedding_failed",
                message: format!("embedding failed: {e}"),
            });
        }
    };

    // ── (b) dim mismatch guard ─────────────────────────────────────
    let dim = executor.embedding_source.dim();
    if query_vec.len() != dim {
        return Err(FindSimilarError {
            kind: "dim_mismatch",
            message: format!(
                "embedding source returned vector of length {} but declared dim {}",
                query_vec.len(),
                dim,
            ),
        });
    }

    // ── (c) resolve per-branch vectors dir ─────────────────────────
    let vectors_dir = executor.branch_manager.branch_vectors_dir(branch);
    if !vectors_dir.exists() {
        return Err(FindSimilarError {
            kind: "branch_not_found",
            message: format!(
                "branch `{branch}` has no vectors directory at {}",
                vectors_dir.display(),
            ),
        });
    }
    let Some(uri) = vectors_dir.to_str() else {
        return Err(FindSimilarError {
            kind: "branch_not_found",
            message: format!(
                "branch `{branch}` vectors path is not valid UTF-8: {}",
                vectors_dir.display(),
            ),
        });
    };

    // ── (d) connect ────────────────────────────────────────────────
    let conn = match lancedb::connect(uri).execute().await {
        Ok(c) => c,
        Err(e) => {
            return Err(FindSimilarError {
                kind: "branch_not_found",
                message: format!("lancedb connect failed for `{branch}`: {e}"),
            });
        }
    };

    // ── (e) open code_chunks table ─────────────────────────────────
    let table = match conn.open_table("code_chunks").execute().await {
        Ok(t) => t,
        Err(e) => {
            return Err(FindSimilarError {
                kind: "table_not_found",
                message: format!("code_chunks table missing for `{branch}`: {e}"),
            });
        }
    };

    // ── (f) nearest_to(query_vec).limit(N).execute() ───────────────
    let limit = usize::try_from(max_results).unwrap_or(usize::MAX);
    let stream = match table
        .query()
        .nearest_to(query_vec.as_slice())
        .map_err(|e| FindSimilarError {
            kind: "query_failed",
            message: format!("nearest_to failed: {e}"),
        })?
        .limit(limit)
        .execute()
        .await
    {
        Ok(s) => s,
        Err(e) => {
            return Err(FindSimilarError {
                kind: "query_failed",
                message: format!("execute failed: {e}"),
            });
        }
    };

    let batches: Vec<arrow_array::RecordBatch> = match stream.try_collect().await {
        Ok(b) => b,
        Err(e) => {
            return Err(FindSimilarError {
                kind: "query_failed",
                message: format!("RecordBatchStream drain failed: {e}"),
            });
        }
    };

    // ── (g) project rows onto FindSimilarHit ───────────────────────
    let mut hits = project_find_similar_rows(&batches);

    // ── (h) defence-in-depth sort by similarity DESC ───────────────
    // Per AC13 / AC35: LanceDB's `nearest_to` results are typically
    // already distance-ordered, but this is not contractually
    // guaranteed across versions; the explicit sort here is
    // load-bearing — `WO-0066` mutation `M3` proves SA5 fails when
    // this line is removed.
    hits.sort_by(|a, b| {
        b.similarity_score
            .partial_cmp(&a.similarity_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(hits)
}

/// Project a vector of `RecordBatch`es from the `LanceDB`
/// `nearest_to` stream onto [`FindSimilarHit`].
///
/// The `_distance` column `LanceDB` appends to `nearest_to`
/// results is `Float32` per the upstream impl convention.  The
/// synthetic `similarity_score = 1.0 / (1.0 + f64::from(_distance))`
/// is monotonic descending in distance — top hit (smallest
/// distance) has the largest score.
fn project_find_similar_rows(batches: &[arrow_array::RecordBatch]) -> Vec<FindSimilarHit> {
    use arrow_array::Array as _;

    let mut hits: Vec<FindSimilarHit> = Vec::new();
    for batch in batches {
        let file_paths = batch
            .column_by_name("file_path")
            .map(arrow_array::cast::AsArray::as_string::<i32>);
        let start_lines = batch
            .column_by_name("start_line")
            .and_then(|a| a.as_any().downcast_ref::<arrow_array::Int32Array>());
        let end_lines = batch
            .column_by_name("end_line")
            .and_then(|a| a.as_any().downcast_ref::<arrow_array::Int32Array>());
        let contents = batch
            .column_by_name("content")
            .map(arrow_array::cast::AsArray::as_string::<i32>);
        let languages = batch
            .column_by_name("language")
            .map(arrow_array::cast::AsArray::as_string::<i32>);
        let symbol_names = batch
            .column_by_name("symbol_name")
            .map(arrow_array::cast::AsArray::as_string::<i32>);
        let symbol_kinds = batch
            .column_by_name("symbol_kind")
            .map(arrow_array::cast::AsArray::as_string::<i32>);
        let distances = batch
            .column_by_name("_distance")
            .and_then(|a| a.as_any().downcast_ref::<arrow_array::Float32Array>());

        let (
            Some(file_paths),
            Some(start_lines),
            Some(end_lines),
            Some(contents),
            Some(languages),
            Some(symbol_names),
            Some(symbol_kinds),
            Some(distances),
        ) = (
            file_paths,
            start_lines,
            end_lines,
            contents,
            languages,
            symbol_names,
            symbol_kinds,
            distances,
        )
        else {
            tracing::warn!(
                "find_similar: skipping RecordBatch with unexpected schema (missing required column or _distance)"
            );
            continue;
        };

        for i in 0..batch.num_rows() {
            let dist_f32 = distances.value(i);
            let similarity_score = 1.0_f64 / (1.0_f64 + f64::from(dist_f32));
            let symbol_name_val = if symbol_names.is_null(i) {
                None
            } else {
                let raw = symbol_names.value(i).to_owned();
                if raw.is_empty() {
                    None
                } else {
                    Some(raw)
                }
            };
            let symbol_kind_val = if symbol_kinds.is_null(i) {
                None
            } else {
                let raw = symbol_kinds.value(i).to_owned();
                if raw.is_empty() {
                    None
                } else {
                    Some(raw)
                }
            };
            hits.push(FindSimilarHit {
                file_path: file_paths.value(i).to_owned(),
                start_line: start_lines.value(i),
                end_line: end_lines.value(i),
                content: contents.value(i).to_owned(),
                language: languages.value(i).to_owned(),
                symbol_name: symbol_name_val,
                symbol_kind: symbol_kind_val,
                similarity_score,
            });
        }
    }
    hits
}

/// Build the JSON-RPC happy-path response envelope for
/// `find_similar`.  `result.isError = false`; `_meta` carries the
/// per-master-plan §3.2 row 5 contract fields.
fn find_similar_success_envelope(
    id: &Value,
    branch: &str,
    query_dim: usize,
    hits: Vec<FindSimilarHit>,
) -> Value {
    let hits_count = hits.len();
    let hits_json: Vec<Value> = hits
        .into_iter()
        .map(|h| {
            json!({
                "file_path": h.file_path,
                "start_line": h.start_line,
                "end_line": h.end_line,
                "content": h.content,
                "language": h.language,
                "symbol_name": h.symbol_name,
                "symbol_kind": h.symbol_kind,
                "similarity_score": h.similarity_score,
            })
        })
        .collect();
    let text = format!(
        "found {hits_count} similar code chunk{} on branch `{branch}`",
        if hits_count == 1 { "" } else { "s" },
    );
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "result": {
            "_meta": {
                "tool": "find_similar",
                "source": "lancedb+coderankembed",
                "branch": branch,
                "query_dim": query_dim,
                "hits_count": hits_count,
                "hits": hits_json,
            },
            "content": [
                { "type": "text", "text": text }
            ],
            "isError": false
        }
    })
}

/// Build the JSON-RPC user-facing error envelope for `find_similar`
/// (per master-plan §3.2 UX contract: runtime failures surface as
/// `result.isError = true` with `_meta.error_kind`, NOT as JSON-RPC
/// `error` envelopes).
fn find_similar_error_envelope(
    id: &Value,
    branch: &str,
    query_dim: usize,
    error_kind: &str,
    error_message: &str,
) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "result": {
            "_meta": {
                "tool": "find_similar",
                "source": "lancedb+coderankembed",
                "branch": branch,
                "query_dim": query_dim,
                "error_kind": error_kind,
                "error_message": error_message,
            },
            "content": [
                {
                    "type": "text",
                    "text": format!("find_similar failed ({error_kind}): {error_message}")
                }
            ],
            "isError": true
        }
    })
}

/// Internal error shape for the `find_definition` read pipeline —
/// threads a `(code, message)` pair out of the KG-locked section so the
/// outer handler can build the JSON-RPC error envelope with the mutex
/// guard already released.
#[derive(Debug)]
struct FindDefinitionReadError {
    code: i64,
    message: String,
}

/// Internal payload shape produced by
/// [`McpServer::handle_find_definition`] — threads the KG read results
/// out of the mutex-guarded block so the response encoding happens with
/// the lock released.
#[derive(Debug)]
enum FindDefinitionPayload {
    /// Definition resolved — carries the [`ucil_core::SymbolResolution`]
    /// projection plus the projected caller list (already JSON-shaped).
    Found {
        /// The resolved symbol's [`ucil_core::SymbolResolution`]
        /// projection from [`KnowledgeGraph::resolve_symbol`].
        resolution: ucil_core::SymbolResolution,
        /// Projected caller list, one JSON object per `calls`-kind
        /// inbound edge: `{qualified_name, file_path, start_line}`.
        callers: Vec<Value>,
    },
    /// Resolver returned `Ok(None)` for the requested `(name,
    /// file_scope)` pair.
    NotFound,
}

/// Execute the knowledge-graph reads that back `find_definition`:
/// acquire the lock, resolve the symbol, enumerate `calls`-kind
/// inbound edges, and project each caller onto its
/// `{qualified_name, file_path, start_line}` shape.
///
/// Kept out of the `McpServer` impl so `handle_find_definition` stays
/// under the `clippy::too_many_lines` threshold; the outer method
/// owns the argument parsing and JSON envelope construction.
fn read_find_definition(
    kg: &Arc<Mutex<KnowledgeGraph>>,
    name: &str,
    file_scope: Option<&str>,
) -> Result<FindDefinitionPayload, FindDefinitionReadError> {
    let guard = kg.lock().map_err(|poisoned| {
        tracing::error!("knowledge graph mutex is poisoned: {poisoned}");
        FindDefinitionReadError {
            code: -32603,
            message: "find_definition: internal error (knowledge graph mutex poisoned)".to_owned(),
        }
    })?;
    match guard.resolve_symbol(name, file_scope) {
        Ok(Some(resolution)) => {
            let entity_id = resolution.id.unwrap_or_default();
            let caller_rows = guard.list_relations_by_target(entity_id).map_err(|e| {
                tracing::error!("list_relations_by_target failed: {e}");
                FindDefinitionReadError {
                    code: -32603,
                    message: format!("find_definition: callers lookup failed: {e}"),
                }
            })?;
            let callers = project_callers(&guard, &caller_rows)?;
            drop(guard);
            Ok(FindDefinitionPayload::Found {
                resolution,
                callers,
            })
        }
        Ok(None) => {
            drop(guard);
            Ok(FindDefinitionPayload::NotFound)
        }
        Err(e) => {
            tracing::error!("resolve_symbol failed: {e}");
            Err(FindDefinitionReadError {
                code: -32603,
                message: format!("find_definition: resolve failed: {e}"),
            })
        }
    }
}

/// Project `calls`-kind inbound relations onto their caller entity's
/// `{qualified_name, file_path, start_line}` JSON shape.
///
/// Dangling foreign keys (source row deleted between queries) are
/// logged and skipped — the caller list is best-effort because the
/// §12.1 `relations` table has no cascading delete.
fn project_callers(
    guard: &std::sync::MutexGuard<'_, KnowledgeGraph>,
    caller_rows: &[ucil_core::Relation],
) -> Result<Vec<Value>, FindDefinitionReadError> {
    let mut callers: Vec<Value> = Vec::new();
    for rel in caller_rows.iter().filter(|r| r.kind == "calls") {
        match guard.get_entity_by_id(rel.source_id) {
            Ok(Some(caller)) => {
                callers.push(json!({
                    "qualified_name": caller.qualified_name,
                    "file_path": caller.file_path,
                    "start_line": caller.start_line,
                }));
            }
            Ok(None) => {
                tracing::warn!(
                    source_id = rel.source_id,
                    "find_definition: caller source row missing (dangling fk)",
                );
            }
            Err(e) => {
                tracing::error!("get_entity_by_id failed: {e}");
                return Err(FindDefinitionReadError {
                    code: -32603,
                    message: format!("find_definition: caller projection failed: {e}"),
                });
            }
        }
    }
    Ok(callers)
}

/// Build the JSON-RPC response envelope for a resolved definition.
fn found_response(
    id: &Value,
    name: &str,
    resolution: &ucil_core::SymbolResolution,
    callers: &[Value],
) -> Value {
    let text = format!(
        "`{name}` defined in {} at line {}",
        resolution.file_path,
        resolution
            .start_line
            .map_or_else(|| "?".to_owned(), |l| l.to_string()),
    );
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "result": {
            "_meta": {
                "tool": "find_definition",
                "source": "tree-sitter+kg",
                "found": true,
                "file_path": resolution.file_path,
                "start_line": resolution.start_line,
                "signature": resolution.signature,
                "doc_comment": resolution.doc_comment,
                "parent_module": resolution.parent_module,
                "qualified_name": resolution.qualified_name,
                "callers": callers.to_vec(),
            },
            "content": [
                { "type": "text", "text": text }
            ],
            "isError": false
        }
    })
}

/// Build the JSON-RPC response envelope for an unresolved symbol.
fn not_found_response(id: &Value, name: &str) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "result": {
            "_meta": {
                "tool": "find_definition",
                "source": "tree-sitter+kg",
                "found": false,
            },
            "content": [
                {
                    "type": "text",
                    "text": format!("no definition found for `{name}`"),
                }
            ],
            "isError": false
        }
    })
}

/// Build a JSON-RPC 2.0 error envelope.
pub(crate) fn jsonrpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "error": { "code": code, "message": message }
    })
}

/// Internal error shape for the `get_conventions` read pipeline —
/// mirrors [`FindDefinitionReadError`]: threads a `(code, message)`
/// pair out of the KG-locked section so the outer handler can build
/// the JSON-RPC error envelope with the mutex guard already released.
#[derive(Debug)]
struct GetConventionsReadError {
    code: i64,
    message: String,
}

/// Execute the knowledge-graph read that backs `get_conventions`:
/// acquire the mutex, call [`KnowledgeGraph::list_conventions`], and
/// release the lock before returning.
///
/// Kept out of the `McpServer` impl so `handle_get_conventions` stays
/// below the `clippy::too_many_lines` threshold — the outer method
/// owns the argument parsing and JSON envelope construction, the
/// helper owns the mutex-lock and KG call.
fn read_conventions(
    kg: &Arc<Mutex<KnowledgeGraph>>,
    category: Option<&str>,
) -> Result<Vec<ucil_core::Convention>, GetConventionsReadError> {
    let guard = kg.lock().map_err(|poisoned| {
        tracing::error!("knowledge graph mutex is poisoned: {poisoned}");
        GetConventionsReadError {
            code: -32603,
            message: "get_conventions: internal error (knowledge graph mutex poisoned)".to_owned(),
        }
    })?;
    let rows = guard.list_conventions(category).map_err(|e| {
        tracing::error!("list_conventions failed: {e}");
        GetConventionsReadError {
            code: -32603,
            message: format!("get_conventions: list failed: {e}"),
        }
    })?;
    drop(guard);
    Ok(rows)
}

/// Project a [`ucil_core::Convention`] onto the JSON-object shape the
/// `get_conventions` response advertises in `_meta.conventions`.
///
/// Every column mirrors the `conventions` table schema at
/// master-plan §12.1 lines 1172-1182 — including the nullable
/// `examples`, `counter_examples`, and `last_verified` columns, which
/// are encoded as `null` when absent.
fn convention_to_json(convention: &ucil_core::Convention) -> Value {
    json!({
        "id": convention.id,
        "category": convention.category,
        "pattern": convention.pattern,
        "examples": convention.examples,
        "counter_examples": convention.counter_examples,
        "confidence": convention.confidence,
        "evidence_count": convention.evidence_count,
        "t_ingested_at": convention.t_ingested_at,
        "last_verified": convention.last_verified,
        "scope": convention.scope,
    })
}

/// Build the JSON-RPC response envelope for a successful
/// `get_conventions` read.  Empty `conventions` is a valid shape — the
/// response carries `_meta.count == 0`, `_meta.conventions == []`, and
/// `isError == false` per master-plan §3.2 row 7.
fn get_conventions_found_response(
    id: &Value,
    category: Option<&str>,
    conventions: &[ucil_core::Convention],
) -> Value {
    let count = conventions.len();
    let text = if count == 0 {
        "no conventions yet".to_owned()
    } else {
        format!("{count} convention{}", if count == 1 { "" } else { "s" })
    };
    let conventions_json: Vec<Value> = conventions.iter().map(convention_to_json).collect();
    let category_json = category.map_or(Value::Null, |c| Value::String(c.to_owned()));
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "result": {
            "_meta": {
                "tool": "get_conventions",
                "source": "kg",
                "count": count,
                "category": category_json,
                "conventions": conventions_json,
            },
            "content": [
                { "type": "text", "text": text }
            ],
            "isError": false
        }
    })
}

// ── search_code helpers (P1-W5-F09) ──────────────────────────────────────────

/// Internal error shape for the `search_code` read pipeline — mirrors
/// [`FindDefinitionReadError`] and [`GetConventionsReadError`] so the
/// outer handler can build the JSON-RPC error envelope with the mutex
/// guard already released.
#[derive(Debug)]
struct SearchCodeReadError {
    /// JSON-RPC 2.0 error code (`-32603` internal, `-32602` invalid
    /// params).
    code: i64,
    /// Human-readable error message propagated up to the caller.
    message: String,
}

/// Parsed, validated `search_code` arguments.  Produced by
/// [`parse_search_code_args`] so the outer [`McpServer::handle_search_code`]
/// stays under the `clippy::too_many_lines` threshold.
#[derive(Debug)]
struct SearchCodeArgs {
    /// Caller's non-empty query string.  Passed verbatim to
    /// [`ucil_core::KnowledgeGraph::search_entities_by_name`] (substring
    /// match) and to [`grep_regex::RegexMatcherBuilder`] (regex match).
    query: String,
    /// Filesystem root for the text half of the search.  Guaranteed to
    /// be an existing directory at this point.
    root: PathBuf,
    /// Per-half hit cap — already saturating-clamped to
    /// [`SEARCH_CODE_MAX_RESULTS`].
    max_results: usize,
}

/// Extract and validate the three `search_code` arguments — `query`
/// (required non-empty string), `root` (optional string; defaults to
/// `std::env::current_dir`), and `max_results` (optional non-negative
/// integer; defaults to [`SEARCH_CODE_DEFAULT_MAX_RESULTS`] and is
/// saturating-clamped at [`SEARCH_CODE_MAX_RESULTS`]).
///
/// Missing-or-wrong-type arguments map to a [`SearchCodeReadError`]
/// with code `-32602` (Invalid params).  Caller converts that into the
/// JSON-RPC error envelope via [`jsonrpc_error`].
fn parse_search_code_args(args: &Value) -> Result<SearchCodeArgs, SearchCodeReadError> {
    // `arguments.query` — required, non-empty string.
    let query: String = match args.get("query") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::String(_)) => {
            return Err(SearchCodeReadError {
                code: -32602,
                message: "search_code: `arguments.query` must not be empty".to_owned(),
            });
        }
        _ => {
            return Err(SearchCodeReadError {
                code: -32602,
                message: "search_code: `arguments.query` is required and must be a string"
                    .to_owned(),
            });
        }
    };

    // `arguments.root` — optional string; default to cwd.  Missing
    // key, explicit `null`, and an empty string all fall back to the
    // daemon's current working directory.
    let root: PathBuf = match args.get("root") {
        None | Some(Value::Null) => cwd_or_dot(),
        Some(Value::String(s)) if s.is_empty() => cwd_or_dot(),
        Some(Value::String(s)) => PathBuf::from(s),
        Some(_) => {
            return Err(SearchCodeReadError {
                code: -32602,
                message: "search_code: `arguments.root` must be a string (or omitted/null)"
                    .to_owned(),
            });
        }
    };
    if !root.is_dir() {
        return Err(SearchCodeReadError {
            code: -32602,
            message: format!(
                "search_code: `arguments.root` does not exist or is not a directory: {}",
                root.display(),
            ),
        });
    }

    // `arguments.max_results` — optional non-negative integer.
    let max_results: usize = match args.get("max_results") {
        None | Some(Value::Null) => SEARCH_CODE_DEFAULT_MAX_RESULTS,
        Some(v) => match v.as_u64() {
            Some(n) => clamp_max_results(n),
            None => {
                return Err(SearchCodeReadError {
                    code: -32602,
                    message:
                        "search_code: `arguments.max_results` must be a non-negative integer (or omitted)"
                            .to_owned(),
                });
            }
        },
    };

    Ok(SearchCodeArgs {
        query,
        root,
        max_results,
    })
}

/// Return the process's current working directory, falling back to
/// `"."` (relative, resolved against whatever the downstream walker
/// decides) if the syscall fails.  The fallback is logged at `WARN`.
fn cwd_or_dot() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|e| {
        tracing::warn!("search_code: current_dir() failed: {e}; falling back to '.'");
        PathBuf::from(".")
    })
}

/// Clamp a caller-supplied `max_results` value to
/// [`SEARCH_CODE_MAX_RESULTS`], logging at `WARN` when the clamp
/// actually fires so the agent can see the cap in its own logs.
fn clamp_max_results(n: u64) -> usize {
    let requested = usize::try_from(n).unwrap_or(SEARCH_CODE_MAX_RESULTS);
    if requested > SEARCH_CODE_MAX_RESULTS {
        tracing::warn!(
            requested,
            cap = SEARCH_CODE_MAX_RESULTS,
            "search_code: `arguments.max_results` clamped to cap",
        );
        SEARCH_CODE_MAX_RESULTS
    } else {
        requested
    }
}

/// One merged row emitted by [`handle_search_code`] — the JSON
/// serialisation is the `_meta.results[]` element shape advertised on
/// the wire (WO-0035 `scope_in` point 7 / acceptance-test field spec).
///
/// `source` is the discriminant:
///
/// * `"symbol"` — only the KG half reported this `(file, line)`.
/// * `"text"` — only the `ripgrep` half reported this `(file, line)`.
/// * `"both"` — both halves reported it; the symbol metadata is
///   preserved and `preview` carries the `ripgrep` line text so the
///   caller sees the raw matched source line.
///
/// `qualified_name` / `signature` are symbol-only; text-only rows
/// omit them via `#[serde(skip_serializing_if = "Option::is_none")]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct SearchCodeResult {
    /// Origin discriminant: `"symbol"`, `"text"`, or `"both"`.
    source: String,
    /// Path of the file the hit came from — absolute when produced by
    /// the text walker (walker yields absolute paths), and as-stored
    /// on the KG row when produced by the symbol half.
    file_path: String,
    /// 1-indexed line number.  For symbol rows this is the KG's
    /// `start_line`; for text rows it is `grep_searcher`'s reported
    /// line number.
    line_number: u64,
    /// Human-readable preview for the hit.  Symbol rows use the
    /// entity's `qualified_name` (falling back to `name` when the
    /// entity has no qualified name); text rows use the matching line
    /// with its trailing terminator stripped.  On a `"both"`
    /// collision, the raw text line wins so the caller sees the
    /// source line rather than the qualified name.
    preview: String,
    /// Fully qualified symbol name — present on `"symbol"` and
    /// `"both"` rows only, and only when the entity row carried one.
    #[serde(skip_serializing_if = "Option::is_none")]
    qualified_name: Option<String>,
    /// Tree-sitter-extracted symbol signature — present on `"symbol"`
    /// and `"both"` rows only, and only when the entity row carried
    /// one.
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
}

/// Execute the symbol half of the `search_code` pipeline: acquire the
/// KG lock, call [`KnowledgeGraph::search_entities_by_name`], and
/// release the lock before returning.
///
/// Kept out of the `McpServer` impl so `handle_search_code` stays
/// below the `clippy::too_many_lines` threshold; the outer method owns
/// the argument parsing and JSON envelope construction.
fn read_symbol_matches(
    kg: &Arc<Mutex<KnowledgeGraph>>,
    query: &str,
    limit: usize,
) -> Result<Vec<ucil_core::Entity>, SearchCodeReadError> {
    let guard = kg.lock().map_err(|poisoned| {
        tracing::error!("knowledge graph mutex is poisoned: {poisoned}");
        SearchCodeReadError {
            code: -32603,
            message: "search_code: internal error (knowledge graph mutex poisoned)".to_owned(),
        }
    })?;
    let rows = guard.search_entities_by_name(query, limit).map_err(|e| {
        tracing::error!("search_entities_by_name failed: {e}");
        SearchCodeReadError {
            code: -32603,
            message: format!("search_code: search failed: {e}"),
        }
    })?;
    drop(guard);
    Ok(rows)
}

/// Merge the symbol-half hits (from the KG) with the text-half hits
/// (from `ripgrep`) into a single [`SearchCodeResult`] list, capped at
/// `max_results`.  Pure function — no IO, no locks — so it is unit-
/// tested independently via [`test_merge_search_results_*`].
///
/// **Dedup key:** `(file_path, line_number)`.  When both halves
/// report the same key, the row is kept at its symbol-produced index
/// (the symbol row is pushed first) and its `source` is flipped from
/// `"symbol"` to `"both"`; the text row's `line_text` overwrites the
/// symbol row's so the caller sees the raw matched line.
///
/// **Ordering:** symbols come first (in the order
/// [`ucil_core::KnowledgeGraph::search_entities_by_name`] returned
/// them — ingest-time DESC), then text hits (walker order, typically
/// filesystem-natural).  The merge preserves this partial order so
/// callers can visually scan "all structural hits first, then the rest
/// of the text walk".
fn merge_search_results(
    symbols: &[ucil_core::Entity],
    texts: &[TextMatch],
    max_results: usize,
) -> Vec<SearchCodeResult> {
    use std::collections::HashMap;

    let cap = max_results.min(symbols.len() + texts.len());
    let mut out: Vec<SearchCodeResult> = Vec::with_capacity(cap);
    let mut seen: HashMap<(String, u64), usize> = HashMap::with_capacity(cap);

    for e in symbols {
        if out.len() >= max_results {
            break;
        }
        let line = e
            .start_line
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0);
        let key = (e.file_path.clone(), line);
        if seen.contains_key(&key) {
            // Duplicate symbol row (same file+line already pushed) —
            // keep the first, drop the second.  Should not happen with
            // the current ingest pipeline but is cheap to guard.
            continue;
        }
        let preview = e.qualified_name.clone().unwrap_or_else(|| e.name.clone());
        let idx = out.len();
        out.push(SearchCodeResult {
            source: "symbol".to_owned(),
            file_path: e.file_path.clone(),
            line_number: line,
            preview,
            qualified_name: e.qualified_name.clone(),
            signature: e.signature.clone(),
        });
        seen.insert(key, idx);
    }

    for t in texts {
        if out.len() >= max_results {
            break;
        }
        let key = (t.file_path.display().to_string(), t.line_number);
        if let Some(&idx) = seen.get(&key) {
            if let Some(slot) = out.get_mut(idx) {
                "both".clone_into(&mut slot.source);
                slot.preview.clone_from(&t.line_text);
            }
            continue;
        }
        let idx = out.len();
        out.push(SearchCodeResult {
            source: "text".to_owned(),
            file_path: t.file_path.display().to_string(),
            line_number: t.line_number,
            preview: t.line_text.clone(),
            qualified_name: None,
            signature: None,
        });
        seen.insert(key, idx);
    }

    out
}

/// Fan out a search query over the three G2 providers in parallel and
/// fuse the per-source ranked outputs via [`fuse_g2_rrf`] per
/// master-plan §5.2 lines 447-461 / `DEC-0015` D1.
///
/// Each provider future is wrapped in
/// `tokio::time::timeout(G2_PER_SOURCE_DEADLINE, ...)` so a single slow
/// engine cannot stall the response — `Err` and `Timeout` are dropped
/// silently with a `tracing::warn!`, matching the partial-results
/// semantics of `fuse_g1` from WO-0048.
async fn run_g2_fan_out(
    factory: &Arc<G2SourceFactory>,
    query: &str,
    root: &Path,
    max_results: usize,
) -> G2FusedOutcome {
    let providers = factory.build();
    let mut tasks = Vec::with_capacity(providers.len());
    for provider in providers {
        let q = query.to_owned();
        let r = root.to_path_buf();
        tasks.push(tokio::spawn(async move {
            let label = provider.source();
            let outcome = timeout(
                G2_PER_SOURCE_DEADLINE,
                provider.execute(&q, &r, max_results),
            )
            .await;
            match outcome {
                Ok(Ok(results)) => Some(results),
                Ok(Err(e)) => {
                    tracing::warn!(?label, error = %e, "g2 provider failed");
                    None
                }
                Err(_elapsed) => {
                    tracing::warn!(?label, "g2 provider exceeded per-source deadline");
                    None
                }
            }
        }));
    }

    let mut per_source: Vec<G2SourceResults> = Vec::with_capacity(tasks.len());
    for task in tasks {
        match task.await {
            Ok(Some(results)) => per_source.push(results),
            Ok(None) => {}
            Err(join_err) => {
                tracing::warn!(error = %join_err, "g2 provider task join failed");
            }
        }
    }

    fuse_g2_rrf(&per_source)
}

/// Build the JSON-RPC 2.0 response envelope for a successful
/// `search_code` read.
///
/// Empty `results` is a valid shape — the response carries
/// `_meta.count == 0`, `_meta.results == []`, `isError == false`, and
/// a human-readable `"no matches for <query>"` text, mirroring
/// `get_conventions_found_response`'s empty-table contract.
fn search_code_response(
    id: &Value,
    query: &str,
    root: &Path,
    symbol_match_count: usize,
    text_match_count: usize,
    results: &[SearchCodeResult],
) -> Value {
    let count = results.len();
    let results_json: Vec<Value> = results
        .iter()
        .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
        .collect();
    let text = if count == 0 {
        format!("no matches for `{query}`")
    } else {
        format!("{count} match{}", if count == 1 { "" } else { "es" })
    };
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "result": {
            "_meta": {
                "tool": "search_code",
                "source": "tree-sitter+ripgrep",
                "count": count,
                "query": query,
                "root": root.display().to_string(),
                "symbol_match_count": symbol_match_count,
                "text_match_count": text_match_count,
                "results": results_json,
            },
            "content": [
                { "type": "text", "text": text }
            ],
            "isError": false
        }
    })
}

// ── Module-level acceptance test ─────────────────────────────────────────────
//
// Placed as a module-level item (NOT inside a `mod tests { }` block)
// so the nextest selector `server::test_all_22_tools_registered`
// resolves — see DEC-0007 and the WO-0006 lesson for the
// test-selector rule this WO is gated against.

/// Acceptance test for `P1-W3-F07`.
///
/// Exercises the real `McpServer::serve` loop over a
/// [`tokio::io::duplex`] pair:
///
/// 1. Writes a `tools/list` JSON-RPC request.
/// 2. Parses the response, asserts 22 tools with the exact §3.2 names,
///    and checks every descriptor's `inputSchema` carries the four
///    CEQP universal properties.
/// 3. Writes a `tools/call` for `understand_code` and asserts the
///    response result carries `_meta.not_yet_implemented: true`.
///
/// No mocks of `serde_json` or `tokio::io` — the duplex pair is real
/// in-memory IO and the server runs in a spawned task.
#[cfg(test)]
#[tokio::test]
// The test walks end-to-end through two full JSON-RPC exchanges
// (tools/list + tools/call), so a single linear body is the clearest
// way to keep the assertions ordered.  Splitting into helpers would
// bury the protocol sequence in indirection without shortening what a
// reader has to follow.
#[allow(clippy::too_many_lines)]
async fn test_all_22_tools_registered() {
    use tokio::io::{duplex, split, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

    // Expected tool names, master-plan §3.2, case-sensitive.
    const EXPECTED_TOOLS: [&str; TOOL_COUNT] = [
        "understand_code",
        "find_definition",
        "find_references",
        "search_code",
        "find_similar",
        "get_context_for_edit",
        "get_conventions",
        "get_architecture",
        "trace_dependencies",
        "blast_radius",
        "explain_history",
        "remember",
        "review_changes",
        "check_quality",
        "run_tests",
        "security_scan",
        "lint_code",
        "type_check",
        "refactor",
        "generate_docs",
        "query_database",
        "check_runtime",
    ];
    const CEQP_PROPS: [&str; 4] = ["reason", "current_task", "files_in_context", "token_budget"];

    let server = McpServer::new();
    assert_eq!(
        server.tools.len(),
        TOOL_COUNT,
        "McpServer::new() must populate exactly 22 tools",
    );

    // Wire up an in-memory duplex and split each end into read/write
    // halves so the server and the test harness can drive IO
    // independently.
    let (client_end, server_end) = duplex(16 * 1024);
    let (server_read, server_write) = split(server_end);
    let (client_read, mut client_write) = split(client_end);

    // Spawn the server loop; joined at the bottom after the client
    // drops its write half.
    let server_task = tokio::spawn(async move { server.serve(server_read, server_write).await });

    // ── tools/list ───────────────────────────────────────────────────
    let req_list = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}
"#;
    client_write
        .write_all(req_list)
        .await
        .expect("write tools/list");
    client_write.flush().await.expect("flush tools/list");

    let mut reader = BufReader::new(client_read);
    let mut frame = String::new();
    reader
        .read_line(&mut frame)
        .await
        .expect("read tools/list response");

    let response: Value =
        serde_json::from_str(frame.trim()).expect("tools/list response must be valid JSON");
    let tools = response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(Value::as_array)
        .expect("tools/list result.tools must be a JSON array");

    assert_eq!(
        tools.len(),
        TOOL_COUNT,
        "tools/list must report exactly 22 tools (got {})",
        tools.len(),
    );

    let got_names: Vec<String> = tools
        .iter()
        .map(|t| {
            t.get("name")
                .and_then(Value::as_str)
                .expect("each tool must carry a string `name`")
                .to_owned()
        })
        .collect();

    for expected in EXPECTED_TOOLS {
        assert!(
            got_names.iter().any(|n| n == expected),
            "tools/list missing expected §3.2 tool: `{expected}` (got {got_names:?})",
        );
    }

    for tool in tools {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let schema = tool
            .get("inputSchema")
            .expect("every tool must carry an inputSchema");
        let props = schema
            .get("properties")
            .and_then(Value::as_object)
            .unwrap_or_else(|| {
                panic!("tool {name} inputSchema.properties must be an object");
            });
        for ceqp in CEQP_PROPS {
            assert!(
                props.contains_key(ceqp),
                "tool `{name}` inputSchema is missing CEQP universal param `{ceqp}`",
            );
        }
    }

    // ── tools/call (understand_code) — stub path ─────────────────────
    let req_call = br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"understand_code","arguments":{"reason":"verifier smoke"}}}
"#;
    client_write
        .write_all(req_call)
        .await
        .expect("write tools/call");
    client_write.flush().await.expect("flush tools/call");

    frame.clear();
    reader
        .read_line(&mut frame)
        .await
        .expect("read tools/call response");
    let call_resp: Value =
        serde_json::from_str(frame.trim()).expect("tools/call response must be valid JSON");
    let meta_not_impl = call_resp
        .pointer("/result/_meta/not_yet_implemented")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(
        meta_not_impl,
        "tools/call response must carry _meta.not_yet_implemented == true, got: {call_resp}",
    );

    // Shut down the client write half — EOF drives the server loop to
    // exit.  Plain `drop` on the WriteHalf is not sufficient because
    // `tokio::io::split` keeps the underlying stream alive as long as
    // either half is live; an explicit `shutdown` flushes + closes.
    client_write
        .shutdown()
        .await
        .expect("shutdown client write half");
    drop(client_write);
    let serve_result = timeout(Duration::from_secs(3), server_task)
        .await
        .expect("server task must finish within 3s of EOF")
        .expect("server task must not panic");
    serve_result.expect("server loop must return Ok after clean EOF");
}

/// Acceptance test for `P1-W4-F06`.
///
/// Frozen selector: `server::test_ceqp_params_on_all_tools` (exact
/// match — must live at module level, not under `mod tests { … }`).
///
/// `test_all_22_tools_registered` above exercises the full wire
/// protocol and already contains a CEQP-properties check, but its
/// selector is `server::test_all_22_tools_registered`.  Master-plan
/// §8.2 assigns CEQP universals their own feature (`P1-W4-F06`) with
/// its own frozen selector, so this is the named regression test that
/// lives independently of the `tools/list` IO test and asserts:
///
/// 1. `ucil_tools()` reports exactly 22 descriptors.
/// 2. Every descriptor's `input_schema.properties` carries the four
///    CEQP universal keys (`reason`, `current_task`,
///    `files_in_context`, `token_budget`).
/// 3. Each CEQP key's `type` matches master-plan §8.2
///    (`string`, `string`, `array`, `integer`).
///
/// Failures are collected across **all** tools then asserted at the
/// end so that a broken schema points at every offender at once —
/// much cheaper to diagnose than fail-at-first.
#[cfg(test)]
#[tokio::test]
async fn test_ceqp_params_on_all_tools() {
    // (key, expected JSON-Schema type) per master-plan §8.2.
    const CEQP_FIELDS: [(&str, &str); 4] = [
        ("reason", "string"),
        ("current_task", "string"),
        ("files_in_context", "array"),
        ("token_budget", "integer"),
    ];

    let tools = ucil_tools();
    assert_eq!(
        tools.len(),
        TOOL_COUNT,
        "ucil_tools() must return exactly {TOOL_COUNT} descriptors, got {}",
        tools.len(),
    );

    let mut missing: Vec<String> = Vec::new();
    let mut type_mismatches: Vec<String> = Vec::new();

    for tool in &tools {
        let props = tool
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .unwrap_or_else(|| {
                panic!(
                    "tool `{}` input_schema is missing a `properties` object",
                    tool.name
                );
            });

        for (key, expected_type) in CEQP_FIELDS {
            let Some(prop) = props.get(key) else {
                missing.push(format!("{}.{}", tool.name, key));
                continue;
            };

            let got_type = prop.get("type").and_then(Value::as_str).unwrap_or("<none>");
            if got_type != expected_type {
                type_mismatches.push(format!(
                    "{}.{}: expected type=`{}`, got `{}`",
                    tool.name, key, expected_type, got_type,
                ));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "CEQP universal params missing on these (tool.prop) pairs: {missing:?}",
    );
    assert!(
        type_mismatches.is_empty(),
        "CEQP universal param types mismatch master-plan §8.2: {type_mismatches:?}",
    );
}

/// Acceptance test for `P1-W3-F08` — progressive startup.
///
/// Frozen selector: `server::test_progressive_startup` (exact match —
/// must live at module level, not under `mod tests { … }` per DEC-0005).
///
/// Master-plan §18 Phase 1 Week 3 line 1745 specifies two observable
/// invariants:
///
/// 1. **Startup budget.** The MCP server accepts and responds to
///    `tools/list` within [`crate::startup::STARTUP_DEADLINE`] — the
///    2 s ceiling from §21.2 lines 2196-2204.
/// 2. **Priority ordering.** Paths touched via the
///    [`crate::startup::handle_call_for_priority`] helper pop off the
///    shared [`crate::priority_queue::PriorityIndexingQueue`] in
///    newest-first order — the "recently queried files first"
///    invariant.
///
/// The test drives both invariants end-to-end over a real
/// [`tokio::io::duplex`] pair (no mocks of `McpServer` or `tokio::io`)
/// and re-asserts the 22-tool contract from
/// `test_all_22_tools_registered` so the `tools/list` catalogue is
/// proven fully wired during the startup window.
#[cfg(test)]
#[tokio::test]
async fn test_progressive_startup() {
    use std::{path::PathBuf, sync::Arc};

    use serde_json::json;
    use tokio::io::{duplex, split, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

    use crate::{
        priority_queue::PriorityIndexingQueue,
        startup::{handle_call_for_priority, ProgressiveStartup, STARTUP_DEADLINE},
    };

    let queue = Arc::new(PriorityIndexingQueue::new());
    let server = McpServer::new();
    let startup = ProgressiveStartup::new(server, Arc::clone(&queue));

    // 64 KiB duplex — the full tools/list response is ≈ 18 KiB for the
    // 22-descriptor catalogue, so the default 16 KiB cap in
    // `test_all_22_tools_registered` would backpressure the server's
    // second `poll_write` (the frame terminator) until the client
    // drains the buffer. We drain concurrently below, but the larger
    // buffer also guards against transient stalls.
    let (client_end, server_end) = duplex(64 * 1024);
    let (server_read, server_write) = split(server_end);
    let (client_read, mut client_write) = split(client_end);

    let (server_task, ready_handle) = startup.start(server_read, server_write);

    // Client-side: write one tools/list request. The ReadyProbeWriter
    // inside ProgressiveStartup signals the ReadyHandle on the server's
    // first framed response.
    let req_list = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}
"#;
    client_write
        .write_all(req_list)
        .await
        .expect("write tools/list");
    client_write.flush().await.expect("flush tools/list");

    // Concurrently drain the response frame while awaiting
    // `ReadyHandle::wait` — an MCP host would always read as fast as
    // possible, and concurrent draining avoids the deadlock where the
    // server's `poll_write` on the frame terminator blocks because the
    // duplex buffer is full, which in turn keeps `seen_newline` from
    // ever flipping inside `ReadyProbeWriter`.
    let mut reader = BufReader::new(client_read);
    let mut frame = String::new();
    let read_fut = reader.read_line(&mut frame);

    // Outer 3 s cap — ReadyHandle::wait already enforces
    // STARTUP_DEADLINE + slack internally, but a belt-and-braces timeout
    // is cheap insurance against a wedged duplex.
    let (elapsed, read_result) = tokio::join!(
        timeout(Duration::from_secs(3), ready_handle.wait()),
        read_fut
    );
    let elapsed = elapsed
        .expect("ready handle must finish within 3 s")
        .expect("ready handle must resolve with Ok(Duration)");
    read_result.expect("read tools/list response");
    assert!(
        elapsed < STARTUP_DEADLINE,
        "startup-to-first-response {elapsed:?} must be < STARTUP_DEADLINE {STARTUP_DEADLINE:?}",
    );
    let response: Value =
        serde_json::from_str(frame.trim()).expect("tools/list response must be valid JSON");
    let tools = response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(Value::as_array)
        .expect("tools/list result.tools must be a JSON array");
    assert_eq!(
        tools.len(),
        TOOL_COUNT,
        "tools/list must report exactly {TOOL_COUNT} tools during startup (got {})",
        tools.len(),
    );

    // Priority-ordering invariant: touching two paths via the CEQP
    // helper causes the most-recently-touched one to pop first.
    handle_call_for_priority(
        &queue,
        &json!({
            "current_task": {
                "files_in_context": ["src/a.rs", "src/b.rs"]
            }
        }),
    );
    let first = queue.pop().expect("queue must have at least one entry");
    assert_eq!(
        first.path,
        PathBuf::from("src/b.rs"),
        "last-touched path must pop first",
    );
    let second = queue.pop().expect("queue must have a second entry");
    assert_eq!(
        second.path,
        PathBuf::from("src/a.rs"),
        "earlier-touched path must pop second",
    );

    // Clean shutdown — EOF drives the server loop to exit, then join.
    client_write
        .shutdown()
        .await
        .expect("shutdown client write half");
    drop(client_write);
    let serve_result = timeout(Duration::from_secs(3), server_task)
        .await
        .expect("server task must finish within 3 s of EOF")
        .expect("server task must not panic");
    serve_result.expect("server loop must return Ok after clean EOF");
}

// ── merge_search_results unit tests (P1-W5-F09, pure function) ───────────
//
// `merge_search_results` is a pure function (no IO, no locks, no
// system calls), so it is unit-tested independently of the full
// `search_code` MCP dispatch — the acceptance test
// `test_search_code_basic` exercises the same function end-to-end.

#[cfg(test)]
fn mk_symbol_entity(
    name: &str,
    kind: &str,
    qualified_name: Option<&str>,
    file_path: &str,
    start_line: i64,
) -> ucil_core::Entity {
    ucil_core::Entity {
        id: None,
        kind: kind.to_owned(),
        name: name.to_owned(),
        qualified_name: qualified_name.map(str::to_owned),
        file_path: file_path.to_owned(),
        start_line: Some(start_line),
        end_line: Some(start_line + 2),
        signature: None,
        doc_comment: None,
        language: Some("rust".to_owned()),
        t_valid_from: None,
        t_valid_to: None,
        importance: 0.5,
        source_tool: Some("tree-sitter".to_owned()),
        source_hash: Some("synthetic".to_owned()),
    }
}

#[cfg(test)]
fn mk_text_match(file_path: &str, line_number: u64, line_text: &str) -> TextMatch {
    TextMatch {
        file_path: PathBuf::from(file_path),
        line_number,
        line_text: line_text.to_owned(),
    }
}

/// Symbol-only path: when only the KG half produces hits,
/// `merge_search_results` returns one row per symbol, each marked
/// `source == "symbol"`, and the order mirrors the input.
#[cfg(test)]
#[test]
fn test_merge_search_results_symbol_only() {
    let symbols = vec![
        mk_symbol_entity(
            "banana_split",
            "function",
            Some("util::banana_split"),
            "/tmp/src/util.rs",
            10,
        ),
        mk_symbol_entity(
            "banana_peel",
            "function",
            Some("util::banana_peel"),
            "/tmp/src/util.rs",
            20,
        ),
    ];
    let texts: Vec<TextMatch> = Vec::new();

    let merged = merge_search_results(&symbols, &texts, 50);
    assert_eq!(
        merged.len(),
        2,
        "symbol-only merge keeps all rows: {merged:?}"
    );
    assert_eq!(merged[0].source, "symbol");
    assert_eq!(
        merged[0].qualified_name.as_deref(),
        Some("util::banana_split")
    );
    assert_eq!(merged[0].preview, "util::banana_split");
    assert_eq!(merged[1].source, "symbol");
    assert_eq!(
        merged[1].qualified_name.as_deref(),
        Some("util::banana_peel")
    );
}

/// Text-only path: when only the `ripgrep` half produces hits,
/// `merge_search_results` returns one row per text match, each
/// marked `source == "text"` with `qualified_name` / `signature`
/// both `None` (and therefore absent from the JSON encoding).
#[cfg(test)]
#[test]
fn test_merge_search_results_text_only() {
    let symbols: Vec<ucil_core::Entity> = Vec::new();
    let texts = vec![
        mk_text_match("/tmp/src/util.rs", 42, "fn banana_split() {}"),
        mk_text_match("/tmp/README.md", 3, "  banana_split — split a banana"),
    ];

    let merged = merge_search_results(&symbols, &texts, 50);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].source, "text");
    assert_eq!(merged[0].file_path, "/tmp/src/util.rs");
    assert_eq!(merged[0].line_number, 42);
    assert_eq!(merged[0].preview, "fn banana_split() {}");
    assert!(merged[0].qualified_name.is_none());
    assert!(merged[0].signature.is_none());
    assert_eq!(merged[1].source, "text");
    assert_eq!(merged[1].file_path, "/tmp/README.md");
}

/// Collision path: the same `(file_path, line_number)` reported by
/// both halves collapses to a single row whose `source` flips to
/// `"both"`, `qualified_name` / `signature` are retained, and
/// `preview` is overwritten with the `ripgrep` line text so the
/// caller sees the raw matched line.
#[cfg(test)]
#[test]
fn test_merge_search_results_collision_flips_to_both() {
    let symbols = vec![mk_symbol_entity(
        "banana_split",
        "function",
        Some("util::banana_split"),
        "/tmp/src/util.rs",
        10,
    )];
    let texts = vec![
        mk_text_match("/tmp/src/util.rs", 10, "fn banana_split() { /* ... */ }"),
        mk_text_match("/tmp/README.md", 3, "  banana_split"),
    ];

    let merged = merge_search_results(&symbols, &texts, 50);
    assert_eq!(merged.len(), 2, "collision collapses to 2 rows: {merged:?}");
    assert_eq!(merged[0].source, "both");
    assert_eq!(merged[0].preview, "fn banana_split() { /* ... */ }");
    assert_eq!(
        merged[0].qualified_name.as_deref(),
        Some("util::banana_split")
    );
    assert_eq!(merged[1].source, "text");
    assert_eq!(merged[1].file_path, "/tmp/README.md");
}

/// Cap path: when `symbols.len() + texts.len()` exceeds
/// `max_results`, `merge_search_results` stops pushing rows as soon
/// as the cap is reached.  Symbol rows are pushed first, so caps
/// below the symbol count return only symbols; caps inside the text
/// range return all symbols plus a prefix of texts.
#[cfg(test)]
#[test]
fn test_merge_search_results_respects_max_results() {
    let symbols = vec![
        mk_symbol_entity("banana_split", "function", None, "/tmp/a.rs", 1),
        mk_symbol_entity("banana_peel", "function", None, "/tmp/b.rs", 2),
    ];
    let texts = vec![
        mk_text_match("/tmp/c.rs", 1, "banana_split"),
        mk_text_match("/tmp/d.rs", 1, "banana_peel"),
    ];

    // Cap below symbol count → only the first symbol comes back.
    let merged_1 = merge_search_results(&symbols, &texts, 1);
    assert_eq!(merged_1.len(), 1);
    assert_eq!(merged_1[0].source, "symbol");
    assert_eq!(merged_1[0].file_path, "/tmp/a.rs");

    // Cap inside the text range → both symbols + one text.
    let merged_3 = merge_search_results(&symbols, &texts, 3);
    assert_eq!(merged_3.len(), 3);
    assert_eq!(merged_3[0].source, "symbol");
    assert_eq!(merged_3[1].source, "symbol");
    assert_eq!(merged_3[2].source, "text");
    assert_eq!(merged_3[2].file_path, "/tmp/c.rs");
}

// ── find_definition acceptance tests (P1-W4-F05) ─────────────────────────
//
// Per DEC-0005, these live at module root (NOT under `mod tests { … }`)
// so the frozen acceptance selector `server::test_find_definition_tool`
// resolves to `ucil_daemon::server::test_find_definition_tool` for
// `cargo nextest run -p ucil-daemon server::test_find_definition_tool`
// without an intermediate `tests::` segment.

/// Build an `McpServer::with_knowledge_graph`-backed server populated
/// by running the real tree-sitter → KG pipeline on the fixture
/// `tests/fixtures/rust-project/src/util.rs`, then upsert a synthetic
/// `calls`-kind relation whose target is the fixture's `evaluate`
/// function so the happy-path response carries a non-empty callers
/// list.
///
/// Returns `(server, kg_arc, tmp_dir, fixture_file_str, evaluate_id,
/// caller_qualified_name)` so the caller can assert the response
/// envelope against known-good values.
#[cfg(test)]
#[allow(clippy::type_complexity)]
fn build_find_definition_fixture() -> (
    McpServer,
    Arc<Mutex<ucil_core::KnowledgeGraph>>,
    tempfile::TempDir,
    String,
    i64,
    String,
) {
    use ucil_core::{Entity, KnowledgeGraph, Relation};

    use crate::executor::{
        rust_project_fixture, IngestPipeline, SOURCE_TOOL, TREE_SITTER_VALID_FROM,
    };

    let tmp = tempfile::TempDir::new().expect("tempdir must be creatable");
    let kg_path = tmp.path().join("knowledge.db");
    let mut kg = KnowledgeGraph::open(&kg_path).expect("KnowledgeGraph::open must succeed");

    // Ingest the fixture so the KG holds a real `evaluate` symbol row.
    let fixture = rust_project_fixture();
    let util_rs = fixture.join("src/util.rs");
    assert!(util_rs.is_file(), "fixture {util_rs:?} must exist");
    let mut pipeline = IngestPipeline::new();
    let inserted = pipeline
        .ingest_file(&mut kg, &util_rs)
        .expect("ingest_file must succeed");
    assert!(
        inserted > 0,
        "fixture ingest must produce ≥1 symbol (got {inserted})"
    );

    let file_path_str = util_rs.display().to_string();

    // Locate the `evaluate` row id (function kind at line 128 in the
    // fixture — the ingest pipeline upserts one row per extracted
    // symbol).
    let evaluate_id: i64 = {
        let rows = kg
            .list_entities_by_file(&file_path_str)
            .expect("list_entities_by_file must succeed");
        rows.iter()
            .find(|e| e.name == "evaluate" && e.kind == "function")
            .and_then(|e| e.id)
            .expect("fixture must contain an `evaluate` function row")
    };

    // Upsert a synthetic caller entity and `calls` relation targeting
    // `evaluate` so the happy-path response has a non-empty callers
    // list — this is the "immediate callers" field from the P1-W4-F05
    // description.
    let caller_qualified_name = "tests::synthetic::caller_of_evaluate@1:1".to_owned();
    let caller = Entity {
        id: None,
        kind: "function".to_owned(),
        name: "caller_of_evaluate".to_owned(),
        qualified_name: Some(caller_qualified_name.clone()),
        file_path: "tests/synthetic.rs".to_owned(),
        start_line: Some(1),
        end_line: Some(3),
        signature: Some("fn caller_of_evaluate()".to_owned()),
        doc_comment: None,
        language: Some("rust".to_owned()),
        t_valid_from: Some(TREE_SITTER_VALID_FROM.to_owned()),
        t_valid_to: None,
        importance: 0.5,
        source_tool: Some(SOURCE_TOOL.to_owned()),
        source_hash: Some("synthetic".to_owned()),
    };
    let caller_id = kg
        .upsert_entity(&caller)
        .expect("synthetic caller upsert must succeed");
    let relation = Relation {
        id: None,
        source_id: caller_id,
        target_id: evaluate_id,
        kind: "calls".to_owned(),
        weight: 1.0,
        t_valid_from: Some(TREE_SITTER_VALID_FROM.to_owned()),
        t_valid_to: None,
        source_tool: Some(SOURCE_TOOL.to_owned()),
        source_evidence: Some("synthetic test edge".to_owned()),
        confidence: 1.0,
    };
    kg.upsert_relation(&relation)
        .expect("synthetic calls relation upsert must succeed");

    let kg_arc = Arc::new(Mutex::new(kg));
    let server = McpServer::with_knowledge_graph(Arc::clone(&kg_arc));
    (
        server,
        kg_arc,
        tmp,
        file_path_str,
        evaluate_id,
        caller_qualified_name,
    )
}

/// Frozen acceptance selector for feature `P1-W4-F05` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon server::test_find_definition_tool`.
///
/// Exercises the full `tools/call` dispatch for `find_definition`
/// against a real tree-sitter → KG pipeline-populated database, with
/// a synthetic `calls` edge in place so the response's callers list
/// is non-empty.  Asserts:
///
/// 1. The JSON-RPC envelope is well-formed (`jsonrpc == "2.0"`,
///    matching `id`, no `error` field).
/// 2. `result._meta.found == true`.
/// 3. `result._meta.tool == "find_definition"` and
///    `result._meta.source == "tree-sitter+kg"`.
/// 4. `result._meta.file_path` matches the ingested fixture path.
/// 5. `result._meta.start_line` is Some positive integer (tree-sitter
///    reports 1-based line numbers).
/// 6. `result._meta.callers` contains an entry whose `qualified_name`
///    matches the synthetic caller seeded in the fixture helper.
/// 7. `result.isError == false`.
/// 8. The phase-1 stub marker `_meta.not_yet_implemented` is
///    **absent** — proving the handler escaped the stub path.
#[cfg(test)]
#[tokio::test]
async fn test_find_definition_tool() {
    let (server, _kg, _tmp, file_path_str, _evaluate_id, caller_qualified_name) =
        build_find_definition_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 42,
        "method": "tools/call",
        "params": {
            "name": "find_definition",
            "arguments": { "name": "evaluate", "reason": "acceptance test" }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert_eq!(
        response.get("jsonrpc").and_then(Value::as_str),
        Some(JSONRPC_VERSION),
        "response must carry jsonrpc == \"2.0\": {response}"
    );
    assert_eq!(
        response.get("id").and_then(Value::as_i64),
        Some(42),
        "response id must echo request id: {response}"
    );
    assert!(
        response.get("error").is_none(),
        "response must not carry an error envelope: {response}"
    );

    let meta = response
        .pointer("/result/_meta")
        .expect("response must carry result._meta");
    assert_eq!(
        meta.get("found").and_then(Value::as_bool),
        Some(true),
        "_meta.found must be true for a resolved symbol: {response}"
    );
    assert_eq!(
        meta.get("tool").and_then(Value::as_str),
        Some("find_definition"),
        "_meta.tool must be \"find_definition\": {response}"
    );
    assert_eq!(
        meta.get("source").and_then(Value::as_str),
        Some("tree-sitter+kg"),
        "_meta.source must be \"tree-sitter+kg\": {response}"
    );
    assert_eq!(
        meta.get("file_path").and_then(Value::as_str),
        Some(file_path_str.as_str()),
        "_meta.file_path must match the ingested fixture path: {response}"
    );
    let start_line = meta
        .get("start_line")
        .and_then(Value::as_i64)
        .expect("_meta.start_line must be a positive integer");
    assert!(
        start_line > 0,
        "_meta.start_line must be 1-based positive: got {start_line}"
    );

    let callers = meta
        .get("callers")
        .and_then(Value::as_array)
        .expect("_meta.callers must be a JSON array");
    assert!(
        callers.iter().any(|c| {
            c.get("qualified_name").and_then(Value::as_str) == Some(caller_qualified_name.as_str())
        }),
        "_meta.callers must contain synthetic caller {caller_qualified_name:?}: got {callers:?}"
    );

    assert_eq!(
        response.pointer("/result/isError").and_then(Value::as_bool),
        Some(false),
        "result.isError must be false on the happy path: {response}"
    );
    assert!(
        response
            .pointer("/result/_meta/not_yet_implemented")
            .is_none(),
        "result._meta.not_yet_implemented must be ABSENT — the handler \
         must have escaped the phase-1 stub path: {response}"
    );
}

/// Negative path: a `find_definition` call for a symbol absent from
/// the knowledge graph must return a well-formed JSON-RPC response
/// envelope with `_meta.found == false` and `isError == false` — NOT
/// a JSON-RPC error, because "symbol not found" is a successful
/// lookup that returned zero rows.
#[cfg(test)]
#[tokio::test]
async fn test_find_definition_tool_unknown_symbol() {
    let (server, _kg, _tmp, _file_path, _id, _caller) = build_find_definition_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "find_definition",
            "arguments": { "name": "this_symbol_does_not_exist_anywhere" }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "unknown-symbol path must not return JSON-RPC error: {response}"
    );
    assert_eq!(
        response
            .pointer("/result/_meta/found")
            .and_then(Value::as_bool),
        Some(false),
        "_meta.found must be false for unresolved symbol: {response}"
    );
    assert_eq!(
        response.pointer("/result/isError").and_then(Value::as_bool),
        Some(false),
        "result.isError must be false (this is a well-formed zero-row \
         result, not an error): {response}"
    );
}

/// Negative path: a `find_definition` call missing the required
/// `name` argument must return a JSON-RPC error envelope with
/// `code == -32602` (Invalid params).
#[cfg(test)]
#[tokio::test]
async fn test_find_definition_tool_missing_name_param() {
    let (server, _kg, _tmp, _file_path, _id, _caller) = build_find_definition_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 99,
        "method": "tools/call",
        "params": {
            "name": "find_definition",
            "arguments": {}
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    let error = response
        .get("error")
        .expect("missing `name` arg must produce JSON-RPC error envelope");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(-32602),
        "missing param error code must be -32602 (Invalid params): {response}"
    );
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .expect("error envelope must carry a string message");
    assert!(
        message.contains("name"),
        "error message should reference missing `name` argument: {message:?}"
    );
    assert!(
        response.get("result").is_none(),
        "JSON-RPC error response must not also carry `result`: {response}"
    );
}

// ── get_conventions acceptance tests (P1-W4-F10) ─────────────────────────
//
// Per DEC-0005, these live at module root so the frozen acceptance
// selector `server::test_get_conventions_tool` resolves to
// `ucil_daemon::server::test_get_conventions_tool` for
// `cargo nextest run -p ucil-daemon server::test_get_conventions_tool`
// without an intermediate `tests::` segment.

/// Build an `McpServer::with_knowledge_graph`-backed server populated
/// with two real `conventions` rows inserted directly through
/// [`KnowledgeGraph::insert_convention`] (one `category='naming'`,
/// one `category='error_handling'`).
///
/// Returns `(server, kg_arc, tmp_dir)` so the caller can assert the
/// response envelope without worrying about the tempdir being dropped.
#[cfg(test)]
fn build_get_conventions_fixture() -> (
    McpServer,
    Arc<Mutex<ucil_core::KnowledgeGraph>>,
    tempfile::TempDir,
) {
    use ucil_core::{Convention, KnowledgeGraph};

    let tmp = tempfile::TempDir::new().expect("tempdir must be creatable");
    let kg_path = tmp.path().join("knowledge.db");
    let mut kg = KnowledgeGraph::open(&kg_path).expect("KnowledgeGraph::open must succeed");

    let naming = Convention {
        id: None,
        category: "naming".to_owned(),
        pattern: "snake_case for functions".to_owned(),
        examples: Some("fn parse_file() {}".to_owned()),
        counter_examples: Some("fn ParseFile() {}".to_owned()),
        confidence: 0.9,
        evidence_count: 42,
        t_ingested_at: String::new(),
        last_verified: None,
        scope: "project".to_owned(),
    };
    let error_handling = Convention {
        id: None,
        category: "error_handling".to_owned(),
        pattern: "thiserror on all library errors".to_owned(),
        examples: None,
        counter_examples: None,
        confidence: 0.8,
        evidence_count: 17,
        t_ingested_at: String::new(),
        last_verified: None,
        scope: "project".to_owned(),
    };
    kg.insert_convention(&naming)
        .expect("naming insert must succeed");
    kg.insert_convention(&error_handling)
        .expect("error_handling insert must succeed");

    let kg_arc = Arc::new(Mutex::new(kg));
    let server = McpServer::with_knowledge_graph(Arc::clone(&kg_arc));
    (server, kg_arc, tmp)
}

/// Frozen acceptance selector for feature `P1-W4-F10` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon server::test_get_conventions_tool`.
///
/// Exercises the full `tools/call` dispatch for `get_conventions`
/// against a real `KnowledgeGraph::open` + `insert_convention`
/// populated database.  Asserts both an unfiltered and a
/// category-filtered call:
///
/// 1. Unfiltered (empty `arguments`) — `_meta.count == 2`,
///    `_meta.category == null`, `_meta.conventions` has two entries
///    in id-asc order with correct `category`/`pattern` round-tripped.
/// 2. `arguments.category == "naming"` — `_meta.count == 1`,
///    `_meta.category == "naming"`, `_meta.conventions[0].pattern ==
///    "snake_case for functions"`.
/// 3. Both responses carry `isError == false`,
///    `_meta.tool == "get_conventions"`, `_meta.source == "kg"`, and
///    the phase-1 stub marker `_meta.not_yet_implemented` is
///    **absent** — proving the handler escaped the stub path.
//
// `too_many_lines` allowed because the test drives TWO complete
// request/response round-trips (unfiltered + filtered) plus the
// assertions for each — mirrors the `test_all_22_tools_registered`
// shape that already carries the same allow.
#[cfg(test)]
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_get_conventions_tool() {
    let (server, _kg, _tmp) = build_get_conventions_fixture();

    // ── Unfiltered call ─────────────────────────────────────────
    let request_all = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "get_conventions",
            "arguments": {}
        }
    })
    .to_string();
    let response_all = server.handle_line(&request_all).await;

    assert_eq!(
        response_all.get("jsonrpc").and_then(Value::as_str),
        Some(JSONRPC_VERSION),
        "response must carry jsonrpc == \"2.0\": {response_all}"
    );
    assert_eq!(
        response_all.get("id").and_then(Value::as_i64),
        Some(1),
        "response id must echo request id: {response_all}"
    );
    assert!(
        response_all.get("error").is_none(),
        "unfiltered response must not carry an error envelope: {response_all}"
    );

    let meta_all = response_all
        .pointer("/result/_meta")
        .expect("unfiltered response must carry result._meta");
    assert_eq!(
        meta_all.get("tool").and_then(Value::as_str),
        Some("get_conventions"),
        "_meta.tool must be \"get_conventions\": {response_all}"
    );
    assert_eq!(
        meta_all.get("source").and_then(Value::as_str),
        Some("kg"),
        "_meta.source must be \"kg\": {response_all}"
    );
    assert_eq!(
        meta_all.get("count").and_then(Value::as_i64),
        Some(2),
        "_meta.count must be 2 (two inserted rows): {response_all}"
    );
    assert!(
        meta_all.get("category").is_some_and(Value::is_null),
        "_meta.category must be null when unfiltered: {response_all}"
    );
    let conventions_all = meta_all
        .get("conventions")
        .and_then(Value::as_array)
        .expect("_meta.conventions must be a JSON array");
    assert_eq!(
        conventions_all.len(),
        2,
        "two inserted rows must come back: {conventions_all:?}"
    );
    assert_eq!(
        conventions_all[0].get("category").and_then(Value::as_str),
        Some("naming"),
        "first row category must be naming: {conventions_all:?}"
    );
    assert_eq!(
        conventions_all[0].get("pattern").and_then(Value::as_str),
        Some("snake_case for functions"),
        "first row pattern round-tripped: {conventions_all:?}"
    );
    assert_eq!(
        conventions_all[1].get("category").and_then(Value::as_str),
        Some("error_handling"),
        "second row category must be error_handling: {conventions_all:?}"
    );
    assert_eq!(
        conventions_all[1].get("pattern").and_then(Value::as_str),
        Some("thiserror on all library errors"),
        "second row pattern round-tripped: {conventions_all:?}"
    );

    assert_eq!(
        response_all
            .pointer("/result/isError")
            .and_then(Value::as_bool),
        Some(false),
        "result.isError must be false on the happy path: {response_all}"
    );
    assert!(
        response_all
            .pointer("/result/_meta/not_yet_implemented")
            .is_none(),
        "result._meta.not_yet_implemented must be ABSENT — the handler \
         must have escaped the phase-1 stub path: {response_all}"
    );

    // ── Filtered call (category = "naming") ─────────────────────
    let request_naming = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "get_conventions",
            "arguments": { "category": "naming" }
        }
    })
    .to_string();
    let response_naming = server.handle_line(&request_naming).await;

    assert!(
        response_naming.get("error").is_none(),
        "filtered response must not carry an error envelope: {response_naming}"
    );
    let meta_naming = response_naming
        .pointer("/result/_meta")
        .expect("filtered response must carry result._meta");
    assert_eq!(
        meta_naming.get("count").and_then(Value::as_i64),
        Some(1),
        "_meta.count must be 1 when filtered to naming: {response_naming}"
    );
    assert_eq!(
        meta_naming.get("category").and_then(Value::as_str),
        Some("naming"),
        "_meta.category must echo the filter: {response_naming}"
    );
    let conventions_naming = meta_naming
        .get("conventions")
        .and_then(Value::as_array)
        .expect("_meta.conventions must be a JSON array (filtered)");
    assert_eq!(
        conventions_naming.len(),
        1,
        "only the naming row matches: {conventions_naming:?}"
    );
    assert_eq!(
        conventions_naming[0].get("pattern").and_then(Value::as_str),
        Some("snake_case for functions"),
        "filtered pattern must match naming row: {conventions_naming:?}"
    );
    assert_eq!(
        response_naming
            .pointer("/result/isError")
            .and_then(Value::as_bool),
        Some(false),
        "result.isError must be false on the filtered happy path: {response_naming}"
    );
}

/// Empty-table path: a `get_conventions` call against a KG with zero
/// conventions must return `_meta.count == 0`,
/// `_meta.conventions == []`, `isError == false`, and a
/// human-readable "no conventions yet" text — the master-plan §3.2
/// row-7 "empty list if none yet extracted" contract.
#[cfg(test)]
#[tokio::test]
async fn test_get_conventions_tool_empty() {
    use ucil_core::KnowledgeGraph;

    let tmp = tempfile::TempDir::new().expect("tempdir must be creatable");
    let kg = KnowledgeGraph::open(&tmp.path().join("knowledge.db"))
        .expect("KnowledgeGraph::open must succeed");
    let kg_arc = Arc::new(Mutex::new(kg));
    let server = McpServer::with_knowledge_graph(Arc::clone(&kg_arc));

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "get_conventions",
            "arguments": {}
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "empty-table call must not return JSON-RPC error: {response}"
    );
    let meta = response
        .pointer("/result/_meta")
        .expect("empty-table response must carry result._meta");
    assert_eq!(
        meta.get("count").and_then(Value::as_i64),
        Some(0),
        "_meta.count must be 0 for empty table: {response}"
    );
    let conventions = meta
        .get("conventions")
        .and_then(Value::as_array)
        .expect("_meta.conventions must be a JSON array even when empty");
    assert!(
        conventions.is_empty(),
        "_meta.conventions must be an empty array for empty table: {conventions:?}"
    );
    assert_eq!(
        response.pointer("/result/isError").and_then(Value::as_bool),
        Some(false),
        "result.isError must be false on empty-table path: {response}"
    );
    let text = response
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .expect("content[0].text must be a string");
    assert!(
        text.contains("no conventions yet"),
        "empty-table text should say \"no conventions yet\": got {text:?}"
    );
}

/// Stub-path regression: a `get_conventions` call on a server built
/// via `McpServer::new()` (no KG attached) must still return the
/// phase-1 `_meta.not_yet_implemented: true` stub envelope —
/// preserving invariant #9 from `ucil-build/phase-log/01-phase-1/
/// CLAUDE.md` for the "no KG" deployment shape.
#[cfg(test)]
#[tokio::test]
async fn test_get_conventions_tool_no_kg_returns_stub() {
    let server = McpServer::new();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "get_conventions",
            "arguments": {}
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "stub path must not return JSON-RPC error: {response}"
    );
    assert_eq!(
        response
            .pointer("/result/_meta/not_yet_implemented")
            .and_then(Value::as_bool),
        Some(true),
        "no-KG call must return _meta.not_yet_implemented == true \
         (phase-1 invariant #9): {response}"
    );
    assert_eq!(
        response
            .pointer("/result/_meta/tool")
            .and_then(Value::as_str),
        Some("get_conventions"),
        "stub envelope must echo tool name: {response}"
    );
}

/// Negative path: a `get_conventions` call whose `arguments.category`
/// is a non-string JSON value (e.g. number) must return a JSON-RPC
/// error envelope with `code == -32602` (Invalid params).
#[cfg(test)]
#[tokio::test]
async fn test_get_conventions_tool_non_string_category() {
    let (server, _kg, _tmp) = build_get_conventions_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "get_conventions",
            "arguments": { "category": 123 }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    let error = response
        .get("error")
        .expect("non-string category must produce JSON-RPC error envelope");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(-32602),
        "invalid-param error code must be -32602 (Invalid params): {response}"
    );
    assert!(
        response.get("result").is_none(),
        "JSON-RPC error response must not also carry `result`: {response}"
    );
}

// ── search_code acceptance tests (P1-W5-F09) ─────────────────────────────
//
// Per DEC-0005 / DEC-0007 and the WO-0006 test-selector lesson, these
// acceptance tests live at module root (NOT under `mod tests { … }`) so
// the frozen acceptance selector `server::test_search_code_basic` resolves
// to `ucil_daemon::server::test_search_code_basic` for
// `cargo nextest run -p ucil-daemon server::test_search_code_basic`
// without an intermediate `tests::` segment.

/// Build a `search_code` test fixture.
///
/// Creates two `tempfile::TempDir` trees: one hosting the `SQLite`
/// knowledge graph (`.db`) and one hosting the source-tree that the
/// in-process `ripgrep` walker will scan.  Writes `src/util.rs` with
/// the `banana_split` function so both halves of the handler (symbol
/// and text) have a real hit to report.  Returns
/// `(server, _kg_arc, _kg_tmp, project_tmp, project_root_string)` so
/// callers can build request JSON against the project path.
///
/// The KG is deliberately kept in a separate tempdir so the scanner
/// does NOT walk over the `.db` file (the walker would happily recurse
/// into it, but the `.db` is binary and gets dropped by
/// `BinaryDetection::quit(0x00)` — still, segregating avoids surprises
/// for future maintainers).
#[cfg(test)]
#[allow(clippy::type_complexity)]
fn build_search_code_fixture() -> (
    McpServer,
    Arc<Mutex<ucil_core::KnowledgeGraph>>,
    tempfile::TempDir,
    tempfile::TempDir,
    String,
    String,
) {
    use std::fs;

    use ucil_core::{Entity, KnowledgeGraph};

    let kg_tmp = tempfile::TempDir::new().expect("kg tempdir must be creatable");
    let project_tmp = tempfile::TempDir::new().expect("project tempdir must be creatable");

    // Write a real Rust source file so `grep-searcher` has something
    // to match on a banana_split query.
    let src_dir = project_tmp.path().join("src");
    fs::create_dir_all(&src_dir).expect("src/ must be creatable under project root");
    let util_rs = src_dir.join("util.rs");
    fs::write(&util_rs, "pub fn banana_split(x: u32) -> u32 { x + 1 }\n")
        .expect("util.rs must be writable");

    // Open the KG and upsert a matching entity row so the symbol half
    // also hits on `banana_split`.  `file_path` is the absolute path
    // to `util.rs` so the merged record shares its `(file_path, line)`
    // key with the text walker's hit.
    let util_abs = util_rs
        .canonicalize()
        .expect("util.rs path must canonicalize");
    let util_abs_str = util_abs.display().to_string();
    let mut kg = KnowledgeGraph::open(&kg_tmp.path().join("knowledge.db"))
        .expect("KnowledgeGraph::open must succeed");
    let entity = Entity {
        id: None,
        kind: "function".to_owned(),
        name: "banana_split".to_owned(),
        qualified_name: Some("project::util::banana_split".to_owned()),
        file_path: util_abs_str.clone(),
        start_line: Some(1),
        end_line: Some(1),
        signature: Some("fn banana_split(x: u32) -> u32".to_owned()),
        doc_comment: None,
        language: Some("rust".to_owned()),
        t_valid_from: Some(crate::executor::TREE_SITTER_VALID_FROM.to_owned()),
        t_valid_to: None,
        importance: 0.5,
        source_tool: Some(crate::executor::SOURCE_TOOL.to_owned()),
        source_hash: Some("synthetic".to_owned()),
    };
    kg.upsert_entity(&entity)
        .expect("banana_split entity upsert must succeed");

    let project_root_str = project_tmp.path().display().to_string();
    let kg_arc = Arc::new(Mutex::new(kg));
    let server = McpServer::with_knowledge_graph(Arc::clone(&kg_arc));
    (
        server,
        kg_arc,
        kg_tmp,
        project_tmp,
        project_root_str,
        util_abs_str,
    )
}

/// Frozen acceptance selector for feature `P1-W5-F09` — see
/// `ucil-build/feature-list.json` entry
/// `-p ucil-daemon server::test_search_code_basic`.
///
/// Exercises the full `tools/call` dispatch for `search_code` against a
/// real `KnowledgeGraph::open` + `upsert_entity` on a temp `SQLite` file
/// AND a real in-process `ignore` + `grep-searcher` + `grep-regex`
/// walker pass on a `tempfile::TempDir` populated with a Rust source
/// file.  No mocks of `SQLite`, `ignore`, or `grep-searcher` — both
/// halves run against real state per the anti-laziness contract.
///
/// Asserts (per WO-0035 `scope_in` bullet 9):
///
/// 1. JSON-RPC envelope is well-formed (`jsonrpc == "2.0"`, matching
///    `id`, no `error`).
/// 2. `_meta.tool == "search_code"` and
///    `_meta.source == "tree-sitter+ripgrep"`.
/// 3. `_meta.count >= 1` (at least one merged result).
/// 4. `_meta.symbol_match_count == 1` (one KG row hit).
/// 5. `_meta.text_match_count >= 1` (at least one `ripgrep` hit).
/// 6. At least one result has `source == "both"` (the symbol row at
///    `util.rs:1` and the text row at `util.rs:1` collapsed).
/// 7. `_meta.results[0].preview` contains `"banana_split"`.
/// 8. `result.isError == false`.
/// 9. The phase-1 stub marker `_meta.not_yet_implemented` is absent.
#[cfg(test)]
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_search_code_basic() {
    let (server, _kg, _kg_tmp, _project_tmp, project_root, _util_abs) = build_search_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 101,
        "method": "tools/call",
        "params": {
            "name": "search_code",
            "arguments": {
                "query": "banana_split",
                "root": project_root,
                "max_results": 50
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert_eq!(
        response.get("jsonrpc").and_then(Value::as_str),
        Some(JSONRPC_VERSION),
        "response must carry jsonrpc == \"2.0\": {response}"
    );
    assert_eq!(
        response.get("id").and_then(Value::as_i64),
        Some(101),
        "response id must echo request id: {response}"
    );
    assert!(
        response.get("error").is_none(),
        "response must not carry an error envelope: {response}"
    );

    let meta = response
        .pointer("/result/_meta")
        .expect("response must carry result._meta");
    assert_eq!(
        meta.get("tool").and_then(Value::as_str),
        Some("search_code"),
        "_meta.tool must be \"search_code\": {response}"
    );
    assert_eq!(
        meta.get("source").and_then(Value::as_str),
        Some("tree-sitter+ripgrep"),
        "_meta.source must be \"tree-sitter+ripgrep\": {response}"
    );

    let count = meta
        .get("count")
        .and_then(Value::as_i64)
        .expect("_meta.count must be an integer");
    assert!(
        count >= 1,
        "_meta.count must be >= 1 (symbol + text merged): got {count} — {response}"
    );
    assert_eq!(
        meta.get("symbol_match_count").and_then(Value::as_i64),
        Some(1),
        "_meta.symbol_match_count must be 1 (one KG row hit): {response}"
    );
    let text_match_count = meta
        .get("text_match_count")
        .and_then(Value::as_i64)
        .expect("_meta.text_match_count must be an integer");
    assert!(
        text_match_count >= 1,
        "_meta.text_match_count must be >= 1 (ripgrep hit): got {text_match_count} — {response}"
    );

    let results = meta
        .get("results")
        .and_then(Value::as_array)
        .expect("_meta.results must be a JSON array");
    assert!(
        !results.is_empty(),
        "_meta.results must contain at least one row: {response}"
    );
    let any_both = results
        .iter()
        .any(|r| r.get("source").and_then(Value::as_str) == Some("both"));
    assert!(
        any_both,
        "at least one result must have source == \"both\" (symbol + text merge at util.rs:1): {results:?}"
    );
    let first_preview = results[0]
        .get("preview")
        .and_then(Value::as_str)
        .expect("_meta.results[0].preview must be a string");
    assert!(
        first_preview.contains("banana_split"),
        "_meta.results[0].preview must mention banana_split: got {first_preview:?}"
    );

    assert_eq!(
        response.pointer("/result/isError").and_then(Value::as_bool),
        Some(false),
        "result.isError must be false on the happy path: {response}"
    );
    assert!(
        response
            .pointer("/result/_meta/not_yet_implemented")
            .is_none(),
        "result._meta.not_yet_implemented must be ABSENT — the handler \
         must have escaped the phase-1 stub path: {response}"
    );
}

// ── Module-level G2-fused acceptance test ───────────────────────────────────
//
// Frozen selectors per `feature-list.json:P2-W7-F06.acceptance_tests[0]
// .selector = "-p ucil-daemon server::test_search_code_fused"`.  Per
// `DEC-0007` the test lives at MODULE ROOT (NOT inside a `mod tests`
// block) so the nextest selector resolves directly.

/// `TestG2SourceProvider` — a UCIL-internal trait substitute used by
/// the frozen `test_search_code_fused` acceptance test.  Per `DEC-0008`
/// §4 / WO-0048 lessons line 363 this is NOT a critical-dep
/// substitute: [`G2SourceProvider`] is a UCIL-owned seam, and the
/// acceptance test exercises the [`run_g2_fan_out`] /
/// [`fuse_g2_rrf`] integration logic against canned per-source
/// inputs to prove the handler wires the fusion math correctly.
#[cfg(test)]
struct TestG2SourceProvider {
    source: ucil_core::G2Source,
    canned_hits: Vec<ucil_core::G2Hit>,
}

#[cfg(test)]
#[async_trait::async_trait]
impl crate::g2_search::G2SourceProvider for TestG2SourceProvider {
    fn source(&self) -> ucil_core::G2Source {
        self.source
    }

    async fn execute(
        &self,
        _query: &str,
        _root: &Path,
        _max_results: usize,
    ) -> Result<ucil_core::G2SourceResults, crate::g2_search::G2SearchError> {
        Ok(ucil_core::G2SourceResults {
            source: self.source,
            hits: self.canned_hits.clone(),
        })
    }
}

/// Build the canned [`G2SourceFactory`] used by
/// `test_search_code_fused`.
///
/// Returns three providers in `[Probe, Ripgrep, Lancedb]` order with
/// the WO-0063 acceptance-criteria-prescribed canned hit sets:
///
/// * Probe — `[(util.rs, 10, 20, "fn foo() // probe"),
///            (util.rs, 30, 40, "fn baz() // probe")]`
/// * Ripgrep — `[(util.rs, 10, 20, "fn foo() // ripgrep"),
///              (other.rs, 5, 5, "// other ripgrep")]`
/// * Lancedb — `[]` (per `DEC-0015` D3 default-empty path)
#[cfg(test)]
fn build_test_g2_factory() -> std::sync::Arc<crate::g2_search::G2SourceFactory> {
    use ucil_core::{G2Hit, G2Source};

    let probe_hits = vec![
        G2Hit {
            file_path: PathBuf::from("util.rs"),
            start_line: 10,
            end_line: 20,
            snippet: "fn foo() // probe".to_owned(),
            score: 0.95,
        },
        G2Hit {
            file_path: PathBuf::from("util.rs"),
            start_line: 30,
            end_line: 40,
            snippet: "fn baz() // probe".to_owned(),
            score: 0.80,
        },
    ];
    let ripgrep_hits = vec![
        G2Hit {
            file_path: PathBuf::from("util.rs"),
            start_line: 10,
            end_line: 20,
            snippet: "fn foo() // ripgrep".to_owned(),
            score: 0.85,
        },
        G2Hit {
            file_path: PathBuf::from("other.rs"),
            start_line: 5,
            end_line: 5,
            snippet: "// other ripgrep".to_owned(),
            score: 0.60,
        },
    ];

    std::sync::Arc::new(crate::g2_search::G2SourceFactory::from_builder(move || {
        let probe_clone = probe_hits.clone();
        let ripgrep_clone = ripgrep_hits.clone();
        vec![
            Box::new(TestG2SourceProvider {
                source: G2Source::Probe,
                canned_hits: probe_clone,
            }),
            Box::new(TestG2SourceProvider {
                source: G2Source::Ripgrep,
                canned_hits: ripgrep_clone,
            }),
            Box::new(TestG2SourceProvider {
                source: G2Source::Lancedb,
                canned_hits: Vec::new(),
            }),
        ]
    }))
}

/// Frozen acceptance selector for feature `P2-W7-F06`.
///
/// Per `feature-list.json` entry `-p ucil-daemon
/// server::test_search_code_fused`.  Per `DEC-0007` this lives at
/// MODULE ROOT — NOT wrapped inside `mod tests {}` — so the
/// `cargo test` selector resolves directly.
///
/// Exercises the full `tools/call` dispatch for `search_code` against:
///
/// * A real `KnowledgeGraph::open` + `upsert_entity` on a temp `SQLite`
///   file (the same setup `test_search_code_basic` uses).
/// * A real in-process `text_search` walker pass on the same temp
///   project.
/// * A canned `G2SourceFactory` whose three providers are
///   [`TestG2SourceProvider`] impls returning the WO-0063
///   acceptance-criteria-prescribed canned hit sets per source.
///
/// The handler runs the legacy `P1-W5-F09` KG+ripgrep merge AND the
/// new G2 fan-out + RRF fusion in the same call; the test asserts:
///
/// 1. JSON-RPC envelope is well-formed.
/// 2. Every legacy `_meta` field from `P1-W5-F09` is preserved
///    byte-shape (per `DEC-0015` D1 — additive evolution).
/// 3. `_meta.g2_fused.hits` is a JSON array of length 3.
/// 4. RRF ranking applied: `(util.rs, 10, 20)` is `hits[0]` with
///    `contributing_sources == [Probe, Ripgrep]`.
/// 5. Snippet selection: `hits[0].snippet == "fn foo() // probe"`
///    (Probe×2.0 wins on weight per WO-0056 line 525).
/// 6. `per_source_ranks` provenance preserved: `[(Probe, 1),
///    (Ripgrep, 1)]` (multiset; order-insensitive).
///
/// # Panics
///
/// Panics on any failed sub-assertion — the panic message is
/// operator-actionable (quotes the JSON content on failure).
#[cfg(test)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)]
pub async fn test_search_code_fused() {
    let (_legacy_server, kg, _kg_tmp, _project_tmp, project_root, _util_abs) =
        build_search_code_fixture();
    let factory = build_test_g2_factory();
    let server = McpServer::with_knowledge_graph(Arc::clone(&kg)).with_g2_sources(factory);

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 301,
        "method": "tools/call",
        "params": {
            "name": "search_code",
            "arguments": {
                "query": "banana_split",
                "root": project_root,
                "max_results": 50
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    // ── Sub-assertion 1: envelope well-formed ──────────────────────
    assert_eq!(
        response.get("jsonrpc").and_then(Value::as_str),
        Some(JSONRPC_VERSION),
        "(1) response must carry jsonrpc == \"2.0\": {response}"
    );
    assert_eq!(
        response.get("id").and_then(Value::as_i64),
        Some(301),
        "(1) response id must echo 301: {response}"
    );
    assert!(
        response.get("error").is_none(),
        "(1) response must not carry an error envelope: {response}"
    );
    assert_eq!(
        response.pointer("/result/isError").and_then(Value::as_bool),
        Some(false),
        "(1) result.isError must be false on the happy path: {response}"
    );

    let meta = response
        .pointer("/result/_meta")
        .expect("(1) response must carry result._meta");

    // ── Sub-assertion 2: legacy fields preserved ───────────────────
    assert_eq!(
        meta.get("tool").and_then(Value::as_str),
        Some("search_code"),
        "(2) _meta.tool must be \"search_code\": {response}"
    );
    assert_eq!(
        meta.get("source").and_then(Value::as_str),
        Some("tree-sitter+ripgrep"),
        "(2) _meta.source must be \"tree-sitter+ripgrep\": {response}"
    );
    assert!(
        meta.get("symbol_match_count")
            .and_then(Value::as_i64)
            .is_some(),
        "(2) _meta.symbol_match_count must be a JSON number: {response}"
    );
    assert!(
        meta.get("text_match_count")
            .and_then(Value::as_i64)
            .is_some(),
        "(2) _meta.text_match_count must be a JSON number: {response}"
    );
    assert!(
        meta.get("results").and_then(Value::as_array).is_some(),
        "(2) _meta.results must be a JSON array: {response}"
    );

    // ── Sub-assertion 3: g2_fused present ──────────────────────────
    let fused = meta
        .get("g2_fused")
        .expect("(3) _meta.g2_fused must be present when factory is attached");
    let fused_hits = fused
        .get("hits")
        .and_then(Value::as_array)
        .expect("(3) _meta.g2_fused.hits must be a JSON array");
    assert_eq!(
        fused_hits.len(),
        3,
        "(3) _meta.g2_fused.hits must have 3 entries (Probe×2 + Ripgrep×2 \
         merge to 3 unique fused hits — (util.rs, 10, 20) Probe+Ripgrep, \
         (util.rs, 30, 40) Probe-only, (other.rs, 5, 5) Ripgrep-only); \
         got {} entries: {fused:?}",
        fused_hits.len(),
    );

    // ── Sub-assertion 4: RRF ranking applied ───────────────────────
    let top = &fused_hits[0];
    assert_eq!(
        top.get("file_path").and_then(Value::as_str),
        Some("util.rs"),
        "(4) hits[0].file_path must be \"util.rs\"; got {top:?}"
    );
    assert_eq!(
        top.get("start_line").and_then(Value::as_u64),
        Some(10),
        "(4) hits[0].start_line must be 10; got {top:?}"
    );
    let contributing = top
        .get("contributing_sources")
        .and_then(Value::as_array)
        .expect("(4) hits[0].contributing_sources must be a JSON array");
    let contrib_strs: Vec<&str> = contributing.iter().filter_map(Value::as_str).collect();
    assert_eq!(
        contrib_strs,
        vec!["Probe", "Ripgrep"],
        "(4) hits[0].contributing_sources must be [\"Probe\", \"Ripgrep\"] \
         (descending rrf_weight per WO-0056 lines 286-296); got {contrib_strs:?} \
         — full hits[0]={top:?}"
    );

    // ── Sub-assertion 5: snippet selection ────────────────────────
    assert_eq!(
        top.get("snippet").and_then(Value::as_str),
        Some("fn foo() // probe"),
        "(5) hits[0].snippet must be \"fn foo() // probe\" (Probe×2.0 \
         wins on weight per WO-0056 line 525); got {top:?}"
    );

    // ── Sub-assertion 6: per_source_ranks captured ────────────────
    let per_source_ranks = top
        .get("per_source_ranks")
        .and_then(Value::as_array)
        .expect("(6) hits[0].per_source_ranks must be a JSON array");
    let mut rank_pairs: Vec<(String, u64)> = per_source_ranks
        .iter()
        .filter_map(|v| {
            let arr = v.as_array()?;
            let src = arr.first()?.as_str()?.to_owned();
            let rank = arr.get(1)?.as_u64()?;
            Some((src, rank))
        })
        .collect();
    rank_pairs.sort();
    let mut expected = vec![("Probe".to_owned(), 1_u64), ("Ripgrep".to_owned(), 1_u64)];
    expected.sort();
    assert_eq!(
        rank_pairs, expected,
        "(6) hits[0].per_source_ranks must contain {{(Probe, 1), \
         (Ripgrep, 1)}} as a multiset; got {rank_pairs:?} — full \
         hits[0]={top:?}"
    );
}

/// Negative path for `P2-W7-F06`: legacy preservation when the
/// `G2SourceFactory` is absent.
///
/// When no factory is attached (i.e. [`McpServer::new()`] or
/// [`McpServer::with_knowledge_graph`] without a subsequent
/// `.with_g2_sources(factory)` call), the `_meta.g2_fused` field MUST
/// be absent while every legacy `_meta` field stays byte-identical
/// per `DEC-0015` D1.  Proves the `Option<Arc<G2SourceFactory>>::None`
/// path is the legacy-shape path.
///
/// # Panics
///
/// Panics on any failed sub-assertion — the panic message quotes the
/// response JSON content on failure.
#[cfg(test)]
#[tokio::test]
pub async fn test_search_code_fused_no_factory() {
    let (server, _kg, _kg_tmp, _project_tmp, project_root, _util_abs) = build_search_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 302,
        "method": "tools/call",
        "params": {
            "name": "search_code",
            "arguments": {
                "query": "banana_split",
                "root": project_root,
                "max_results": 50
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    let meta = response
        .pointer("/result/_meta")
        .expect("response must carry result._meta");
    assert!(
        meta.get("g2_fused").is_none(),
        "_meta.g2_fused must be ABSENT when no G2SourceFactory is \
         attached (the Option::None path is the legacy-shape path per \
         DEC-0015 D1); got {response}"
    );
    assert_eq!(
        meta.get("tool").and_then(Value::as_str),
        Some("search_code"),
        "legacy _meta.tool must be preserved: {response}"
    );
    assert_eq!(
        meta.get("source").and_then(Value::as_str),
        Some("tree-sitter+ripgrep"),
        "legacy _meta.source must be preserved: {response}"
    );
    assert!(
        meta.get("results").and_then(Value::as_array).is_some(),
        "legacy _meta.results array must be preserved: {response}"
    );
}

/// Negative path: `arguments.query` is an empty string — the handler
/// must return a JSON-RPC error envelope with `code == -32602` (Invalid
/// params), per WO-0035 `scope_in` bullet 6.  Empty queries would match
/// every line in every file and are therefore not a useful operation.
#[cfg(test)]
#[tokio::test]
async fn test_search_code_tool_empty_query() {
    let (server, _kg, _kg_tmp, _project_tmp, project_root, _util_abs) = build_search_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 201,
        "method": "tools/call",
        "params": {
            "name": "search_code",
            "arguments": { "query": "", "root": project_root }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    let error = response
        .get("error")
        .expect("empty query must produce a JSON-RPC error envelope");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(-32602),
        "empty-query error code must be -32602 (Invalid params): {response}"
    );
    assert!(
        response.get("result").is_none(),
        "JSON-RPC error response must not also carry `result`: {response}"
    );
}

/// Stub-path regression: a `search_code` call on a server built via
/// `McpServer::new()` (no KG attached) must still return the phase-1
/// `_meta.not_yet_implemented: true` stub envelope — preserving
/// invariant #9 from `ucil-build/phase-log/01-phase-1/CLAUDE.md`.
#[cfg(test)]
#[tokio::test]
async fn test_search_code_tool_no_kg_returns_stub() {
    let server = McpServer::new();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 202,
        "method": "tools/call",
        "params": {
            "name": "search_code",
            "arguments": { "query": "banana_split" }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "stub path must not return JSON-RPC error: {response}"
    );
    assert_eq!(
        response
            .pointer("/result/_meta/not_yet_implemented")
            .and_then(Value::as_bool),
        Some(true),
        "no-KG call must return _meta.not_yet_implemented == true \
         (phase-1 invariant #9): {response}"
    );
    assert_eq!(
        response
            .pointer("/result/_meta/tool")
            .and_then(Value::as_str),
        Some("search_code"),
        "stub envelope must echo tool name: {response}"
    );
}

/// Negative path: `arguments.query` is a non-string JSON value (number)
/// — must produce a `-32602` Invalid-params error.
#[cfg(test)]
#[tokio::test]
async fn test_search_code_tool_non_string_query() {
    let (server, _kg, _kg_tmp, _project_tmp, project_root, _util_abs) = build_search_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 203,
        "method": "tools/call",
        "params": {
            "name": "search_code",
            "arguments": { "query": 42, "root": project_root }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    let error = response
        .get("error")
        .expect("non-string query must produce a JSON-RPC error envelope");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(-32602),
        "non-string-query error code must be -32602 (Invalid params): {response}"
    );
    assert!(
        response.get("result").is_none(),
        "JSON-RPC error response must not also carry `result`: {response}"
    );
}

/// Negative path: `arguments.root` points at a path that does not
/// exist on disk — must produce a `-32602` Invalid-params error.
#[cfg(test)]
#[tokio::test]
async fn test_search_code_tool_nonexistent_root() {
    let (server, _kg, _kg_tmp, _project_tmp, _project_root, _util_abs) =
        build_search_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 204,
        "method": "tools/call",
        "params": {
            "name": "search_code",
            "arguments": {
                "query": "banana_split",
                "root": "/this/path/does/not/exist/anywhere-on-disk"
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    let error = response
        .get("error")
        .expect("nonexistent root must produce a JSON-RPC error envelope");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(-32602),
        "nonexistent-root error code must be -32602 (Invalid params): {response}"
    );
    assert!(
        response.get("result").is_none(),
        "JSON-RPC error response must not also carry `result`: {response}"
    );
}

/// Cap path: `arguments.max_results = 10_000` — the handler must clamp
/// the count to [`SEARCH_CODE_MAX_RESULTS`] (500) rather than returning
/// 10 000 rows.  We assert the property at the result-count level: the
/// text walker can't emit more than 500 hits even if the repo had more
/// matches, and the merged list must stay under the cap.
#[cfg(test)]
#[tokio::test]
async fn test_search_code_tool_max_results_clamp() {
    let (server, _kg, _kg_tmp, _project_tmp, project_root, _util_abs) = build_search_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 205,
        "method": "tools/call",
        "params": {
            "name": "search_code",
            "arguments": {
                "query": "banana_split",
                "root": project_root,
                "max_results": 10_000_i64
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "max_results > cap must be clamped, not rejected: {response}"
    );
    let count = response
        .pointer("/result/_meta/count")
        .and_then(Value::as_i64)
        .expect("_meta.count must be present on happy path");
    let cap = i64::try_from(SEARCH_CODE_MAX_RESULTS).expect("cap fits in i64");
    assert!(
        count <= cap,
        "merged count ({count}) must be <= SEARCH_CODE_MAX_RESULTS ({cap}): {response}"
    );
}

/// Text-only path: query matches file contents but has no matching KG
/// entity — `_meta.symbol_match_count` must be 0,
/// `_meta.text_match_count >= 1`, and every result must carry
/// `source == "text"`.
#[cfg(test)]
#[tokio::test]
async fn test_search_code_tool_only_text_no_symbol() {
    use std::fs;

    use ucil_core::KnowledgeGraph;

    let kg_tmp = tempfile::TempDir::new().expect("kg tempdir must be creatable");
    let project_tmp = tempfile::TempDir::new().expect("project tempdir must be creatable");

    // Write a file containing "mango_marker" — no matching KG entity
    // will be inserted for this token.
    let src_dir = project_tmp.path().join("src");
    fs::create_dir_all(&src_dir).expect("src/ must be creatable");
    fs::write(
        src_dir.join("only_text.rs"),
        "// mango_marker appears here but no entity row points at it\n",
    )
    .expect("only_text.rs must be writable");

    let kg = KnowledgeGraph::open(&kg_tmp.path().join("knowledge.db"))
        .expect("KnowledgeGraph::open must succeed");
    let kg_arc = Arc::new(Mutex::new(kg));
    let server = McpServer::with_knowledge_graph(Arc::clone(&kg_arc));

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 206,
        "method": "tools/call",
        "params": {
            "name": "search_code",
            "arguments": {
                "query": "mango_marker",
                "root": project_tmp.path().display().to_string()
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "text-only path must not return JSON-RPC error: {response}"
    );
    let meta = response
        .pointer("/result/_meta")
        .expect("response must carry result._meta");
    assert_eq!(
        meta.get("symbol_match_count").and_then(Value::as_i64),
        Some(0),
        "_meta.symbol_match_count must be 0 (no matching KG entity): {response}"
    );
    let text_match_count = meta
        .get("text_match_count")
        .and_then(Value::as_i64)
        .expect("_meta.text_match_count must be an integer");
    assert!(
        text_match_count >= 1,
        "_meta.text_match_count must be >= 1 (ripgrep hit): got {text_match_count} — {response}"
    );
    let results = meta
        .get("results")
        .and_then(Value::as_array)
        .expect("_meta.results must be a JSON array");
    assert!(
        !results.is_empty(),
        "_meta.results must contain at least one row: {response}"
    );
    for r in results {
        assert_eq!(
            r.get("source").and_then(Value::as_str),
            Some("text"),
            "every result must have source == \"text\" (no symbol half): got {r:?}"
        );
    }
}

// ── understand_code tests (P1-W4-F09, WO-0036) ───────────────────────────────

/// Build an `McpServer::with_knowledge_graph`-backed server populated
/// by running the real tree-sitter → KG pipeline on the fixture
/// `tests/fixtures/rust-project/src/util.rs` with the *canonical
/// absolute path* — so that `understand_code`'s `list_entities_by_file`
/// lookup against a canonicalised `target` succeeds.
///
/// Also upserts a synthetic caller of `evaluate` so the symbol-mode
/// response carries a non-empty `inbound_edges` list.
///
/// Returns `(server, kg_arc, tmp_dir, fixture_root_canonical,
/// util_rs_canonical, evaluate_qualified_name,
/// caller_qualified_name)`.
#[cfg(test)]
#[allow(clippy::type_complexity)]
fn build_understand_code_fixture() -> (
    McpServer,
    Arc<Mutex<ucil_core::KnowledgeGraph>>,
    tempfile::TempDir,
    String,
    String,
    String,
    String,
) {
    use ucil_core::{Entity, KnowledgeGraph, Relation};

    use crate::executor::{
        rust_project_fixture, IngestPipeline, SOURCE_TOOL, TREE_SITTER_VALID_FROM,
    };

    let tmp = tempfile::TempDir::new().expect("tempdir must be creatable");
    let kg_path = tmp.path().join("knowledge.db");
    let mut kg = KnowledgeGraph::open(&kg_path).expect("KnowledgeGraph::open must succeed");

    let fixture_root = rust_project_fixture()
        .canonicalize()
        .expect("fixture root canonicalises");
    let util_rs = fixture_root.join("src/util.rs");
    assert!(util_rs.is_file(), "fixture {util_rs:?} must exist");

    let mut pipeline = IngestPipeline::new();
    let inserted = pipeline
        .ingest_file(&mut kg, &util_rs)
        .expect("ingest_file must succeed");
    assert!(
        inserted > 0,
        "fixture ingest must produce ≥1 symbol (got {inserted})"
    );

    let util_rs_canonical = util_rs.display().to_string();

    // Locate the `evaluate` row + its stored `qualified_name`.
    let (evaluate_id, evaluate_qn): (i64, String) = {
        let rows = kg
            .list_entities_by_file(&util_rs_canonical)
            .expect("list_entities_by_file must succeed");
        let row = rows
            .iter()
            .find(|e| e.name == "evaluate" && e.kind == "function")
            .expect("fixture must contain an `evaluate` function row");
        (
            row.id.expect("row must carry id"),
            row.qualified_name
                .clone()
                .expect("row must carry qualified_name"),
        )
    };

    // Upsert a synthetic caller + `calls` relation so the symbol-mode
    // response has non-empty `inbound_edges`.
    let caller_qn = "tests::synthetic::caller_of_evaluate@1:1".to_owned();
    let caller = Entity {
        id: None,
        kind: "function".to_owned(),
        name: "caller_of_evaluate".to_owned(),
        qualified_name: Some(caller_qn.clone()),
        file_path: "tests/synthetic.rs".to_owned(),
        start_line: Some(1),
        end_line: Some(3),
        signature: Some("fn caller_of_evaluate()".to_owned()),
        doc_comment: None,
        language: Some("rust".to_owned()),
        t_valid_from: Some(TREE_SITTER_VALID_FROM.to_owned()),
        t_valid_to: None,
        importance: 0.5,
        source_tool: Some(SOURCE_TOOL.to_owned()),
        source_hash: Some("synthetic".to_owned()),
    };
    let caller_id = kg
        .upsert_entity(&caller)
        .expect("synthetic caller upsert must succeed");
    let relation = Relation {
        id: None,
        source_id: caller_id,
        target_id: evaluate_id,
        kind: "calls".to_owned(),
        weight: 1.0,
        t_valid_from: Some(TREE_SITTER_VALID_FROM.to_owned()),
        t_valid_to: None,
        source_tool: Some(SOURCE_TOOL.to_owned()),
        source_evidence: Some("synthetic test edge".to_owned()),
        confidence: 1.0,
    };
    kg.upsert_relation(&relation)
        .expect("synthetic calls relation upsert must succeed");

    let fixture_root_canonical = fixture_root.display().to_string();

    let kg_arc = Arc::new(Mutex::new(kg));
    let server = McpServer::with_knowledge_graph(Arc::clone(&kg_arc));
    (
        server,
        kg_arc,
        tmp,
        fixture_root_canonical,
        util_rs_canonical,
        evaluate_qn,
        caller_qn,
    )
}

/// Frozen acceptance selector for feature `P1-W4-F09` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon server::test_understand_code_tool`.
///
/// Exercises the file-mode happy path of the `understand_code`
/// `tools/call` dispatch against a real tree-sitter → KG
/// pipeline-populated database.  Asserts:
///
/// 1. The JSON-RPC envelope is well-formed (`jsonrpc == "2.0"`,
///    matching `id`, no `error` field).
/// 2. `result._meta.tool == "understand_code"` and
///    `result._meta.source == "tree-sitter+kg"`.
/// 3. `result._meta.kind == "file"` and `result._meta.target`
///    echoes the caller's request string.
/// 4. `result._meta.summary.language == "rust"`.
/// 5. `result._meta.summary.import_count == 5` — the five `use`
///    declarations in `tests/fixtures/rust-project/src/util.rs` (three
///    at the top of the file plus two inside the `mod tests` block;
///    tree-sitter counts them all because `count_imports` walks the
///    whole AST, not just the module root).
/// 6. `result._meta.summary.line_count >= 1` — non-zero lines.
/// 7. `result._meta.summary.top_level_symbols` is a non-empty array
///    and carries an `evaluate` function row.
/// 8. `result._meta.summary.kg_entity_count >= 1` — the ingest
///    pipeline produced at least one KG row for this file.
/// 9. `result.isError == false`.
/// 10. `result._meta.not_yet_implemented` is **absent** — the handler
///     must have escaped the phase-1 stub path.
#[cfg(test)]
fn assert_understand_code_file_response(response: &Value, expected_target: &str) {
    let meta = response
        .pointer("/result/_meta")
        .expect("response must carry result._meta");
    assert_eq!(
        meta.get("tool").and_then(Value::as_str),
        Some("understand_code"),
        "_meta.tool must be \"understand_code\": {response}"
    );
    assert_eq!(
        meta.get("source").and_then(Value::as_str),
        Some("tree-sitter+kg"),
        "_meta.source must be \"tree-sitter+kg\": {response}"
    );
    assert_eq!(
        meta.get("kind").and_then(Value::as_str),
        Some("file"),
        "_meta.kind must be \"file\": {response}"
    );
    assert_eq!(
        meta.get("target").and_then(Value::as_str),
        Some(expected_target),
        "_meta.target must echo caller's target string: {response}"
    );

    let summary = meta
        .get("summary")
        .expect("_meta.summary must be present on file-mode response");
    assert_eq!(
        summary.get("language").and_then(Value::as_str),
        Some("rust"),
        "_meta.summary.language must be \"rust\" for util.rs: {response}"
    );
    let import_count = summary
        .get("import_count")
        .and_then(Value::as_u64)
        .expect("_meta.summary.import_count must be an integer");
    assert_eq!(
        import_count, 5,
        "_meta.summary.import_count must be 5 (three top-level `use` decls \
         + two inside `mod tests` in tests/fixtures/rust-project/src/util.rs): \
         got {import_count} — {response}"
    );
    let line_count = summary
        .get("line_count")
        .and_then(Value::as_u64)
        .expect("_meta.summary.line_count must be an integer");
    assert!(
        line_count >= 1,
        "_meta.summary.line_count must be >= 1: got {line_count} — {response}"
    );

    let top_level_symbols = summary
        .get("top_level_symbols")
        .and_then(Value::as_array)
        .expect("_meta.summary.top_level_symbols must be a JSON array");
    assert!(
        !top_level_symbols.is_empty(),
        "_meta.summary.top_level_symbols must be non-empty: {response}"
    );
    assert!(
        top_level_symbols.iter().any(|s| {
            s.get("name").and_then(Value::as_str) == Some("evaluate")
                && s.get("kind").and_then(Value::as_str) == Some("function")
        }),
        "_meta.summary.top_level_symbols must contain an `evaluate` function row: got {top_level_symbols:?}"
    );

    let kg_entity_count = summary
        .get("kg_entity_count")
        .and_then(Value::as_u64)
        .expect("_meta.summary.kg_entity_count must be an integer");
    assert!(
        kg_entity_count >= 1,
        "_meta.summary.kg_entity_count must be >= 1 (ingest pipeline \
         produced rows): got {kg_entity_count} — {response}"
    );

    assert_eq!(
        response.pointer("/result/isError").and_then(Value::as_bool),
        Some(false),
        "result.isError must be false on the happy path: {response}"
    );
    assert!(
        response
            .pointer("/result/_meta/not_yet_implemented")
            .is_none(),
        "result._meta.not_yet_implemented must be ABSENT — the handler \
         must have escaped the phase-1 stub path: {response}"
    );
}

#[cfg(test)]
#[tokio::test]
async fn test_understand_code_tool() {
    let (server, _kg, _tmp, fixture_root, util_rs_canonical, _eval_qn, _caller_qn) =
        build_understand_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 101,
        "method": "tools/call",
        "params": {
            "name": "understand_code",
            "arguments": {
                "target": util_rs_canonical.as_str(),
                "kind": "file",
                "root": fixture_root,
                "reason": "acceptance test"
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert_eq!(
        response.get("jsonrpc").and_then(Value::as_str),
        Some(JSONRPC_VERSION),
        "response must carry jsonrpc == \"2.0\": {response}"
    );
    assert_eq!(
        response.get("id").and_then(Value::as_i64),
        Some(101),
        "response id must echo request id: {response}"
    );
    assert!(
        response.get("error").is_none(),
        "response must not carry an error envelope: {response}"
    );

    assert_understand_code_file_response(&response, util_rs_canonical.as_str());
}

/// Symbol-mode happy path: `kind == "symbol"` + an ingested qualified
/// name resolves through `get_entity_by_qualified_name` and returns
/// the entity projection plus inbound/outbound edges.  The synthetic
/// caller seeded in the fixture should appear in `inbound_edges`.
#[cfg(test)]
#[tokio::test]
async fn test_understand_code_tool_symbol_mode() {
    let (server, _kg, _tmp, fixture_root, _util_rs, evaluate_qn, caller_qn) =
        build_understand_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 102,
        "method": "tools/call",
        "params": {
            "name": "understand_code",
            "arguments": {
                "target": evaluate_qn.as_str(),
                "kind": "symbol",
                "root": fixture_root,
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "symbol-mode happy path must not return an error envelope: {response}"
    );

    let meta = response
        .pointer("/result/_meta")
        .expect("response must carry result._meta");
    assert_eq!(
        meta.get("kind").and_then(Value::as_str),
        Some("symbol"),
        "_meta.kind must be \"symbol\": {response}"
    );
    assert_eq!(
        meta.get("found").and_then(Value::as_bool),
        Some(true),
        "_meta.found must be true for a resolved qualified_name: {response}"
    );
    assert_eq!(
        meta.get("target").and_then(Value::as_str),
        Some(evaluate_qn.as_str()),
        "_meta.target must echo the requested qualified_name: {response}"
    );

    let summary = meta
        .get("summary")
        .expect("_meta.summary must be present on symbol-mode response");
    let inbound_edges = summary
        .get("inbound_edges")
        .and_then(Value::as_array)
        .expect("_meta.summary.inbound_edges must be an array");
    assert!(
        inbound_edges.iter().any(|e| {
            e.get("peer_qualified_name").and_then(Value::as_str) == Some(caller_qn.as_str())
                && e.get("relation_type").and_then(Value::as_str) == Some("calls")
        }),
        "inbound_edges must contain the synthetic `calls` edge from {caller_qn:?}: \
         got {inbound_edges:?}"
    );

    let entity = summary
        .get("entity")
        .expect("_meta.summary.entity must be present");
    assert_eq!(
        entity.get("name").and_then(Value::as_str),
        Some("evaluate"),
        "_meta.summary.entity.name must be \"evaluate\": {response}"
    );
    assert_eq!(
        entity.get("entity_type").and_then(Value::as_str),
        Some("function"),
        "_meta.summary.entity.entity_type must be \"function\": {response}"
    );
    assert_eq!(
        response.pointer("/result/isError").and_then(Value::as_bool),
        Some(false),
        "result.isError must be false on symbol-mode happy path: {response}"
    );
    assert!(
        response
            .pointer("/result/_meta/not_yet_implemented")
            .is_none(),
        "result._meta.not_yet_implemented must be ABSENT on symbol mode: {response}"
    );
}

/// Auto-detect mode: when `kind` is omitted and the target resolves
/// to a file under `root`, the dispatcher must pick file mode.
#[cfg(test)]
#[tokio::test]
async fn test_understand_code_tool_auto_detect_file() {
    let (server, _kg, _tmp, fixture_root, util_rs_canonical, _eval_qn, _caller_qn) =
        build_understand_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 103,
        "method": "tools/call",
        "params": {
            "name": "understand_code",
            "arguments": {
                "target": util_rs_canonical,
                "root": fixture_root,
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "auto-detected file mode must not produce error: {response}"
    );
    assert_eq!(
        response
            .pointer("/result/_meta/kind")
            .and_then(Value::as_str),
        Some("file"),
        "auto-detection with a path-to-file target must route to file mode: {response}"
    );
}

/// Malformed args: missing `target` must yield JSON-RPC `-32602`.
#[cfg(test)]
#[tokio::test]
async fn test_understand_code_tool_missing_target() {
    let (server, _kg, _tmp, fixture_root, _util_rs, _eval_qn, _caller_qn) =
        build_understand_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 104,
        "method": "tools/call",
        "params": {
            "name": "understand_code",
            "arguments": { "root": fixture_root }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    let error = response
        .get("error")
        .expect("missing `target` must produce JSON-RPC error envelope");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(-32602),
        "missing-target error code must be -32602 (Invalid params): {response}"
    );
}

/// Malformed args: empty-string `target` must yield `-32602`.
#[cfg(test)]
#[tokio::test]
async fn test_understand_code_tool_empty_target() {
    let (server, _kg, _tmp, fixture_root, _util_rs, _eval_qn, _caller_qn) =
        build_understand_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 105,
        "method": "tools/call",
        "params": {
            "name": "understand_code",
            "arguments": { "target": "", "root": fixture_root }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    let error = response
        .get("error")
        .expect("empty `target` must produce JSON-RPC error envelope");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(-32602),
        "empty-target error code must be -32602 (Invalid params): {response}"
    );
}

/// Malformed args: a `kind` string outside the
/// `{"file","symbol","module"}` set must yield `-32602`.
#[cfg(test)]
#[tokio::test]
async fn test_understand_code_tool_invalid_kind() {
    let (server, _kg, _tmp, fixture_root, _util_rs, _eval_qn, _caller_qn) =
        build_understand_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 106,
        "method": "tools/call",
        "params": {
            "name": "understand_code",
            "arguments": {
                "target": "something",
                "kind": "class",
                "root": fixture_root,
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    let error = response
        .get("error")
        .expect("invalid `kind` must produce JSON-RPC error envelope");
    assert_eq!(
        error.get("code").and_then(Value::as_i64),
        Some(-32602),
        "invalid-kind error code must be -32602 (Invalid params): {response}"
    );
}

/// Symbol-mode unknown symbol: the response must be a well-formed
/// envelope with `_meta.found == false` and `isError == false` — NOT
/// a JSON-RPC error (WO-0036 `scope_in` bullet 7).
#[cfg(test)]
#[tokio::test]
async fn test_understand_code_tool_unknown_symbol() {
    let (server, _kg, _tmp, fixture_root, _util_rs, _eval_qn, _caller_qn) =
        build_understand_code_fixture();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 107,
        "method": "tools/call",
        "params": {
            "name": "understand_code",
            "arguments": {
                "target": "definitely_not_a_symbol::anywhere@0:0",
                "kind": "symbol",
                "root": fixture_root,
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "unknown-symbol path must not return a JSON-RPC error: {response}"
    );
    assert_eq!(
        response
            .pointer("/result/_meta/found")
            .and_then(Value::as_bool),
        Some(false),
        "_meta.found must be false for unresolved symbol: {response}"
    );
    assert_eq!(
        response.pointer("/result/isError").and_then(Value::as_bool),
        Some(false),
        "result.isError must be false (well-formed zero-row result, \
         not an error): {response}"
    );
}

/// When the server is built without a KG attached, `understand_code`
/// must fall through to the phase-1 stub — preserving phase-1
/// invariant #9 for hosts that haven't wired up a KG yet.
#[cfg(test)]
#[tokio::test]
async fn test_understand_code_tool_no_kg_returns_stub() {
    let server = McpServer::new();

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 108,
        "method": "tools/call",
        "params": {
            "name": "understand_code",
            "arguments": { "target": "x" }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert!(
        response.get("error").is_none(),
        "stub path must not return a JSON-RPC error: {response}"
    );
    assert_eq!(
        response
            .pointer("/result/_meta/not_yet_implemented")
            .and_then(Value::as_bool),
        Some(true),
        "no-KG path must fall through to the phase-1 stub \
         (_meta.not_yet_implemented == true): {response}"
    );
    assert_eq!(
        response
            .pointer("/result/_meta/tool")
            .and_then(Value::as_str),
        Some("understand_code"),
        "_meta.tool must echo the tool name even in the stub path: {response}"
    );
}

// ── WO-0066 / P2-W8-F08 frozen acceptance test ────────────────────────────
//
// `server::test_find_similar_tool` lives at MODULE ROOT (NOT inside
// `mod tests {}`) per `DEC-0007` so the frozen selector
// `cargo nextest run -p ucil-daemon server::test_find_similar_tool`
// resolves directly to `ucil_daemon::server::test_find_similar_tool`
// without an intermediate `tests::` segment.

/// Frozen acceptance selector for feature `P2-W8-F08` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-daemon server::test_find_similar_tool`.
///
/// Closes Phase 2 Week 8 and the entire Phase 2 envelope per
/// master-plan §3.2 line 219 (`find_similar` tool listing) +
/// §18 Phase 2 Week 8 line 1791 ("Vector search works").
///
/// Eight sub-assertions (SA1..SA8) per the WO-0066 scope:
///
/// 1. **SA1** — happy-path JSON-RPC envelope: `jsonrpc == "2.0"`,
///    matching `id`, no `error`, `result.isError == false`.
/// 2. **SA2** — `_meta` shape: `tool == "find_similar"`, `source`
///    non-empty string, `branch == "main"`, `query_dim == 768`,
///    `hits_count` is a JSON number, `hits` is a JSON array.
/// 3. **SA3** — hits length matches `max_results` and each hit
///    carries the projection fields with the right types.
/// 4. **SA4** — IDENTITY query: a snippet that is byte-identical
///    to a known corpus chunk's content puts that chunk's
///    `file_path` at `_meta.hits[0]`.  Under
///    `TestEmbeddingSource`'s deterministic Sha256-derived
///    vectors, identical input → identical embedding → distance
///    ≈ 0 → top hit.
/// 5. **SA5** — similarity ordering monotonically descending:
///    `hits[i].similarity_score >= hits[i+1].similarity_score`
///    for all `i`.
/// 6. **SA6** — error path: missing `arguments.snippet` returns
///    JSON-RPC `error.code == -32602` with `error.message`
///    mentioning `snippet`.
/// 7. **SA7** — error path: `arguments.branch ==
///    "nonexistent-branch"` returns `result.isError == true` with
///    `_meta.error_kind ∈ {"branch_not_found", "table_not_found"}`.
/// 8. **SA8** — fall-through path: `McpServer::new()` (no
///    `with_find_similar_executor`) responding to
///    `find_similar` returns `_meta.not_yet_implemented == true`,
///    preserving phase-1 invariant #9 for the un-attached case.
///
/// # Panics
///
/// Panics on any sub-assertion failure (test-only).  Panic
/// messages are operator-actionable — they quote the JSON
/// response on failure.
#[cfg(test)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)]
pub async fn test_find_similar_tool() {
    use crate::branch_manager::BranchManager;
    use crate::executor::{
        build_synthetic_chunker_for_lancedb_f04, read_table_rows_for_lancedb_f04,
        TestEmbeddingSource,
    };
    use crate::lancedb_indexer::LancedbChunkIndexer;

    // ── Fixture: build per-branch code_chunks table populated via
    //    LancedbChunkIndexer + TestEmbeddingSource ─────────────────
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

    // Three small Rust source files with DISTINCT multi-statement
    // bodies so the synthetic chunker emits ≥1 chunk per file under
    // the WordLevel tokenizer.
    let foo_path = repo.path().join("src/foo.rs");
    let bar_path = repo.path().join("src/bar.rs");
    let baz_path = repo.path().join("src/baz.rs");
    tokio::fs::create_dir_all(foo_path.parent().expect("foo parent"))
        .await
        .expect("mkdir src");
    let alpha_text = "pub fn foo_alpha() { let x = 1; let y = 2; let z = x + y; }";
    let beta_text = "pub fn bar_beta() -> i32 { 42 }";
    let gamma_text = "pub fn baz_gamma(input: &str) -> usize { input.len() }";
    tokio::fs::write(&foo_path, alpha_text)
        .await
        .expect("write foo.rs");
    tokio::fs::write(&bar_path, beta_text)
        .await
        .expect("write bar.rs");
    tokio::fs::write(&baz_path, gamma_text)
        .await
        .expect("write baz.rs");

    let mut indexer =
        LancedbChunkIndexer::new(mgr.clone(), "main", chunker.clone(), source.clone());
    let stats = indexer
        .index_paths(
            repo.path(),
            &[foo_path.clone(), bar_path.clone(), baz_path.clone()],
        )
        .await
        .expect("index_paths must succeed");
    assert!(
        stats.chunks_inserted >= 3,
        "fixture: expected ≥3 chunks across 3 files; got stats={stats:?}"
    );
    let (_rows, count) = read_table_rows_for_lancedb_f04(&branches_root, "main").await;
    assert!(
        count >= 3,
        "fixture: code_chunks table must have ≥3 rows after indexing; got count={count}"
    );

    let executor = Arc::new(FindSimilarExecutor::new(
        mgr.clone(),
        source.clone() as Arc<dyn crate::lancedb_indexer::EmbeddingSource>,
        "main",
    ));
    let server = McpServer::new().with_find_similar_executor(executor);

    // ── SA1 — happy-path envelope ──────────────────────────────────
    //
    // Uses a query snippet IDENTICAL to baz.rs's content so the
    // deterministic Sha256-derived embedding produces an
    // identical vector to the chunk emitted from baz.rs — distance
    // ≈ 0 to that chunk, which lands at hits[0] in SA4.
    let identity_snippet = gamma_text.to_owned();
    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 401,
        "method": "tools/call",
        "params": {
            "name": "find_similar",
            "arguments": {
                "snippet": identity_snippet.clone(),
                "max_results": 3
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert_eq!(
        response.get("jsonrpc").and_then(Value::as_str),
        Some(JSONRPC_VERSION),
        "(SA1) response must carry jsonrpc == \"2.0\": {response}"
    );
    assert_eq!(
        response.get("id").and_then(Value::as_i64),
        Some(401),
        "(SA1) response id must echo 401: {response}"
    );
    assert!(
        response.get("error").is_none(),
        "(SA1) response must not carry an error envelope: {response}"
    );
    assert_eq!(
        response.pointer("/result/isError").and_then(Value::as_bool),
        Some(false),
        "(SA1) result.isError must be false on the happy path: {response}"
    );

    let meta = response
        .pointer("/result/_meta")
        .expect("(SA1) response must carry result._meta");

    // ── SA2 — _meta shape ──────────────────────────────────────────
    assert_eq!(
        meta.get("tool").and_then(Value::as_str),
        Some("find_similar"),
        "(SA2) _meta.tool must be \"find_similar\": {response}"
    );
    let source_str = meta
        .get("source")
        .and_then(Value::as_str)
        .expect("(SA2) _meta.source must be a string");
    assert!(
        !source_str.is_empty(),
        "(SA2) _meta.source must be a non-empty string; got {source_str:?}"
    );
    assert_eq!(
        meta.get("branch").and_then(Value::as_str),
        Some("main"),
        "(SA2) _meta.branch must be \"main\": {response}"
    );
    assert_eq!(
        meta.get("query_dim").and_then(Value::as_u64),
        Some(768),
        "(SA2) _meta.query_dim must be 768 (TestEmbeddingSource default): {response}"
    );
    assert!(
        meta.get("hits_count").and_then(Value::as_u64).is_some(),
        "(SA2) _meta.hits_count must be a JSON number: {response}"
    );
    let hits = meta
        .get("hits")
        .and_then(Value::as_array)
        .expect("(SA2) _meta.hits must be a JSON array");

    // ── SA3 — hits count and projection structure ──────────────────
    let hits_count_meta = meta
        .get("hits_count")
        .and_then(Value::as_u64)
        .expect("(SA3) _meta.hits_count must be a JSON number");
    assert_eq!(
        hits.len() as u64,
        hits_count_meta,
        "(SA3) _meta.hits.len() must equal _meta.hits_count: \
         hits.len()={} hits_count={hits_count_meta}; response={response}",
        hits.len(),
    );
    assert!(
        hits.len() >= 3,
        "(SA3) _meta.hits.len() must be >= 3 (max_results=3, corpus=≥3): \
         got {} hits; response={response}",
        hits.len(),
    );
    for (i, hit) in hits.iter().enumerate() {
        let file_path = hit
            .get("file_path")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("(SA3) hits[{i}].file_path must be a string: {hit}"));
        assert!(
            !file_path.is_empty(),
            "(SA3) hits[{i}].file_path must be non-empty: {hit}"
        );
        let start_line = hit
            .get("start_line")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| panic!("(SA3) hits[{i}].start_line must be an integer: {hit}"));
        assert!(
            start_line >= 1,
            "(SA3) hits[{i}].start_line must be >= 1: {hit}"
        );
        let end_line = hit
            .get("end_line")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| panic!("(SA3) hits[{i}].end_line must be an integer: {hit}"));
        assert!(
            end_line >= start_line,
            "(SA3) hits[{i}].end_line must be >= start_line: {hit}"
        );
        assert!(
            hit.get("content").and_then(Value::as_str).is_some(),
            "(SA3) hits[{i}].content must be a string: {hit}"
        );
        assert!(
            hit.get("language").and_then(Value::as_str).is_some(),
            "(SA3) hits[{i}].language must be a string: {hit}"
        );
        // symbol_name and symbol_kind are nullable (Option<String>)
        let symbol_name_field = hit
            .get("symbol_name")
            .unwrap_or_else(|| panic!("(SA3) hits[{i}].symbol_name field must be present: {hit}"));
        assert!(
            symbol_name_field.is_string() || symbol_name_field.is_null(),
            "(SA3) hits[{i}].symbol_name must be string or null: {hit}"
        );
        let symbol_kind_field = hit
            .get("symbol_kind")
            .unwrap_or_else(|| panic!("(SA3) hits[{i}].symbol_kind field must be present: {hit}"));
        assert!(
            symbol_kind_field.is_string() || symbol_kind_field.is_null(),
            "(SA3) hits[{i}].symbol_kind must be string or null: {hit}"
        );
        assert!(
            hit.get("similarity_score")
                .and_then(Value::as_f64)
                .is_some(),
            "(SA3) hits[{i}].similarity_score must be a number: {hit}"
        );
    }

    // ── SA4 — IDENTITY query: top hit is baz.rs ────────────────────
    //
    // The chunker emits chunk content unchanged from the source slice,
    // so the identical input produces an identical content string AND
    // therefore — under TestEmbeddingSource's deterministic
    // Sha256-derived vectors — an identical 768-d embedding, hence
    // distance ≈ 0 to that chunk and the largest similarity_score.
    let top_file_path = hits[0]
        .get("file_path")
        .and_then(Value::as_str)
        .expect("(SA4) hits[0].file_path must be a string");
    assert!(
        top_file_path.ends_with("baz.rs"),
        "(SA4) hits[0].file_path must end with \"baz.rs\" (the file holding the \
         identical-content chunk under TestEmbeddingSource's Sha256-derived \
         deterministic embedding); got {top_file_path:?}; response={response}"
    );

    // ── SA5 — similarity ordering monotonically descending ─────────
    for i in 0..hits.len().saturating_sub(1) {
        let s_i = hits[i]
            .get("similarity_score")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("(SA5) hits[{i}].similarity_score: {}", hits[i]));
        let s_next = hits[i + 1]
            .get("similarity_score")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("(SA5) hits[{}].similarity_score: {}", i + 1, hits[i + 1]));
        assert!(
            s_i >= s_next,
            "(SA5) hits[{i}].similarity_score ({s_i}) must be >= \
             hits[{}].similarity_score ({s_next}); response={response}",
            i + 1,
        );
    }

    // ── SA6 — error path: missing arguments.snippet ────────────────
    let request_missing_snippet = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 402,
        "method": "tools/call",
        "params": {
            "name": "find_similar",
            "arguments": {}
        }
    })
    .to_string();
    let response_missing_snippet = server.handle_line(&request_missing_snippet).await;
    let err_missing = response_missing_snippet
        .get("error")
        .expect("(SA6) missing snippet must produce a JSON-RPC error envelope");
    assert_eq!(
        err_missing.get("code").and_then(Value::as_i64),
        Some(-32602),
        "(SA6) missing-snippet error code must be -32602 (Invalid params): \
         {response_missing_snippet}"
    );
    let err_message = err_missing
        .get("message")
        .and_then(Value::as_str)
        .expect("(SA6) error.message must be a string");
    assert!(
        err_message.contains("snippet"),
        "(SA6) error.message must mention `snippet`; got {err_message:?}; \
         response={response_missing_snippet}"
    );

    // ── SA7 — error path: nonexistent branch ───────────────────────
    let request_nonexistent_branch = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 403,
        "method": "tools/call",
        "params": {
            "name": "find_similar",
            "arguments": {
                "snippet": "anything",
                "branch": "nonexistent-branch"
            }
        }
    })
    .to_string();
    let response_nonexistent_branch = server.handle_line(&request_nonexistent_branch).await;
    assert!(
        response_nonexistent_branch.get("error").is_none(),
        "(SA7) nonexistent-branch must NOT produce a JSON-RPC error envelope \
         (per master-plan §3.2 UX: runtime failures are isError=true, NOT \
         JSON-RPC error); got {response_nonexistent_branch}"
    );
    assert_eq!(
        response_nonexistent_branch
            .pointer("/result/isError")
            .and_then(Value::as_bool),
        Some(true),
        "(SA7) result.isError must be true on nonexistent-branch: \
         {response_nonexistent_branch}"
    );
    let error_kind = response_nonexistent_branch
        .pointer("/result/_meta/error_kind")
        .and_then(Value::as_str)
        .expect("(SA7) _meta.error_kind must be present on nonexistent-branch");
    assert!(
        matches!(error_kind, "branch_not_found" | "table_not_found"),
        "(SA7) _meta.error_kind must be \"branch_not_found\" OR \
         \"table_not_found\" for a nonexistent branch dir; got {error_kind:?}; \
         response={response_nonexistent_branch}"
    );

    // ── SA8 — fall-through path when no executor attached ──────────
    let server_no_executor = McpServer::new();
    let request_no_executor = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 404,
        "method": "tools/call",
        "params": {
            "name": "find_similar",
            "arguments": {
                "snippet": "anything"
            }
        }
    })
    .to_string();
    let response_no_executor = server_no_executor.handle_line(&request_no_executor).await;
    assert!(
        response_no_executor.get("error").is_none(),
        "(SA8) no-executor stub path must not return a JSON-RPC error: \
         {response_no_executor}"
    );
    assert_eq!(
        response_no_executor
            .pointer("/result/_meta/not_yet_implemented")
            .and_then(Value::as_bool),
        Some(true),
        "(SA8) no-executor call must return _meta.not_yet_implemented == true \
         (phase-1 invariant #9): {response_no_executor}"
    );
    assert_eq!(
        response_no_executor
            .pointer("/result/_meta/tool")
            .and_then(Value::as_str),
        Some("find_similar"),
        "(SA8) stub envelope must echo tool name: {response_no_executor}"
    );
}

// ── G4 architecture MCP tool tests (P3-W10-F16/F17/F18) ──────────────────────
//
// Three frozen `#[tokio::test]` selectors live at MODULE ROOT (NOT inside
// any inner `mod tests { … }`) so the substring-match selector
// `cargo test -p ucil-daemon server::test_get_architecture_tool`
// resolves uniquely without `--exact` per DEC-0007 + WO-0067/0068
// lessons §planner.  Each test injects a deterministic in-process
// `TestG4Source` impl (shared helper below) and exercises the
// corresponding handler through `handle_tools_call` end-to-end.
//
// The shared `TestG4Source` helper lives under `#[cfg(test)]` so it
// compiles only in the test profile — exempt from the production-side
// `mock|fake|stub` word-ban grep per WO-0048 line 363 + WO-0069
// carve-out.

/// Behaviour-light in-process [`G4Source`] impl driving the three
/// frozen selectors below.  Per `DEC-0008` §4 the [`G4Source`] trait
/// is UCIL-internal (the dependency-inversion seam) so a local impl
/// in a test is not a substitute for any external wire format —
/// same shape as the `TestG3Source` (WO-0070) and
/// `executor::test_g4_architecture_query`'s `TestG4Source`
/// (`crates/ucil-daemon/src/executor.rs:3822`) precedents.
///
/// The helper carries the `source_id` plus a pre-canned
/// `Vec<G4DependencyEdge>`; `execute()` returns the edges verbatim
/// under [`G4SourceStatus::Available`] with `elapsed_ms = 0`.
#[cfg(test)]
struct TestG4Source {
    id: String,
    edges: Vec<crate::g4::G4DependencyEdge>,
}

#[cfg(test)]
#[async_trait::async_trait]
impl G4Source for TestG4Source {
    fn source_id(&self) -> &str {
        &self.id
    }

    async fn execute(&self, _query: &G4Query) -> G4SourceOutput {
        G4SourceOutput {
            source_id: self.id.clone(),
            status: G4SourceStatus::Available,
            elapsed_ms: 0,
            edges: self.edges.clone(),
            error: None,
        }
    }
}

/// Convenience: build a `Vec<Arc<dyn G4Source>>` of one
/// [`TestG4Source`] from a pre-canned edge list.
#[cfg(test)]
fn test_g4_source_list(
    id: &str,
    edges: Vec<crate::g4::G4DependencyEdge>,
) -> Arc<Vec<Arc<dyn G4Source>>> {
    let src: Arc<dyn G4Source> = Arc::new(TestG4Source {
        id: id.to_owned(),
        edges,
    });
    Arc::new(vec![src])
}

/// Convenience: build a [`crate::g4::G4DependencyEdge`] for the
/// three frozen tests.  Always uses `G4EdgeOrigin::Inferred` since
/// the test surfaces only need to drive the dependency-inversion
/// seam, not the (P3-W10-F14 future) ground-truth-on-conflict
/// branch.
#[cfg(test)]
fn make_g4_test_edge(
    source: &str,
    target: &str,
    kind: crate::g4::G4EdgeKind,
    weight: f64,
    src_id: &str,
) -> crate::g4::G4DependencyEdge {
    crate::g4::G4DependencyEdge {
        source: source.to_owned(),
        target: target.to_owned(),
        edge_kind: kind,
        source_id: src_id.to_owned(),
        origin: crate::g4::G4EdgeOrigin::Inferred,
        coupling_weight: weight,
    }
}

/// Frozen acceptance test for `P3-W10-F16` (`get_architecture`).
///
/// Master-plan §3.2 row 8 / §5.4 lines 483-500 / §18 Phase 3 Week 10
/// line 1812.  Drives `handle_tools_call` end-to-end through the
/// [`McpServer::handle_get_architecture`] handler with a deterministic
/// in-process [`TestG4Source`] returning four edges spanning four
/// node names — the architecture surface the tool's `_meta.modules`
/// + `_meta.edges` envelope advertises.
///
/// Selector substring-match: `cargo test -p ucil-daemon
/// server::test_get_architecture_tool` resolves uniquely without
/// `--exact` per DEC-0007 + WO-0067/0068 lessons §planner.
#[cfg(test)]
#[tokio::test]
async fn test_get_architecture_tool() {
    use crate::g4::G4EdgeKind;

    let edges = vec![
        make_g4_test_edge("A", "B", G4EdgeKind::Import, 0.9, "test-g4-source"),
        make_g4_test_edge("B", "C", G4EdgeKind::Call, 0.7, "test-g4-source"),
        make_g4_test_edge("C", "D", G4EdgeKind::Implements, 0.6, "test-g4-source"),
        make_g4_test_edge("A", "D", G4EdgeKind::Inherits, 0.4, "test-g4-source"),
    ];
    let sources = test_g4_source_list("test-g4-source", edges);
    let server = McpServer::new().with_g4_sources(sources);

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 0xA1,
        "method": "tools/call",
        "params": {
            "name": "get_architecture",
            "arguments": {
                "target": "A",
                "max_depth": 4,
                "max_edges": 256,
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert_eq!(
        response.get("error"),
        None,
        "(precondition) handler must not return JSON-RPC error: {response}"
    );

    let meta = response
        .pointer("/result/_meta")
        .expect("(precondition) response must carry result._meta");

    // ── SA1 — _meta.tool ─────────────────────────────────────────────
    assert_eq!(
        meta.get("tool").and_then(Value::as_str),
        Some("get_architecture"),
        "(SA1) _meta.tool == \"get_architecture\"; left: {:?}, right: \"get_architecture\"",
        meta.get("tool")
    );

    // ── SA2 — _meta.source ───────────────────────────────────────────
    assert_eq!(
        meta.get("source").and_then(Value::as_str),
        Some("g4-architecture-fanout"),
        "(SA2) _meta.source == \"g4-architecture-fanout\"; left: {:?}, right: \"g4-architecture-fanout\"",
        meta.get("source")
    );

    // ── SA3 — _meta.modules sorted unique [A, B, C, D] ──────────────
    let modules = meta
        .get("modules")
        .and_then(Value::as_array)
        .expect("(SA3 precondition) _meta.modules must be a JSON array");
    let module_strs: Vec<&str> = modules.iter().filter_map(Value::as_str).collect();
    assert_eq!(
        module_strs,
        vec!["A", "B", "C", "D"],
        "(SA3) modules contains all four nodes sorted unique; left: {module_strs:?}, right: [\"A\", \"B\", \"C\", \"D\"]"
    );

    // ── SA4 — _meta.edges.len() == 4 ────────────────────────────────
    let edges_arr = meta
        .get("edges")
        .and_then(Value::as_array)
        .expect("(SA4 precondition) _meta.edges must be a JSON array");
    assert_eq!(
        edges_arr.len(),
        4,
        "(SA4) edges.len() == 4; left: {}, right: 4",
        edges_arr.len()
    );

    // ── SA5 — highest coupling_weight is 0.9 (A->B Import) ──────────
    let max_weight = edges_arr
        .iter()
        .filter_map(|e| e.get("coupling_weight").and_then(Value::as_f64))
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_weight - 0.9).abs() < 1e-9,
        "(SA5) max edge coupling_weight == 0.9 (A->B Import); left: {max_weight}, right: 0.9"
    );

    // ── SA6 — master_timed_out == false ─────────────────────────────
    assert_eq!(
        meta.get("master_timed_out").and_then(Value::as_bool),
        Some(false),
        "(SA6) master_timed_out == false; left: {:?}, right: false",
        meta.get("master_timed_out")
    );

    // ── SA7 — _meta.source_results.len() == 1 ───────────────────────
    let src_results = meta
        .get("source_results")
        .and_then(Value::as_array)
        .expect("(SA7 precondition) _meta.source_results must be a JSON array");
    assert_eq!(
        src_results.len(),
        1,
        "(SA7) source_results.len() == 1; left: {}, right: 1",
        src_results.len()
    );

    // ── SA8 — source_results[0].status == "available" ──────────────
    assert_eq!(
        src_results[0].get("status").and_then(Value::as_str),
        Some("available"),
        "(SA8) source_results[0].status == \"available\"; left: {:?}, right: \"available\"",
        src_results[0].get("status")
    );
}

/// Frozen acceptance test for `P3-W10-F17` (`trace_dependencies`).
///
/// Master-plan §3.2 row 9 / §5.4.  Drives `handle_tools_call`
/// end-to-end with a `TestG4Source` producing a small chain plus a
/// fork (A->B, B->C, C->D, E->B) so the directional BFS surfaces both
/// upstream (A@1 + E@1) and downstream (C@1 + D@2) correctly when
/// `direction == "both"`, AND honours direction filtering when
/// `direction == "upstream"` (omits `_meta.downstream` entirely).
///
/// Selector substring-match: `cargo test -p ucil-daemon
/// server::test_trace_dependencies_tool` resolves uniquely without
/// `--exact`.
#[cfg(test)]
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_trace_dependencies_tool() {
    use crate::g4::G4EdgeKind;

    let edges = vec![
        make_g4_test_edge("A", "B", G4EdgeKind::Call, 0.9, "test-g4-source"),
        make_g4_test_edge("B", "C", G4EdgeKind::Call, 0.8, "test-g4-source"),
        make_g4_test_edge("C", "D", G4EdgeKind::Call, 0.7, "test-g4-source"),
        make_g4_test_edge("E", "B", G4EdgeKind::Call, 0.6, "test-g4-source"),
    ];
    let sources = test_g4_source_list("test-g4-source", edges);
    let server = McpServer::new().with_g4_sources(sources);

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 0xB1,
        "method": "tools/call",
        "params": {
            "name": "trace_dependencies",
            "arguments": {
                "target": "B",
                "direction": "both",
                "max_depth": 3,
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert_eq!(
        response.get("error"),
        None,
        "(precondition) handler must not return JSON-RPC error: {response}"
    );

    let meta = response
        .pointer("/result/_meta")
        .expect("(precondition) response must carry result._meta");

    // ── SA1 — _meta.tool ─────────────────────────────────────────────
    assert_eq!(
        meta.get("tool").and_then(Value::as_str),
        Some("trace_dependencies"),
        "(SA1) _meta.tool == \"trace_dependencies\"; left: {:?}, right: \"trace_dependencies\"",
        meta.get("tool")
    );

    // ── SA2 — _meta.target ──────────────────────────────────────────
    assert_eq!(
        meta.get("target").and_then(Value::as_str),
        Some("B"),
        "(SA2) _meta.target == \"B\"; left: {:?}, right: \"B\"",
        meta.get("target")
    );

    // ── SA3 — _meta.direction ──────────────────────────────────────
    assert_eq!(
        meta.get("direction").and_then(Value::as_str),
        Some("both"),
        "(SA3) _meta.direction == \"both\"; left: {:?}, right: \"both\"",
        meta.get("direction")
    );

    // ── SA4 — upstream contains [A@1, E@1] sorted alphabetically ───
    let upstream = meta
        .get("upstream")
        .and_then(Value::as_array)
        .expect("(SA4 precondition) _meta.upstream must be a JSON array");
    let upstream_pairs: Vec<(String, u64)> = upstream
        .iter()
        .filter_map(|e| {
            let n = e.get("node")?.as_str()?.to_owned();
            let d = e.get("depth")?.as_u64()?;
            Some((n, d))
        })
        .collect();
    assert_eq!(
        upstream_pairs,
        vec![
            ("A".to_owned(), 1u64),
            ("E".to_owned(), 1u64),
        ],
        "(SA4) upstream contains A@1 + E@1 sorted alphabetically; left: {upstream_pairs:?}, right: [(\"A\", 1), (\"E\", 1)]"
    );

    // ── SA5 — downstream contains [C@1, D@2] sorted by depth asc ──
    let downstream = meta
        .get("downstream")
        .and_then(Value::as_array)
        .expect("(SA5 precondition) _meta.downstream must be a JSON array");
    let downstream_pairs: Vec<(String, u64)> = downstream
        .iter()
        .filter_map(|e| {
            let n = e.get("node")?.as_str()?.to_owned();
            let d = e.get("depth")?.as_u64()?;
            Some((n, d))
        })
        .collect();
    assert_eq!(
        downstream_pairs,
        vec![
            ("C".to_owned(), 1u64),
            ("D".to_owned(), 2u64),
        ],
        "(SA5) downstream contains C@1 + D@2 sorted by depth ascending; left: {downstream_pairs:?}, right: [(\"C\", 1), (\"D\", 2)]"
    );

    // ── SA6 — master_timed_out == false ─────────────────────────────
    assert_eq!(
        meta.get("master_timed_out").and_then(Value::as_bool),
        Some(false),
        "(SA6) master_timed_out == false; left: {:?}, right: false",
        meta.get("master_timed_out")
    );

    // ── SA7 — direction = "upstream" omits _meta.downstream ─────────
    let edges_again = vec![
        make_g4_test_edge("A", "B", G4EdgeKind::Call, 0.9, "test-g4-source"),
        make_g4_test_edge("B", "C", G4EdgeKind::Call, 0.8, "test-g4-source"),
        make_g4_test_edge("C", "D", G4EdgeKind::Call, 0.7, "test-g4-source"),
        make_g4_test_edge("E", "B", G4EdgeKind::Call, 0.6, "test-g4-source"),
    ];
    let sources_upstream_only = test_g4_source_list("test-g4-source", edges_again);
    let server_upstream_only = McpServer::new().with_g4_sources(sources_upstream_only);
    let request_upstream_only = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 0xB2,
        "method": "tools/call",
        "params": {
            "name": "trace_dependencies",
            "arguments": {
                "target": "B",
                "direction": "upstream",
                "max_depth": 3,
            }
        }
    })
    .to_string();
    let response_upstream_only = server_upstream_only
        .handle_line(&request_upstream_only)
        .await;
    let meta_upstream_only = response_upstream_only
        .pointer("/result/_meta")
        .expect("(SA7 precondition) upstream-only response must carry result._meta");
    assert_eq!(
        meta_upstream_only.get("direction").and_then(Value::as_str),
        Some("upstream"),
        "(SA7) direction echoed as \"upstream\"; left: {:?}, right: \"upstream\"",
        meta_upstream_only.get("direction")
    );
    assert!(
        meta_upstream_only.get("downstream").is_none(),
        "(SA7) _meta.downstream must be ABSENT when direction == \"upstream\" — direction filtering is load-bearing; left: {:?}, right: <key absent>",
        meta_upstream_only.get("downstream")
    );
    assert!(
        meta_upstream_only.get("upstream").is_some(),
        "(SA7) _meta.upstream must still be PRESENT when direction == \"upstream\"; left: <absent>, right: <present>"
    );
}

/// Frozen acceptance test for `P3-W10-F18` (`blast_radius`).
///
/// Master-plan §3.2 row 10 / §5.4 line 495.  Drives `handle_tools_call`
/// end-to-end with a `TestG4Source` producing a 5-node BFS tree
/// (A->B, A->C, B->D, C->E) so the bidirectional BFS with
/// multiplicative coupling-weight attenuation surfaces (B, C, D, E)
/// in `_meta.impacted` ranked by `path_weight` descending.
///
/// Selector substring-match: `cargo test -p ucil-daemon
/// server::test_blast_radius_tool` resolves uniquely without
/// `--exact`.
#[cfg(test)]
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_blast_radius_tool() {
    use crate::g4::G4EdgeKind;

    let edges = vec![
        make_g4_test_edge("A", "B", G4EdgeKind::Import, 0.9, "test-g4-source"),
        make_g4_test_edge("A", "C", G4EdgeKind::Call, 0.5, "test-g4-source"),
        make_g4_test_edge("B", "D", G4EdgeKind::Call, 0.8, "test-g4-source"),
        make_g4_test_edge("C", "E", G4EdgeKind::Call, 0.4, "test-g4-source"),
    ];
    let sources = test_g4_source_list("test-g4-source", edges);
    let server = McpServer::new().with_g4_sources(sources);

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 0xC1,
        "method": "tools/call",
        "params": {
            "name": "blast_radius",
            "arguments": {
                "target": "A",
                "max_depth": 3,
                "max_edges": 256,
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;

    assert_eq!(
        response.get("error"),
        None,
        "(precondition) handler must not return JSON-RPC error: {response}"
    );

    let meta = response
        .pointer("/result/_meta")
        .expect("(precondition) response must carry result._meta");

    // ── SA1 — _meta.tool ─────────────────────────────────────────────
    assert_eq!(
        meta.get("tool").and_then(Value::as_str),
        Some("blast_radius"),
        "(SA1) _meta.tool == \"blast_radius\"; left: {:?}, right: \"blast_radius\"",
        meta.get("tool")
    );

    // ── SA2 — _meta.target == ["A"] (single-string lifted) ─────────
    let target_arr = meta
        .get("target")
        .and_then(Value::as_array)
        .expect("(SA2 precondition) _meta.target must be an array (single-string lifted)");
    let target_strs: Vec<&str> = target_arr.iter().filter_map(Value::as_str).collect();
    assert_eq!(
        target_strs,
        vec!["A"],
        "(SA2) _meta.target == [\"A\"] (single-string target lifted to array); left: {target_strs:?}, right: [\"A\"]"
    );

    // ── SA3 — impacted.len() == 4 (B, C, D, E; A excluded) ─────────
    let impacted = meta
        .get("impacted")
        .and_then(Value::as_array)
        .expect("(SA3 precondition) _meta.impacted must be a JSON array");
    assert_eq!(
        impacted.len(),
        4,
        "(SA3) impacted.len() == 4 (B, C, D, E excluding seed A); left: {}, right: 4",
        impacted.len()
    );

    // ── SA4 — impacted[0].node is one of {B, D} (highest weight) ──
    let top_node = impacted
        .first()
        .and_then(|v| v.get("node"))
        .and_then(Value::as_str)
        .expect("(SA4 precondition) impacted[0].node must be a string")
        .to_owned();
    assert!(
        matches!(top_node.as_str(), "B" | "D"),
        "(SA4) impacted[0].node is one of {{B, D}} (highest path_weight chain A->B=0.9 or A->B->D=0.72); left: {top_node:?}, right: \"B\" or \"D\""
    );

    // ── SA5 — descending path_weight invariant ─────────────────────
    let weights: Vec<f64> = impacted
        .iter()
        .filter_map(|e| e.get("path_weight").and_then(Value::as_f64))
        .collect();
    assert!(
        weights.len() >= 2,
        "(SA5 precondition) impacted must carry at least 2 entries to compare; left: {}, right: >= 2",
        weights.len()
    );
    assert!(
        weights[0] >= weights[1],
        "(SA5) impacted[0].path_weight >= impacted[1].path_weight (descending sort invariant); left: {} (impacted[0]={}), right: {} (impacted[1]={})",
        weights[0],
        impacted[0].get("node").and_then(Value::as_str).unwrap_or("?"),
        weights[1],
        impacted[1].get("node").and_then(Value::as_str).unwrap_or("?"),
    );

    // ── SA6 — array-target shape preserved ─────────────────────────
    let edges_array = vec![
        make_g4_test_edge("A", "B", G4EdgeKind::Import, 0.9, "test-g4-source"),
        make_g4_test_edge("A", "C", G4EdgeKind::Call, 0.5, "test-g4-source"),
        make_g4_test_edge("B", "D", G4EdgeKind::Call, 0.8, "test-g4-source"),
        make_g4_test_edge("C", "E", G4EdgeKind::Call, 0.4, "test-g4-source"),
    ];
    let sources_array_target = test_g4_source_list("test-g4-source", edges_array);
    let server_array_target = McpServer::new().with_g4_sources(sources_array_target);
    let request_array_target = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 0xC2,
        "method": "tools/call",
        "params": {
            "name": "blast_radius",
            "arguments": {
                "target": ["A", "C"],
                "max_depth": 3,
                "max_edges": 256,
            }
        }
    })
    .to_string();
    let response_array_target = server_array_target.handle_line(&request_array_target).await;
    let meta_array_target = response_array_target
        .pointer("/result/_meta")
        .expect("(SA6 precondition) array-target response must carry result._meta");
    let target_array_meta = meta_array_target
        .get("target")
        .and_then(Value::as_array)
        .expect("(SA6 precondition) _meta.target must be an array");
    let target_array_strs: Vec<&str> = target_array_meta.iter().filter_map(Value::as_str).collect();
    assert_eq!(
        target_array_strs,
        vec!["A", "C"],
        "(SA6) _meta.target preserves array shape; left: {target_array_strs:?}, right: [\"A\", \"C\"]"
    );

    // ── SA7 — dependency_chain is non-empty path-shape canary ──────
    let dep_chain = meta
        .get("dependency_chain")
        .and_then(Value::as_array)
        .expect("(SA7 precondition) _meta.dependency_chain must be a JSON array");
    assert!(
        !dep_chain.is_empty(),
        "(SA7) dependency_chain.len() >= 1; left: 0, right: >= 1"
    );
    for (i, entry) in dep_chain.iter().enumerate() {
        let s = entry
            .as_str()
            .expect("(SA7 precondition) every dependency_chain entry must be a string");
        assert!(
            !s.is_empty() && s.contains(" -> "),
            "(SA7) dependency_chain[{i}] is non-empty AND contains \" -> \"; left: {s:?}, right: <non-empty path with ' -> '>"
        );
    }

    // ── SA8 — master_timed_out == false ─────────────────────────────
    assert_eq!(
        meta.get("master_timed_out").and_then(Value::as_bool),
        Some(false),
        "(SA8) master_timed_out == false; left: {:?}, right: false",
        meta.get("master_timed_out")
    );
}

// ── G7 quality MCP tool tests (P3-W11-F10) ────────────────────────────────────
//
// One frozen `#[tokio::test]` selector lives at MODULE ROOT (NOT inside
// any inner `mod tests { … }`) so the substring-match selector
// `cargo test -p ucil-daemon server::test_check_quality_tool` resolves
// uniquely without `--exact` per DEC-0007 + WO-0067/0068 lessons §planner.
//
// The test injects deterministic in-process `TestG7Source` /
// `TestG8Source` impls (UCIL's own dependency-inversion seam per
// DEC-0008 §4 — these are NOT mocks of any external wire format) and
// exercises `handle_check_quality` end-to-end through `handle_line` so
// the parallel `tokio::join!` fan-out + merge projection is asserted
// over the real JSON-RPC envelope.

/// Frozen acceptance test for `P3-W11-F10` (`check_quality`).
///
/// Master-plan §3.2 row 14 + §5.7 + §5.8 + §18 Phase 3 Week 11 item 6.
/// Drives `handle_tools_call` end-to-end through
/// [`McpServer::handle_check_quality`] with deterministic
/// in-process [`G7Source`] + [`G8Source`] impls and asserts SA1-SA6:
///
/// * **SA1 — Issue count**: 3 G7 issues spanning Critical / High /
///   Medium severities → `issues[].len() == 3` (after the
///   severity-weighted merge, which preserves count when groups are
///   keyed distinct).
/// * **SA2 — Severity vocabulary canary**: `issues[0].severity ==
///   "critical"` (lowercase per §5.7 + §12.1 + WO-0085 sentinel-row
///   precedent).
/// * **SA3 — Untested-function count**: 2 G8 candidates with distinct
///   `test_path` values → `untested_functions[].len() == 2` (after
///   the dedup-by-test-path merge).
/// * **SA4 — Untested-function path**: `untested_functions[0].test_path`
///   matches the seeded test path.
/// * **SA5 — Master deadline did not trip**: both G7 + G8 sources
///   return immediately under the 5500 ms / 5000 ms masters →
///   `meta.master_timed_out == false`.
/// * **SA6 — Parallelism wall-clock canary**:
///   `meta.wall_elapsed_ms < 5000` ms.  If the parallel `tokio::join!`
///   regresses to sequential awaits on the per-source 4500 ms
///   deadlines, this canary trips before either inner timeout fires.
///
/// Selector substring-match: `cargo test -p ucil-daemon
/// server::test_check_quality_tool` resolves uniquely without
/// `--exact`.
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_check_quality_tool() {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::g7::{G7Issue, G7SourceOutput, G7SourceStatus, Severity};
    use crate::g8::{G8TestCandidate, TestDiscoveryMethod};

    /// Local [`G7Source`] impl returning a pre-canned issue list.
    /// Per `DEC-0008` §4 the [`G7Source`] trait is UCIL-internal so
    /// this is not a substitute for any external wire format —
    /// matches the `g7::test_g7_parallel_pipeline` precedent.
    struct TestG7Source {
        id: String,
        issues: Vec<G7Issue>,
    }

    #[async_trait::async_trait]
    impl G7Source for TestG7Source {
        fn source_id(&self) -> &str {
            &self.id
        }

        async fn execute(&self, _query: &G7Query) -> G7SourceOutput {
            G7SourceOutput {
                source_id: self.id.clone(),
                status: G7SourceStatus::Available,
                elapsed_ms: 0,
                issues: self.issues.clone(),
                error: None,
            }
        }
    }

    /// Local [`G8Source`] impl returning a pre-canned candidate list.
    /// Same `DEC-0008` §4 carve-out as `TestG7Source` above.
    struct TestG8Source {
        id: String,
        method: TestDiscoveryMethod,
        candidates: Vec<G8TestCandidate>,
    }

    #[async_trait::async_trait]
    impl G8Source for TestG8Source {
        fn source_id(&self) -> String {
            self.id.clone()
        }

        fn method(&self) -> TestDiscoveryMethod {
            self.method
        }

        async fn execute(&self, _query: &G8Query) -> Result<Vec<G8TestCandidate>, String> {
            Ok(self.candidates.clone())
        }
    }

    // Three G7 issues spanning Critical / High / Medium severities,
    // anchored to distinct `(file_path, line_start, category)` keys
    // so the severity-weighted merge preserves all three in the
    // output (no group collapse).
    let issues = vec![
        G7Issue {
            source_tool: "lsp:rust-analyzer".to_owned(),
            file_path: "src/auth.rs".to_owned(),
            line_start: Some(10),
            line_end: Some(10),
            category: "type_error".to_owned(),
            severity: Severity::Critical,
            message: "borrow checker error".to_owned(),
            rule_id: Some("E0382".to_owned()),
            fix_suggestion: Some("clone the borrow".to_owned()),
        },
        G7Issue {
            source_tool: "eslint".to_owned(),
            file_path: "src/auth.rs".to_owned(),
            line_start: Some(20),
            line_end: Some(20),
            category: "lint".to_owned(),
            severity: Severity::High,
            message: "no-unused-vars".to_owned(),
            rule_id: Some("no-unused-vars".to_owned()),
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "ruff".to_owned(),
            file_path: "src/auth.rs".to_owned(),
            line_start: Some(30),
            line_end: Some(30),
            category: "lint".to_owned(),
            severity: Severity::Medium,
            message: "F401: unused import".to_owned(),
            rule_id: Some("F401".to_owned()),
            fix_suggestion: None,
        },
    ];

    // Two G8 candidates with distinct `test_path` values so the
    // dedup-by-test-path merge produces exactly two output rows.
    let candidates = vec![
        G8TestCandidate {
            test_path: PathBuf::from("tests/test_auth_login.rs"),
            source_path: Some(PathBuf::from("src/auth.rs")),
            method: TestDiscoveryMethod::Convention,
            confidence: 0.95,
        },
        G8TestCandidate {
            test_path: PathBuf::from("tests/test_auth_logout.rs"),
            source_path: Some(PathBuf::from("src/auth.rs")),
            method: TestDiscoveryMethod::Import,
            confidence: 0.85,
        },
    ];

    let g7_src: Arc<dyn G7Source + Send + Sync> = Arc::new(TestG7Source {
        id: "test-g7-source".to_owned(),
        issues,
    });
    let g8_src: Arc<dyn G8Source + Send + Sync> = Arc::new(TestG8Source {
        id: "test-g8-source".to_owned(),
        method: TestDiscoveryMethod::Convention,
        candidates,
    });

    let server = McpServer::new()
        .with_g7_sources(Arc::new(vec![g7_src]))
        .with_g8_sources(Arc::new(vec![g8_src]));

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 0xC1,
        "method": "tools/call",
        "params": {
            "name": "check_quality",
            "arguments": {
                "target": "src/auth.rs",
                "reason": "verifier smoke",
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;
    assert_eq!(
        response.get("error"),
        None,
        "(precondition) handler must not return JSON-RPC error: {response}"
    );

    let text = response
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .expect("(precondition) response must carry result.content[0].text");
    let parsed: Value = serde_json::from_str(text)
        .expect("(precondition) result.content[0].text must be valid JSON");

    let issues_arr = parsed
        .get("issues")
        .and_then(Value::as_array)
        .expect("(precondition) parsed payload must carry issues[]");
    let untested_arr = parsed
        .get("untested_functions")
        .and_then(Value::as_array)
        .expect("(precondition) parsed payload must carry untested_functions[]");
    let meta = parsed
        .get("meta")
        .expect("(precondition) parsed payload must carry meta");

    // ── SA1 — issues[] length == 3 ──────────────────────────────────
    assert_eq!(
        issues_arr.len(),
        3,
        "(SA1) issues[] length; left: {}, right: 3",
        issues_arr.len()
    );

    // ── SA2 — issues[0].severity == "critical" (vocabulary canary) ──
    assert_eq!(
        issues_arr[0].get("severity").and_then(Value::as_str),
        Some("critical"),
        "(SA2) issues[0].severity == \"critical\"; left: {:?}, right: \"critical\"",
        issues_arr[0].get("severity")
    );

    // ── SA3 — untested_functions[] length == 2 ──────────────────────
    assert_eq!(
        untested_arr.len(),
        2,
        "(SA3) untested_functions[] length; left: {}, right: 2",
        untested_arr.len()
    );

    // ── SA4 — untested_functions[0].test_path matches seeded path ──
    let test_path_0 = untested_arr[0]
        .get("test_path")
        .and_then(Value::as_str)
        .expect("(SA4 precondition) untested_functions[0].test_path must be a string");
    assert!(
        test_path_0.contains("tests/test_auth_login.rs")
            || test_path_0.contains("tests/test_auth_logout.rs"),
        "(SA4) untested_functions[0].test_path matches a seeded test path; left: {test_path_0:?}, right: one of [tests/test_auth_login.rs, tests/test_auth_logout.rs]"
    );

    // ── SA5 — meta.master_timed_out == false ────────────────────────
    assert_eq!(
        meta.get("master_timed_out").and_then(Value::as_bool),
        Some(false),
        "(SA5) meta.master_timed_out == false; left: {:?}, right: false",
        meta.get("master_timed_out")
    );

    // ── SA6 — meta.wall_elapsed_ms < 5000 (parallelism canary) ──────
    let wall_elapsed = meta
        .get("wall_elapsed_ms")
        .and_then(Value::as_u64)
        .expect("(SA6 precondition) meta.wall_elapsed_ms must be a u64");
    assert!(
        wall_elapsed < 5000,
        "(SA6) meta.wall_elapsed_ms < 5000 ms (parallelism canary — sequential awaits would compound G7+G8 per-source deadlines); left: {wall_elapsed}, right: < 5000"
    );
}

// ── G7 type_check MCP tool tests (P3-W11-F15) ─────────────────────────────────
//
// One frozen `#[tokio::test]` selector lives at MODULE ROOT (NOT inside
// any inner `mod tests { … }`) so the substring-match selector
// `cargo test -p ucil-daemon server::test_type_check_tool` resolves
// uniquely without `--exact` per DEC-0007 + WO-0067/0068 lessons §planner.
//
// The test injects a deterministic in-process `ScriptedFakeSerenaClient`
// (UCIL's own `SerenaClient` trait impl per `DEC-0008` §4
// dependency-inversion seam — same pattern used by
// `quality_pipeline.rs`'s test suite, carried forward through
// WO-0048 / WO-0085 / WO-0089 / WO-0090) and exercises
// `handle_type_check` end-to-end through `handle_line` so the
// type-error filter projection is asserted over the real JSON-RPC
// envelope.

/// Frozen acceptance test for `P3-W11-F15` (`type_check`).
///
/// Master-plan §3.2 row 18.  Drives `handle_tools_call` end-to-end
/// through [`McpServer::handle_type_check`] with a deterministic
/// in-process `ScriptedFakeSerenaClient` returning a mixed bag of
/// LSP diagnostics across three files (Rust + Python + TypeScript)
/// and asserts SA1-SA5:
///
/// * **SA1 — Type-error count**: 5 raw diagnostics (3 type errors +
///   1 warning + 1 clippy lint error) → after filter, 3 type errors
///   survive in `errors[]`.
/// * **SA2 — Severity vocabulary canary**: every `errors[].severity
///   == "error"` (lowercase per §5.7 + §12.1).
/// * **SA3 — Language coverage**: `errors[].language` covers
///   `{rust, python, typescript}` — all three type-checker sources
///   represented.
/// * **SA4 — Files-checked count**: `meta.files_checked == 3` (the
///   3 input files all have LSP-supported extensions).
/// * **SA5 — Files-skipped count**: `meta.files_skipped == 0`.
///
/// Selector substring-match: `cargo test -p ucil-daemon
/// server::test_type_check_tool` resolves uniquely without
/// `--exact`.
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_type_check_tool() {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use lsp_types::{
        CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall,
        Diagnostic as LspDiagnostic, DiagnosticSeverity, NumberOrString, Position, Range,
        TypeHierarchyItem, Url,
    };
    use ucil_lsp_diagnostics::{DiagnosticsClient, DiagnosticsClientError, SerenaClient};

    /// Helper: construct an `lsp_types::Diagnostic` from a compact
    /// set of fields.  Mirrors `quality_pipeline::test_fixtures::make_diag`
    /// but inlined here so the test does not pull a `pub(super)`-only
    /// helper across module boundaries.
    fn make_diag(
        severity: Option<DiagnosticSeverity>,
        source: Option<&str>,
        code: Option<NumberOrString>,
        message: &str,
    ) -> LspDiagnostic {
        LspDiagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            severity,
            code,
            code_description: None,
            source: source.map(str::to_owned),
            message: message.to_owned(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    /// `SerenaClient` impl scripted to return a fixed
    /// diagnostics-by-URI map.  Per `DEC-0008` §4 the [`SerenaClient`]
    /// trait is UCIL-internal so this is not a substitute for any
    /// external wire format — the same structural pattern as
    /// `quality_pipeline.rs::test_fixtures::ScriptedFakeSerenaClient`
    /// (now 6 WOs deep through WO-0048 / WO-0085 / WO-0089 / WO-0090).
    struct ScriptedFakeSerenaClient {
        diagnostics_by_uri: Mutex<Vec<(Url, Vec<LspDiagnostic>)>>,
    }

    impl ScriptedFakeSerenaClient {
        fn new(scripted: Vec<(Url, Vec<LspDiagnostic>)>) -> Self {
            Self {
                diagnostics_by_uri: Mutex::new(scripted),
            }
        }
    }

    #[async_trait]
    impl SerenaClient for ScriptedFakeSerenaClient {
        async fn diagnostics(
            &self,
            uri: Url,
        ) -> Result<Vec<LspDiagnostic>, DiagnosticsClientError> {
            let script = self
                .diagnostics_by_uri
                .lock()
                .expect("ScriptedFakeSerenaClient mutex poisoned")
                .clone();
            for (scripted_uri, diags) in script {
                if scripted_uri == uri {
                    return Ok(diags);
                }
            }
            Ok(Vec::new())
        }

        async fn call_hierarchy_incoming(
            &self,
            _item: CallHierarchyItem,
        ) -> Result<Vec<CallHierarchyIncomingCall>, DiagnosticsClientError> {
            Ok(Vec::new())
        }

        async fn call_hierarchy_outgoing(
            &self,
            _item: CallHierarchyItem,
        ) -> Result<Vec<CallHierarchyOutgoingCall>, DiagnosticsClientError> {
            Ok(Vec::new())
        }

        async fn type_hierarchy_supertypes(
            &self,
            _item: TypeHierarchyItem,
        ) -> Result<Vec<TypeHierarchyItem>, DiagnosticsClientError> {
            Ok(Vec::new())
        }
    }

    // Use absolute paths so `Url::from_file_path` returns Ok(_).
    // Three target files spanning Rust / Python / TypeScript.
    let rust_path = "/test/src/foo.rs";
    let py_path = "/test/src/bar.py";
    let ts_path = "/test/src/baz.ts";

    let rust_url = Url::from_file_path(rust_path)
        .expect("(precondition) absolute Rust path must convert to Url");
    let py_url = Url::from_file_path(py_path)
        .expect("(precondition) absolute Python path must convert to Url");
    let ts_url = Url::from_file_path(ts_path)
        .expect("(precondition) absolute TypeScript path must convert to Url");

    // Five raw diagnostics distributed across the three files:
    //
    // * Rust file → 3 diagnostics: 1 Error/rust-analyzer (type error,
    //   KEPT), 1 Error/clippy (lint error, FILTERED), 1
    //   Warning/rust-analyzer (lint warning, FILTERED)
    // * Python file → 1 diagnostic: 1 Error/pyright (type error, KEPT)
    // * TypeScript file → 1 diagnostic: 1 Error/tsserver (type error,
    //   KEPT)
    let rust_diags = vec![
        make_diag(
            Some(DiagnosticSeverity::ERROR),
            Some("rust-analyzer"),
            Some(NumberOrString::String("E0308".to_owned())),
            "mismatched types: expected `u32`, found `i32`",
        ),
        make_diag(
            Some(DiagnosticSeverity::ERROR),
            Some("clippy"),
            Some(NumberOrString::String("clippy::needless_return".to_owned())),
            "unneeded `return` statement",
        ),
        make_diag(
            Some(DiagnosticSeverity::WARNING),
            Some("rust-analyzer"),
            None,
            "unused variable: `x`",
        ),
    ];
    let py_diags = vec![make_diag(
        Some(DiagnosticSeverity::ERROR),
        Some("pyright"),
        Some(NumberOrString::String("reportGeneralTypeIssues".to_owned())),
        "Argument of type \"str\" cannot be assigned to parameter of type \"int\"",
    )];
    let ts_diags = vec![make_diag(
        Some(DiagnosticSeverity::ERROR),
        Some("tsserver"),
        Some(NumberOrString::String("TS2322".to_owned())),
        "Type 'string' is not assignable to type 'number'.",
    )];

    let scripted = vec![
        (rust_url, rust_diags),
        (py_url, py_diags),
        (ts_url, ts_diags),
    ];
    let fake: Arc<dyn SerenaClient + Send + Sync> =
        Arc::new(ScriptedFakeSerenaClient::new(scripted));
    let client = Arc::new(DiagnosticsClient::new(fake));
    let server = McpServer::new().with_diagnostics_client(client);

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 0xD1,
        "method": "tools/call",
        "params": {
            "name": "type_check",
            "arguments": {
                "files": [rust_path, py_path, ts_path],
                "reason": "verifier smoke",
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;
    assert_eq!(
        response.get("error"),
        None,
        "(precondition) handler must not return JSON-RPC error: {response}"
    );

    let text = response
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .expect("(precondition) response must carry result.content[0].text");
    let parsed: Value = serde_json::from_str(text)
        .expect("(precondition) result.content[0].text must be valid JSON");

    let errors_arr = parsed
        .get("errors")
        .and_then(Value::as_array)
        .expect("(precondition) parsed payload must carry errors[]");
    let meta = parsed
        .get("meta")
        .expect("(precondition) parsed payload must carry meta");

    // ── SA1 — errors[] length == 3 ──────────────────────────────────
    //
    // 5 raw diagnostics → 3 type errors after filter (rust-analyzer
    // E0308 + pyright reportGeneralTypeIssues + tsserver TS2322 all
    // KEPT; clippy lint error + Warning-severity rust-analyzer all
    // FILTERED).
    //
    // Mutation contract M3: dropping the `is_type_error_diagnostic`
    // filter (`.filter(|_| true)`) flips this assertion from PASS to
    // FAIL with `(SA1) errors[] length; left: 5, right: 3`.
    assert_eq!(
        errors_arr.len(),
        3,
        "(SA1) errors[] length; left: {}, right: 3",
        errors_arr.len()
    );

    // ── SA2 — every errors[].severity == "error" ────────────────────
    for (i, e) in errors_arr.iter().enumerate() {
        let sev = e.get("severity").and_then(Value::as_str);
        assert_eq!(
            sev,
            Some("error"),
            "(SA2) errors[{i}].severity == \"error\"; left: {sev:?}, right: \"error\""
        );
    }

    // ── SA3 — errors[].language covers {rust, python, typescript} ──
    let languages: HashSet<String> = errors_arr
        .iter()
        .filter_map(|e| e.get("language").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let expected: HashSet<String> = ["rust", "python", "typescript"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        languages, expected,
        "(SA3) errors[].language covers all 3 languages; left: {languages:?}, right: {expected:?}"
    );

    // ── SA4 — meta.files_checked == 3 ───────────────────────────────
    assert_eq!(
        meta.get("files_checked").and_then(Value::as_u64),
        Some(3),
        "(SA4) meta.files_checked == 3; left: {:?}, right: 3",
        meta.get("files_checked")
    );

    // ── SA5 — meta.files_skipped == 0 ───────────────────────────────
    assert_eq!(
        meta.get("files_skipped").and_then(Value::as_u64),
        Some(0),
        "(SA5) meta.files_skipped == 0; left: {:?}, right: 0",
        meta.get("files_skipped")
    );
}

// ── G4+G7+G8 review_changes MCP tool tests (P3-W11-F11) ──────────────────────
//
// One frozen `#[tokio::test]` selector lives at MODULE ROOT (NOT inside
// any inner `mod tests { … }`) so the substring-match selector
// `cargo test -p ucil-daemon server::test_review_changes_tool` resolves
// uniquely without `--exact` per DEC-0007 + WO-0067/0068 lessons §planner.
//
// The test injects deterministic in-process `TestG4Source` /
// `TestG7Source` / `TestG8Source` impls (UCIL's own dependency-inversion
// seam per DEC-0008 §4 — these are NOT mocks of any external wire
// format) and exercises `handle_review_changes` end-to-end through
// `handle_line` so the parallel `tokio::join!` 3-arity fan-out + merge
// projection is asserted over the real JSON-RPC envelope.

/// Frozen acceptance test for `P3-W11-F11` (`review_changes`).
///
/// Master-plan §3.2 row 13 + §5.4 + §5.7 + §5.8 + §18 Phase 3 Week 11
/// item 6.  Drives `handle_tools_call` end-to-end through
/// [`McpServer::handle_review_changes`] with deterministic in-process
/// [`G4Source`] + [`G7Source`] + [`G8Source`] impls and asserts SA1-SA8:
///
/// * **SA1 — Findings count**: 3 G7 issues + 2 G4 blast-radius nodes →
///   `findings[].len() == 5`.  G8 contributes only to
///   `untested_functions[]` (NOT `findings[]`).
/// * **SA2 — Severity-rank invariant**: `findings[0].severity ==
///   "critical"` AND `findings[]` is sorted descending by severity
///   weight (Critical=4, High=3, Medium=2, Low=1, Info=0).
/// * **SA3 — Source-group provenance**: collected `findings[].source_group`
///   set covers AT LEAST `{"quality", "architecture"}`.
/// * **SA4 — Untested-function count**: 2 G8 candidates with distinct
///   `test_path` values → `untested_functions[].len() == 2`.
/// * **SA5 — Blast-radius impacted count**: 2 nodes reachable from
///   the seed file `src/foo.rs` via the seeded G4 edges →
///   `blast_radius.impacted[].len() == 2`.
/// * **SA6 — Master deadline did not trip**: all three G4 + G7 + G8
///   sources return immediately under their masters →
///   `meta.master_timed_out == false`.
/// * **SA7 — Parallelism wall-clock canary**:
///   `meta.wall_elapsed_ms < 6000` ms.  Sequential awaits over the
///   3 G-source masters (G4=12s, G7=5.5s, G8=5s) would compound past
///   6000 ms — wall-clock guard catches sequential-await regressions.
/// * **SA8 — Finding shape integrity**: every entry in `findings[]`
///   carries the required keys (`severity`, `category`, `source_group`,
///   `file`, `line`, `message`) AND every `severity` is lowercase per
///   §5.7 + §12.1 vocabulary canary.
///
/// Selector substring-match: `cargo test -p ucil-daemon
/// server::test_review_changes_tool` resolves uniquely without
/// `--exact`.
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_review_changes_tool() {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::g4::{G4DependencyEdge, G4EdgeKind, G4EdgeOrigin};
    use crate::g7::{G7Issue, G7SourceOutput, G7SourceStatus, Severity};
    use crate::g8::{G8TestCandidate, TestDiscoveryMethod};

    /// Local [`G7Source`] impl returning a pre-canned issue list.
    /// Per `DEC-0008` §4 the [`G7Source`] trait is UCIL-internal so
    /// this is not a substitute for any external wire format —
    /// matches the `test_check_quality_tool` precedent (now 6+ WOs
    /// deep through WO-0085 / WO-0089 / WO-0090).
    struct TestG7Source {
        id: String,
        issues: Vec<G7Issue>,
    }

    #[async_trait::async_trait]
    impl G7Source for TestG7Source {
        fn source_id(&self) -> &str {
            &self.id
        }

        async fn execute(&self, _query: &G7Query) -> G7SourceOutput {
            G7SourceOutput {
                source_id: self.id.clone(),
                status: G7SourceStatus::Available,
                elapsed_ms: 0,
                issues: self.issues.clone(),
                error: None,
            }
        }
    }

    /// Local [`G8Source`] impl returning a pre-canned candidate list.
    /// Same `DEC-0008` §4 carve-out as `TestG7Source` above —
    /// matches the `test_check_quality_tool` precedent.
    struct TestG8Source {
        id: String,
        method: TestDiscoveryMethod,
        candidates: Vec<G8TestCandidate>,
    }

    #[async_trait::async_trait]
    impl G8Source for TestG8Source {
        fn source_id(&self) -> String {
            self.id.clone()
        }

        fn method(&self) -> TestDiscoveryMethod {
            self.method
        }

        async fn execute(&self, _query: &G8Query) -> Result<Vec<G8TestCandidate>, String> {
            Ok(self.candidates.clone())
        }
    }

    // ── G4 seed: 2 edges from `src/foo.rs` so the merged
    // `project_blast_radius_impacted` BFS yields exactly 2 impacted
    // nodes (the two targets, depth=1) — matches scope_in #5.a.
    let g4_edges = vec![
        G4DependencyEdge {
            source: "src/foo.rs".to_owned(),
            target: "src/foo_helper.rs".to_owned(),
            edge_kind: G4EdgeKind::Import,
            source_id: "test-g4-source".to_owned(),
            origin: G4EdgeOrigin::Inferred,
            coupling_weight: 0.9,
        },
        G4DependencyEdge {
            source: "src/foo.rs".to_owned(),
            target: "src/foo_dep.rs".to_owned(),
            edge_kind: G4EdgeKind::Call,
            source_id: "test-g4-source".to_owned(),
            origin: G4EdgeOrigin::Inferred,
            coupling_weight: 0.7,
        },
    ];
    let g4_src: Arc<dyn G4Source> = Arc::new(TestG4Source {
        id: "test-g4-source".to_owned(),
        edges: g4_edges,
    });
    let g4_sources: Arc<Vec<Arc<dyn G4Source>>> = Arc::new(vec![g4_src]);

    // ── G7 seed: 3 G7 issues spanning Critical / High / Medium
    // severities, each anchored to a distinct
    // `(file_path, line_start, category)` key so the
    // severity-weighted merge preserves all three groups in the
    // output (no group collapse) — matches scope_in #5.b +
    // `test_check_quality_tool` precedent.
    let g7_issues = vec![
        G7Issue {
            source_tool: "lsp:rust-analyzer".to_owned(),
            file_path: "src/foo.rs".to_owned(),
            line_start: Some(10),
            line_end: Some(10),
            category: "type_error".to_owned(),
            severity: Severity::Critical,
            message: "borrow checker error".to_owned(),
            rule_id: Some("E0382".to_owned()),
            fix_suggestion: Some("clone the borrow".to_owned()),
        },
        G7Issue {
            source_tool: "eslint".to_owned(),
            file_path: "src/foo.rs".to_owned(),
            line_start: Some(20),
            line_end: Some(20),
            category: "lint".to_owned(),
            severity: Severity::High,
            message: "no-unused-vars".to_owned(),
            rule_id: Some("no-unused-vars".to_owned()),
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "ruff".to_owned(),
            file_path: "src/bar.py".to_owned(),
            line_start: Some(30),
            line_end: Some(30),
            category: "lint".to_owned(),
            severity: Severity::Medium,
            message: "F401: unused import".to_owned(),
            rule_id: Some("F401".to_owned()),
            fix_suggestion: None,
        },
    ];
    let g7_src: Arc<dyn G7Source + Send + Sync> = Arc::new(TestG7Source {
        id: "test-g7-source".to_owned(),
        issues: g7_issues,
    });
    let g7_sources: Arc<Vec<Arc<dyn G7Source + Send + Sync>>> = Arc::new(vec![g7_src]);

    // ── G8 seed: 2 G8 candidates with distinct `test_path` values so
    // the dedup-by-test-path merge yields exactly 2 output rows —
    // matches scope_in #5.c + `test_check_quality_tool` precedent.
    let g8_candidates = vec![
        G8TestCandidate {
            test_path: PathBuf::from("tests/test_foo_login.rs"),
            source_path: Some(PathBuf::from("src/foo.rs")),
            method: TestDiscoveryMethod::Convention,
            confidence: 0.95,
        },
        G8TestCandidate {
            test_path: PathBuf::from("tests/test_foo_logout.rs"),
            source_path: Some(PathBuf::from("src/foo.rs")),
            method: TestDiscoveryMethod::Import,
            confidence: 0.85,
        },
    ];
    let g8_src: Arc<dyn G8Source + Send + Sync> = Arc::new(TestG8Source {
        id: "test-g8-source".to_owned(),
        method: TestDiscoveryMethod::Convention,
        candidates: g8_candidates,
    });
    let g8_sources: Arc<Vec<Arc<dyn G8Source + Send + Sync>>> = Arc::new(vec![g8_src]);

    let server = McpServer::new()
        .with_g4_sources(g4_sources)
        .with_g7_sources(g7_sources)
        .with_g8_sources(g8_sources);

    let request = json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": 0xD1,
        "method": "tools/call",
        "params": {
            "name": "review_changes",
            "arguments": {
                "changed_files": ["src/foo.rs", "src/bar.py"],
                "reason": "verifier smoke",
            }
        }
    })
    .to_string();
    let response = server.handle_line(&request).await;
    assert_eq!(
        response.get("error"),
        None,
        "(precondition) handler must not return JSON-RPC error: {response}"
    );

    let text = response
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .expect("(precondition) response must carry result.content[0].text");
    let parsed: Value = serde_json::from_str(text)
        .expect("(precondition) result.content[0].text must be valid JSON");

    let findings_arr = parsed
        .get("findings")
        .and_then(Value::as_array)
        .expect("(precondition) parsed payload must carry findings[]");
    let untested_arr = parsed
        .get("untested_functions")
        .and_then(Value::as_array)
        .expect("(precondition) parsed payload must carry untested_functions[]");
    let blast_radius = parsed
        .get("blast_radius")
        .expect("(precondition) parsed payload must carry blast_radius");
    let impacted_arr = blast_radius
        .get("impacted")
        .and_then(Value::as_array)
        .expect("(precondition) blast_radius.impacted must be an array");
    let meta = parsed
        .get("meta")
        .expect("(precondition) parsed payload must carry meta");

    // ── SA1 — findings[] length == 5 (3 G7 + 2 G4) ──────────────────
    assert_eq!(
        findings_arr.len(),
        5,
        "(SA1) findings[] length; left: {}, right: 5",
        findings_arr.len()
    );

    // ── SA2 — findings sorted descending by severity, top critical ──
    assert_eq!(
        findings_arr[0].get("severity").and_then(Value::as_str),
        Some("critical"),
        "(SA2) findings[0].severity == \"critical\"; left: {:?}, right: \"critical\"",
        findings_arr[0].get("severity")
    );
    fn sa2_weight(s: &str) -> i8 {
        match s {
            "critical" => 4,
            "high" => 3,
            "medium" => 2,
            "low" => 1,
            "info" => 0,
            _ => -1,
        }
    }
    for i in 0..findings_arr.len().saturating_sub(1) {
        let lhs = findings_arr[i]
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("");
        let rhs = findings_arr[i + 1]
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(
            sa2_weight(lhs) >= sa2_weight(rhs),
            "(SA2) findings[{i}].severity weight >= findings[{}].severity weight (descending sort invariant); left: {} ({lhs:?}), right: {} ({rhs:?})",
            i + 1,
            sa2_weight(lhs),
            sa2_weight(rhs),
        );
    }

    // ── SA3 — findings[].source_group covers {quality, architecture} ─
    let source_groups: BTreeSet<String> = findings_arr
        .iter()
        .filter_map(|f| {
            f.get("source_group")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert!(
        source_groups.contains("quality"),
        "(SA3) findings[].source_group set contains \"quality\"; left: {source_groups:?}, right: contains \"quality\""
    );
    assert!(
        source_groups.contains("architecture"),
        "(SA3) findings[].source_group set contains \"architecture\"; left: {source_groups:?}, right: contains \"architecture\""
    );

    // ── SA4 — untested_functions[] length == 2 ──────────────────────
    assert_eq!(
        untested_arr.len(),
        2,
        "(SA4) untested_functions[] length; left: {}, right: 2",
        untested_arr.len()
    );

    // ── SA5 — blast_radius.impacted[] length == 2 ───────────────────
    assert_eq!(
        impacted_arr.len(),
        2,
        "(SA5) blast_radius.impacted[] length; left: {}, right: 2",
        impacted_arr.len()
    );

    // ── SA6 — meta.master_timed_out == false ────────────────────────
    assert_eq!(
        meta.get("master_timed_out").and_then(Value::as_bool),
        Some(false),
        "(SA6) meta.master_timed_out == false; left: {:?}, right: false",
        meta.get("master_timed_out")
    );

    // ── SA7 — meta.wall_elapsed_ms < 6000 (parallelism canary) ──────
    let wall_elapsed = meta
        .get("wall_elapsed_ms")
        .and_then(Value::as_u64)
        .expect("(SA7 precondition) meta.wall_elapsed_ms must be a u64");
    assert!(
        wall_elapsed < 6000,
        "(SA7) meta.wall_elapsed_ms < 6000 ms (parallelism canary — sequential awaits over G4+G7+G8 per-group masters would compound); left: {wall_elapsed}, right: < 6000"
    );

    // ── SA8 — finding shape integrity (every required field set,
    // severity is lowercase) ────────────────────────────────────────
    for (i, finding) in findings_arr.iter().enumerate() {
        let sev = finding.get("severity").and_then(Value::as_str);
        assert!(
            matches!(
                sev,
                Some("critical" | "high" | "medium" | "low" | "info")
            ),
            "(SA8) findings[{i}].severity is lowercase canonical vocabulary; left: {sev:?}, right: one of [critical, high, medium, low, info]"
        );
        assert!(
            finding.get("category").is_some(),
            "(SA8) findings[{i}].category present; left: None, right: Some(_)"
        );
        assert!(
            finding.get("source_group").is_some(),
            "(SA8) findings[{i}].source_group present; left: None, right: Some(_)"
        );
        assert!(
            finding.get("file").is_some(),
            "(SA8) findings[{i}].file present; left: None, right: Some(_)"
        );
        // `line` may be `Value::Null` for blast-radius rows — assert
        // the key is present (Value::Null counts as present) but
        // do NOT require `is_some()` to be a JSON value.  The Value
        // shape covers `null` so `.get("line").is_some()` passes
        // even when the field is `Value::Null`.
        assert!(
            finding.get("line").is_some(),
            "(SA8) findings[{i}].line present; left: None, right: Some(_)"
        );
        assert!(
            finding.get("message").is_some(),
            "(SA8) findings[{i}].message present; left: None, right: Some(_)"
        );
    }
}
