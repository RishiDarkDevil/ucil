#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
// Doc-comment first-paragraph length is governed by the SA-numbered
// rustdoc convention (`DEC-0007`), not the default 200-char clippy
// budget; suppressing matches WO-0070/0085/0089/0090/0093 frozen-test
// precedent. The frozen test fn carries an explicit `# Panics`
// section so the global allow is only needed for the helper rustdoc.
#![allow(clippy::too_long_first_doc_paragraph)]
//! `P3-W11-F13` — quality-pipeline MCP integration test binary.
//!
//! Master-plan §3.2 row 14 (`check_quality` MCP tool — runs lint /
//! type-check / security scan and returns a `{ issues[],
//! untested_functions[], meta }` payload). Master-plan §5.7 lines
//! 539-559 — G7 (Quality) parallel fan-out under a 5-6 s master
//! deadline. Master-plan §5.7 line 555 + §12.1 — severity ladder
//! `critical > high > medium > low > info`, lowercase serialisation.
//!
//! # Coverage axis
//!
//! One module-root `#[tokio::test]` drives the `check_quality` MCP
//! tool end-to-end through the real `McpServer::serve` loop over a
//! `tokio::io::duplex` pair (no in-process `handle_line` shortcut),
//! exercising the parallel `tokio::join!(execute_g7, execute_g8)`
//! fan-out + severity-weighted merger + JSON-RPC envelope encoder.
//!
//! Assertions cover (`DEC-0007` SA-numbered):
//!
//! * **SA1** — `issues[]` length is at least 3 (the spec floor for
//!   `check_quality`'s usefulness — fewer than 3 issues at
//!   distinct `(file, line, category)` keys means the merger
//!   collapsed unexpectedly).
//! * **SA2** — Every `issues[i].severity` is in the lowercase
//!   master-plan §5.7 / §12.1 vocabulary `{critical, high, medium,
//!   low, info}`.
//! * **SA3** — `meta.master_timed_out` is `false` under the default
//!   5500 ms / 5000 ms G7 / G8 masters with sources that return
//!   immediately.
//!
//! # Trait-seam carve-out (`DEC-0008` §4)
//!
//! The test-side `SeededG7Source` / `SeededG8Source` impls below are
//! UCIL-owned `G7Source` / `G8Source` trait realisations — the
//! trait IS the dependency-inversion seam. Production impls live
//! in `crates/ucil-daemon/` and arrive in follow-up production-
//! wiring WOs that bundle the daemon-startup orchestration; this
//! test file ships only the integration-test surface.
//!
//! # File layout (`DEC-0010`)
//!
//! Binary lives at `tests/integration/test_quality_pipeline.rs` per
//! the workspace convention pinned by the `[[test]]` entry in
//! `tests/integration/Cargo.toml`. Master-plan §17.2 line 1693
//! lists `test_quality_pipeline.rs` under `tests/integration/`.
//!
//! # Frozen-test placement (`DEC-0007`)
//!
//! `pub async fn test_quality_pipeline_detects_severity_classified_issues`
//! lives at module ROOT (no nested `mod tests { … }` wrapper) so
//! the substring-match selector `cargo test --test
//! test_quality_pipeline test_quality_pipeline_detects_severity_classified_issues`
//! resolves directly without a `tests::` path prefix.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{duplex, split, AsyncBufReadExt, AsyncWriteExt, BufReader};

use ucil_daemon::g7::{G7Issue, G7Query, G7Source, G7SourceOutput, G7SourceStatus, Severity};
use ucil_daemon::g8::{G8Query, G8Source, G8TestCandidate, TestDiscoveryMethod};
use ucil_daemon::server::McpServer;

// ── Test-side `G7Source` / `G8Source` impls — `DEC-0008` §4 seam ──────────

/// UCIL-owned `G7Source` realisation seeded with a deterministic
/// list of `G7Issue` rows. The trait IS the dependency-inversion
/// seam — production impls (e.g. `LspDiagnosticsG7Source`,
/// `EslintG7Source`, `RuffG7Source`, `SemgrepG7Source`) live in
/// `crates/ucil-daemon/` and arrive in follow-up production-
/// wiring WOs.
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

/// UCIL-owned `G8Source` realisation seeded with deterministic
/// `G8TestCandidate` rows. Same `DEC-0008` §4 trait-seam
/// rationale as `SeededG7Source`. Production impls
/// (`ConventionG8Source`, `ImportG8Source`, `KgRelationsG8Source`)
/// live in `crates/ucil-daemon/`.
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

// ── F13 frozen test ──────────────────────────────────────────────────────

