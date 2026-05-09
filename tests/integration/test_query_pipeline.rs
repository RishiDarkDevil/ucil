#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
// Doc-comment first-paragraph length is governed by the SA-numbered
// rustdoc convention (`DEC-0007`). The frozen test fn carries an
// explicit `# Panics` section so the global allow is only needed for
// the helper rustdoc.
#![allow(clippy::too_long_first_doc_paragraph)]
//! `P3-W11-F16a` — full-pipeline cross-group MCP integration test
//! binary.
//!
//! Master-plan §3.2 row 8 (`get_architecture` MCP tool — fans out
//! through the G4 (Architecture) backbone). Master-plan §3.4 +
//! §6.2 lines 643-658 — cross-group fusion + RRF formula with the
//! query-type weight matrix. Master-plan §6.1 line 606 — degraded
//! groups + per-group timeout.
//!
//! # Coverage axis
//!
//! One module-root `#[tokio::test]` exercises the [`McpServer`]'s
//! full `with_g4_sources` + `with_g7_sources` + `with_g8_sources`
//! builder chain, drives `get_architecture` end-to-end through the
//! real [`McpServer::serve`] loop over a `tokio::io::duplex` pair,
//! and asserts the response surfaces the structured G4 fan-out
//! envelope (`_meta.modules`, `_meta.source ==
//! "g4-architecture-fanout"`, `_meta.source_results`) — i.e. that
//! the `with_g4_sources` builder is wired through the dispatcher.
//!
//! `_meta.modules` is the canonical empirical surface that
//! distinguishes the `with_g4_sources`-enabled response from the
//! phase-1 `_meta.not_yet_implemented: true` fall-through path —
//! it is the load-bearing field that flips when M3 (drop
//! `with_g4_sources`) is applied.
//!
//! Sub-assertions (`DEC-0007` SA-numbered):
//!
//! * **SA1** — Response carries `_meta.modules` (only present when
//!   `with_g4_sources(...)` is wired into the [`McpServer`];
//!   absent under the `_meta.not_yet_implemented` fall-through
//!   path). Mutation-targeted: M3 drops `with_g4_sources(...)` →
//!   SA1 trips.
//! * **SA2** — Response carries `_meta.source ==
//!   "g4-architecture-fanout"` — the real-handler emit per
//!   server.rs:1538 (the fall-through path emits `_meta.tool`
//!   only, no `_meta.source`).
//! * **SA3** — `_meta.source_results[0].source_id` names a seeded
//!   source — the cross-group provenance retention contract per
//!   master-plan §6.1 line 606.
//!
//! # Trait-seam carve-out (`DEC-0008` §4)
//!
//! `SeededG4Source` / `SeededG7Source` / `SeededG8Source` impls
//! below are UCIL-owned trait realisations — the trait IS the
//! dependency-inversion seam. Production impls (e.g.
//! `CodeGraphContextG4Source`, `LSPCallHierarchyG4Source`) live in
//! `crates/ucil-daemon/` and arrive in follow-up production-wiring
//! WOs.
//!
//! # File layout (`DEC-0010`) and frozen-test placement (`DEC-0007`)
//!
//! Binary lives at `tests/integration/test_query_pipeline.rs`
//! (master-plan §17.2 line 1693). The frozen `pub async fn` lives
//! at module ROOT (no nested `mod tests { … }` wrapper) so the
//! substring-match selector
//! `cargo test --test test_query_pipeline test_query_pipeline_returns_fused_results_with_group_provenance`
//! resolves directly without a `tests::` path prefix.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{duplex, split, AsyncBufReadExt, AsyncWriteExt, BufReader};

use ucil_daemon::g4::{
    G4DependencyEdge, G4EdgeKind, G4EdgeOrigin, G4Query, G4Source, G4SourceOutput, G4SourceStatus,
};
use ucil_daemon::g7::{G7Issue, G7Query, G7Source, G7SourceOutput, G7SourceStatus, Severity};
use ucil_daemon::g8::{G8Query, G8Source, G8TestCandidate, TestDiscoveryMethod};
use ucil_daemon::server::McpServer;

// ── Test-side `G4Source` impl — `DEC-0008` §4 dep-inversion seam ──────────

/// UCIL-owned [`G4Source`] realisation seeded with a deterministic
/// list of [`G4DependencyEdge`] rows. The trait IS the
/// dependency-inversion seam — production impls
/// (`CodeGraphContextG4Source`, `LSPCallHierarchyG4Source`,
/// `DependencyCruiserG4Source`) live in `crates/ucil-daemon/`.
struct SeededG4Source {
    source_id: String,
    edges: Vec<G4DependencyEdge>,
}

#[async_trait]
impl G4Source for SeededG4Source {
    fn source_id(&self) -> &str {
        &self.source_id
    }

    async fn execute(&self, _query: &G4Query) -> G4SourceOutput {
        G4SourceOutput {
            source_id: self.source_id.clone(),
            status: G4SourceStatus::Available,
            elapsed_ms: 0,
            edges: self.edges.clone(),
            error: None,
        }
    }
}

