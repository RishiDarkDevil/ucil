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

use crate::g2_search::G2SourceFactory;
use crate::text_search::{self, TextMatch, TextSearchError};
use ucil_core::{fuse_g2_rrf, G2FusedOutcome, G2SourceResults};

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

/// UCIL's MCP server over newline-delimited JSON-RPC 2.0.
///
/// Tool *dispatch* is still a Phase-2 concern (G1/G2 fusion) — every
/// `tools/call` handler in this skeleton returns a stub envelope
/// carrying `_meta.not_yet_implemented: true`, per phase-1 invariant
/// #9.  What this type **does** provide, and what the verifier tests,
/// is a working wire protocol: a host agent can `initialize`, list the
/// 22 UCIL tools, and receive structured (stub) responses on
/// `tools/call`.
#[derive(Debug, Clone)]
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