/// Drives `check_quality` end-to-end through `McpServer::serve`.
///
/// Constructs a `SeededG7Source` returning 5 issues spanning
/// `Critical / High / Medium / Low / Info` severities and a
/// `SeededG8Source` returning a single test candidate (the G8
/// pairing satisfies the `check_quality` dispatch precondition but
/// is not load-bearing for this test's quality-side assertions).
/// The 5 issues are anchored at distinct `(file_path, line_start,
/// category)` keys so the severity-weighted merge preserves all 5
/// in the output (no group collapse).
///
/// The test wraps the entire JSON-RPC exchange in
/// `tokio::time::timeout(Duration::from_secs(30), ...)` as a
/// belt-and-braces hang guard; the daemon's per-source / per-master
/// deadlines already cap the inner work at well under 6 s.
///
/// # Panics
///
/// Panics with an `(SAn) ...` body if any sub-assertion fails, or
/// with a `(precondition) ...` body if the JSON-RPC exchange itself
/// fails (transport hang, missing response field, etc.) — the
/// `DEC-0007` panic-message convention carries forward from
/// WO-0067 / WO-0083 / WO-0085 / WO-0089 / WO-0090 / WO-0093.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::missing_panics_doc)] // panic semantics described in the rustdoc # Panics section above.
pub async fn test_quality_pipeline_detects_severity_classified_issues() {
    tokio::time::timeout(Duration::from_secs(30), run_quality_pipeline_assertions())
        .await
        .expect("(precondition) quality-pipeline test must finish within 30 s wall-clock");
}

#[allow(clippy::too_many_lines)]
async fn run_quality_pipeline_assertions() {
    // Five issues spanning every severity rung in the master-plan
    // §5.7 ladder. Distinct `(file_path, line_start, category)`
    // keys keep the severity-weighted merger from collapsing any
    // group — output count == input count.
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
        G7Issue {
            source_tool: "semgrep".to_owned(),
            file_path: "src/auth.rs".to_owned(),
            line_start: Some(40),
            line_end: Some(40),
            category: "security".to_owned(),
            severity: Severity::Low,
            message: "informational hint".to_owned(),
            rule_id: Some("info-hint".to_owned()),
            fix_suggestion: None,
        },
        G7Issue {
            source_tool: "lsp:rust-analyzer".to_owned(),
            file_path: "src/auth.rs".to_owned(),
            line_start: Some(50),
            line_end: Some(50),
            category: "style".to_owned(),
            severity: Severity::Info,
            message: "consider inlining".to_owned(),
            rule_id: Some("inline-hint".to_owned()),
            fix_suggestion: None,
        },
    ];

    // One G8 candidate is enough to satisfy the `check_quality`
    // dispatch precondition (`g7_sources.is_some() ||
    // g8_sources.is_some()`). The quality-side assertions below
    // do not depend on `untested_functions[]` — that is the F14
    // file's coverage axis.
    let candidates = vec![G8TestCandidate {
        test_path: std::path::PathBuf::from("tests/test_auth_pipeline.rs"),
        source_path: Some(std::path::PathBuf::from("src/auth.rs")),
        method: TestDiscoveryMethod::Convention,
        confidence: 0.9,
    }];

    let g7_src: Arc<dyn G7Source + Send + Sync> = Arc::new(SeededG7Source {
        source_id: "seeded-g7-source".to_owned(),
        issues,
    });
    let g8_src: Arc<dyn G8Source + Send + Sync> = Arc::new(SeededG8Source {
        source_id: "seeded-g8-source".to_owned(),
        method: TestDiscoveryMethod::Convention,
        candidates,
    });

    let server = McpServer::new()
        .with_g7_sources(Arc::new(vec![g7_src]))
        .with_g8_sources(Arc::new(vec![g8_src]));

    // Real in-memory duplex so we exercise `McpServer::serve`'s
    // newline-delimited JSON-RPC dispatcher — no `handle_line`
    // shortcut. 16 KiB matches the WO-0040 mcp-stdio precedent.
    let (client_end, server_end) = duplex(16 * 1024);
    let (server_read, server_write) = split(server_end);
    let (client_read, mut client_write) = split(client_end);

    let server_task = tokio::spawn(async move { server.serve(server_read, server_write).await });

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0xD13,
        "method": "tools/call",
        "params": {
            "name": "check_quality",
            "arguments": {
                "target": "src/auth.rs",
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

    // Drive `serve()` to clean EOF.
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

    let payload_text = response
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .expect("(precondition) response must carry result.content[0].text");
    let payload: Value = serde_json::from_str(payload_text)
        .expect("(precondition) result.content[0].text must be valid JSON");

    let issues_arr = payload
        .get("issues")
        .and_then(Value::as_array)
        .expect("(precondition) parsed payload must carry issues[] array");
    let meta = payload
        .get("meta")
        .expect("(precondition) parsed payload must carry meta object");

    // ── SA1 — issues[] length ≥ 3 (mutation-targeted) ───────────────
    assert!(
        issues_arr.len() >= 3,
        "(SA1) issues[] length ≥ 3; left: {}, right: 3",
        issues_arr.len()
    );

    // ── SA2 — every issues[i].severity in §5.7 vocabulary ──────────
    let allowed: [&str; 5] = ["critical", "high", "medium", "low", "info"];
    for (idx, issue) in issues_arr.iter().enumerate() {
        let severity = issue
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("<missing>");
        assert!(
            allowed.contains(&severity),
            "(SA2) issues[{idx}].severity in §5.7 vocabulary {{critical, high, medium, low, info}}; left: {severity:?}, right: one of {allowed:?}"
        );
    }

    // ── SA3 — meta.master_timed_out == false ───────────────────────
    assert_eq!(
        meta.get("master_timed_out").and_then(Value::as_bool),
        Some(false),
        "(SA3) meta.master_timed_out == false; left: {:?}, right: false",
        meta.get("master_timed_out")
    );
}