// ── Test-side `G7Source` impl ─────────────────────────────────────────────

/// UCIL-owned [`G7Source`] realisation paired with the G4 + G8
/// builders to exercise the full [`McpServer`] dispatcher surface.
struct SeededG7Source {
    source_id: String,
    issues: Vec<G7Issue>,
}

#[async_trait]
impl G7Source for SeededG7Source {
    fn source_id(&self) -> &str {
        &self.source_id
    }

    async fn execute(&self, _query: &G7Query) -> G7SourceOutput {
        G7SourceOutput {
            source_id: self.source_id.clone(),
            status: G7SourceStatus::Available,
            elapsed_ms: 0,
            issues: self.issues.clone(),
            error: None,
        }
    }
}

// ── Test-side `G8Source` impl ─────────────────────────────────────────────

/// UCIL-owned [`G8Source`] realisation paired with the G4 + G7
/// builders.
struct SeededG8Source {
    source_id: String,
    method: TestDiscoveryMethod,
    candidates: Vec<G8TestCandidate>,
}

#[async_trait]
impl G8Source for SeededG8Source {
    fn source_id(&self) -> String {
        self.source_id.clone()
    }

    fn method(&self) -> TestDiscoveryMethod {
        self.method
    }

    async fn execute(&self, _query: &G8Query) -> Result<Vec<G8TestCandidate>, String> {
        Ok(self.candidates.clone())
    }
}

// ── F16a frozen test ──────────────────────────────────────────────────────

/// Drives `get_architecture` end-to-end through [`McpServer::serve`]
/// to assert the full-pipeline cross-group dispatch surface.
///
/// Constructs an [`McpServer`] with three builders chained:
/// `with_g4_sources(...)` (load-bearing — `get_architecture`'s
/// `_meta.modules` projection only fires when this is wired),
/// `with_g7_sources(...)` (corroborates the multi-builder chain
/// compiles + dispatches), and `with_g8_sources(...)` (same).
///
/// The G4 source seeds 4 dependency edges spanning 4 distinct node
/// names + 4 distinct edge kinds (`Import / Call / Implements /
/// Inherits`) so the merger projects all 4 onto `_meta.modules`.
/// The G7 + G8 source seeds are minimal — they are not load-bearing
/// for the `get_architecture`-side assertions.
///
/// The test wraps the entire JSON-RPC exchange in
/// `tokio::time::timeout(Duration::from_secs(30), ...)` as a
/// belt-and-braces hang guard.
///
/// # Panics
///
/// Panics with an `(SAn) ...` body on any sub-assertion failure or
/// a `(precondition) ...` body on JSON-RPC transport failures —
/// `DEC-0007` panic-message convention from
/// WO-0067/0083/0085/0089/0090/0093 frozen-test precedent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::missing_panics_doc)] // panic semantics described in the rustdoc # Panics section above.
pub async fn test_query_pipeline_returns_fused_results_with_group_provenance() {
    tokio::time::timeout(Duration::from_secs(30), run_query_pipeline_assertions())
        .await
        .expect("(precondition) query-pipeline test must finish within 30 s wall-clock");
}

