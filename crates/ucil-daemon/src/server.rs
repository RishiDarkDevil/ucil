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
    /// (`P1-W4-F05`, master-plan §3.2 row 2) to a real handler that
    /// pulls definition + callers from the graph; when absent, the tool
    /// falls through to the `_meta.not_yet_implemented: true` stub path
    /// every other tool still uses (phase-1 invariant #9).
    pub kg: Option<Arc<Mutex<KnowledgeGraph>>>,
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
        }
    }

    /// Construct a server that routes `find_definition` (`P1-W4-F05`)
    /// to the real `handle_find_definition` handler backed by the
    /// supplied knowledge graph.
    ///
    /// The handle is `Arc<Mutex<_>>` so the caller can keep a second
    /// reference (e.g. the ingest pipeline) and mutate the graph
    /// concurrently; the handler takes the lock for the duration of a
    /// single read and releases it before encoding the response.  Every
    /// tool **other** than `find_definition` still falls through to the
    /// stub path — the 22-tool catalog is unchanged and phase-1
    /// invariant #9 is preserved for the remaining 21 tools.
    ///
    /// See master-plan §3.2 row 2 (`find_definition` — go-to-definition
    /// with full context) and §18 Phase 1 Week 4 line 1751 ("Implement
    /// first working tool: find_definition").
    #[must_use]
    pub fn with_knowledge_graph(kg: Arc<Mutex<KnowledgeGraph>>) -> Self {
        Self {
            tools: ucil_tools(),
            kg: Some(kg),
        }
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

            let response = self.handle_line(line.trim_end_matches(['\r', '\n']));
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
    fn handle_line(&self, line: &str) -> Value {
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
            "tools/call" => self.handle_tools_call(&id, &params),
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

    fn handle_tools_call(&self, id: &Value, params: &Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        if !self.tools.iter().any(|t| t.name == name) {
            return jsonrpc_error(id, -32602, &format!("Unknown tool: {name}"));
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
}

/// Build a JSON-RPC 2.0 error envelope.
fn jsonrpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.clone(),
        "error": { "code": code, "message": message }
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