#[allow(clippy::too_many_lines)]
async fn run_query_pipeline_assertions() {
    // Four edges spanning four distinct node names — the §5.4
    // architecture surface that `_meta.modules` projects. `Import`
    // edge carries the highest coupling weight (0.9) so the merge
    // ordering keeps it deterministic.
    let g4_edges = vec![
        G4DependencyEdge {
            source: "auth_module".to_owned(),
            target: "session_module".to_owned(),
            edge_kind: G4EdgeKind::Import,
            source_id: "seeded-g4-source".to_owned(),
            origin: G4EdgeOrigin::Inferred,
            coupling_weight: 0.9,
        },
        G4DependencyEdge {
            source: "session_module".to_owned(),
            target: "cache_module".to_owned(),
            edge_kind: G4EdgeKind::Call,
            source_id: "seeded-g4-source".to_owned(),
            origin: G4EdgeOrigin::Inferred,
            coupling_weight: 0.7,
        },
        G4DependencyEdge {
            source: "cache_module".to_owned(),
            target: "store_module".to_owned(),
            edge_kind: G4EdgeKind::Implements,
            source_id: "seeded-g4-source".to_owned(),
            origin: G4EdgeOrigin::Inferred,
            coupling_weight: 0.6,
        },
        G4DependencyEdge {
            source: "auth_module".to_owned(),
            target: "store_module".to_owned(),
            edge_kind: G4EdgeKind::Inherits,
            source_id: "seeded-g4-source".to_owned(),
            origin: G4EdgeOrigin::Inferred,
            coupling_weight: 0.4,
        },
    ];

    let g4_source: Arc<dyn G4Source> = Arc::new(SeededG4Source {
        source_id: "seeded-g4-source".to_owned(),
        edges: g4_edges,
    });

    let g7_source: Arc<dyn G7Source + Send + Sync> = Arc::new(SeededG7Source {
        source_id: "seeded-g7-source".to_owned(),
        issues: vec![G7Issue {
            source_tool: "lsp:rust-analyzer".to_owned(),
            file_path: "src/auth.rs".to_owned(),
            line_start: Some(10),
            line_end: Some(10),
            category: "type_error".to_owned(),
            severity: Severity::High,
            message: "type-check error".to_owned(),
            rule_id: Some("E0308".to_owned()),
            fix_suggestion: None,
        }],
    });

    let g8_source: Arc<dyn G8Source + Send + Sync> = Arc::new(SeededG8Source {
        source_id: "seeded-g8-source".to_owned(),
        method: TestDiscoveryMethod::Convention,
        candidates: vec![G8TestCandidate {
            test_path: PathBuf::from("tests/test_auth_pipeline.rs"),
            source_path: Some(PathBuf::from("src/auth.rs")),
            method: TestDiscoveryMethod::Convention,
            confidence: 0.9,
        }],
    });

    let server = McpServer::new()
        .with_g4_sources(Arc::new(vec![g4_source]))
        .with_g7_sources(Arc::new(vec![g7_source]))
        .with_g8_sources(Arc::new(vec![g8_source]));

    let (client_end, server_end) = duplex(16 * 1024);
    let (server_read, server_write) = split(server_end);
    let (client_read, mut client_write) = split(client_end);

    let server_task = tokio::spawn(async move { server.serve(server_read, server_write).await });

    // `get_architecture` is the load-bearing dispatch path —
    // `_meta.modules` is only emitted when `with_g4_sources(...)`
    // is wired (server.rs:1532-1544).
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0xD16,
        "method": "tools/call",
        "params": {
            "name": "get_architecture",
            "arguments": {
                "target": "auth_module",
                "max_depth": 4,
                "max_edges": 256,
                "reason": "verifier smoke",
            }
        }
    });
    let mut request_line =
        serde_json::to_vec(&request).expect("(precondition) request JSON must serialise");
    request_line.push(b'\n');

    client_write
        .write_all(&request_line)
        .await
        .expect("(precondition) client_write must accept request");
    client_write
        .flush()
        .await
        .expect("(precondition) client_write must flush request");

    let mut reader = BufReader::new(client_read);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .await
        .expect("(precondition) reader must yield response line");

    client_write
        .shutdown()
        .await
        .expect("(precondition) client_write shutdown must succeed");
    drop(client_write);
    let serve_result = tokio::time::timeout(Duration::from_secs(5), server_task)
        .await
        .expect("(precondition) server task must finish within 5 s of EOF")
        .expect("(precondition) server task must not panic");
    serve_result.expect("(precondition) server loop must return Ok after clean EOF");

    let response: Value = serde_json::from_str(response_line.trim())
        .expect("(precondition) response line must be valid JSON-RPC");
    assert_eq!(
        response.get("error"),
        None,
        "(precondition) handler must not emit JSON-RPC error envelope: {response}"
    );

    let meta = response
        .pointer("/result/_meta")
        .expect("(precondition) response must carry result._meta");

    // ── SA1 — _meta.modules present (mutation-targeted via M3) ──
    let modules = meta.get("modules").and_then(Value::as_array);
    assert!(
        modules.is_some(),
        "(SA1) _meta.modules present (only emitted when `with_g4_sources(...)` wired into McpServer); left: <absent>, right: <present array>"
    );
    let modules = modules.expect("(SA1 precondition) _meta.modules guarded by is_some() above");
    assert!(
        modules.len() >= 4,
        "(SA1) _meta.modules covers all 4 seeded distinct node names; left: {} modules, right: ≥ 4",
        modules.len()
    );

    // ── SA2 — _meta.source == "g4-architecture-fanout" ─────────────
    assert_eq!(
        meta.get("source").and_then(Value::as_str),
        Some("g4-architecture-fanout"),
        "(SA2) _meta.source == \"g4-architecture-fanout\"; left: {:?}, right: \"g4-architecture-fanout\"",
        meta.get("source")
    );

    // ── SA3 — _meta.source_results[0].source_id names seeded source ─
    let source_results = meta
        .get("source_results")
        .and_then(Value::as_array)
        .expect("(SA3 precondition) _meta.source_results must be a JSON array");
    assert!(
        !source_results.is_empty(),
        "(SA3 precondition) _meta.source_results non-empty; left: empty, right: ≥ 1 entry"
    );
    let provenance_id = source_results[0]
        .get("source_id")
        .and_then(Value::as_str)
        .unwrap_or("<missing>");
    assert_eq!(
        provenance_id, "seeded-g4-source",
        "(SA3) _meta.source_results[0].source_id names the seeded G4 source — cross-group provenance retention; left: {provenance_id:?}, right: \"seeded-g4-source\""
    );
}
