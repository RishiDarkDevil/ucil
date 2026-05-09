#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
// Doc-comment first-paragraph length is governed by the SA-numbered
// rustdoc convention (`DEC-0007`). The frozen test fn carries an
// explicit `# Panics` section so the global allow is only needed for
// the helper rustdoc.
#![allow(clippy::too_long_first_doc_paragraph)]
//! `P3-W11-F14` — testing-pipeline MCP integration test binary.
//!
//! Master-plan §3.2 row 15 (`run_tests` MCP tool — discover + run
//! relevant tests). Master-plan §5.8 lines 561-579 — G8 (Testing)
//! parallel fan-out under a 5 s master deadline; "Discover ALL
//! relevant tests via ALL methods: 1. Convention-based / 2. Import-
//! based / 3. `KG`-based — concurrently, then merge". §6.1 lines 605-
//! 608 — per-group timeout + `degraded_groups` surface.
//!
//! # Coverage axis
//!
//! One module-root `#[tokio::test]` drives `check_quality` end-to-
//! end through `McpServer::serve` over a `tokio::io::duplex` pair.
//! `check_quality` is the dispatch path that fans out both G7 (the
//! quality side, dormant in this test) AND G8 (the testing side,
//! load-bearing) per server.rs line 963-965, projecting the merged
//! G8 candidates onto `result.content[0].text.untested_functions[]`.
//!
//! `run_tests` itself currently falls through to the phase-1
//! `_meta.not_yet_implemented: true` stub path — its dedicated MCP
//! handler is a follow-up production-wiring WO. Until that lands,
//! `check_quality`'s `untested_functions[]` projection is the
//! canonical surface for asserting G8 dispatch.
//!
//! Sub-assertions (`DEC-0007` SA-numbered):
//!
//! * **SA1** — `untested_functions[]` length ≥ 3 (3 sources × 2
//!   candidates each = 6 distinct test paths after the dedup-by-
//!   test-path merge).
//! * **SA2** — At least one `untested_functions[i].test_path`
//!   matches a seeded candidate's path (provenance retention).
//! * **SA3** — Across every `untested_functions[i].methods_found_by`
//!   array, the union covers all 3 `TestDiscoveryMethod` variants
//!   (`convention`, `import`, `kg_relations`) — the master-plan §5.8
//!   "ALL methods" mandate. M2 (mutation-targeted) flips one
//!   source's `method` field so the union shrinks to 2 distinct
//!   methods, tripping this SA.
//!
//! # Trait-seam carve-out (`DEC-0008` §4)
//!
//! The test-side `SeededG8Source` impl below is a UCIL-owned
//! `G8Source` trait realisation — the trait IS the dependency-
//! inversion seam. Production impls (`ConventionG8Source`,
//! `ImportG8Source`, `KgRelationsG8Source`) live in
//! `crates/ucil-daemon/` and arrive in follow-up production-wiring
//! WOs that bundle the daemon-startup orchestration; this test
//! file ships only the integration-test surface.
//!
//! # File layout (`DEC-0010`) and frozen-test placement (`DEC-0007`)
//!
//! Binary lives at `tests/integration/test_testing_pipeline.rs`
//! (master-plan §17.2 line 1693). The single `pub async fn` lives at
//! module ROOT (no nested `mod tests { … }` wrapper) so the
//! substring-match selector
//! `cargo test --test test_testing_pipeline test_testing_pipeline_discovers_tests_in_fixture_projects`
//! resolves directly without a `tests::` path prefix.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{duplex, split, AsyncBufReadExt, AsyncWriteExt, BufReader};

use ucil_daemon::g8::{G8Query, G8Source, G8TestCandidate, TestDiscoveryMethod};
use ucil_daemon::server::McpServer;

// ── Test-side `G8Source` impl — `DEC-0008` §4 dep-inversion seam ──────────

/// UCIL-owned `G8Source` realisation seeded with deterministic
/// `G8TestCandidate` rows. Each candidate's `method` field is
/// stamped from `self.method` at `execute()`-time so the source's
/// declared method propagates to every candidate the merge consumes
/// — keeps the M2 mutation contract authoritative (changing
/// `self.method` flips `methods_found_by` for every candidate the
/// source emits, shrinking the union by exactly one).
///
/// Production `G8Source` impls (`ConventionG8Source`,
/// `ImportG8Source`, `KgRelationsG8Source`) live in
/// `crates/ucil-daemon/`; this trait-seam realisation is the test-
/// side counterpart.
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
        // Stamp `self.method` onto every emitted candidate so the
        // M2 mutation contract is authoritative (changing
        // `self.method` propagates through the merge layer's
        // `candidate.method` read).
        let stamped: Vec<G8TestCandidate> = self
            .candidates
            .iter()
            .map(|c| G8TestCandidate {
                test_path: c.test_path.clone(),
                source_path: c.source_path.clone(),
                method: self.method,
                confidence: c.confidence,
            })
            .collect();
        Ok(stamped)
    }
}

// ── F14 frozen test ──────────────────────────────────────────────────────

/// Drives `check_quality` end-to-end through `McpServer::serve` to
/// exercise the G8 (Testing) discovery dispatch.
///
/// Constructs three `SeededG8Source` instances, one per
/// `TestDiscoveryMethod` variant (`Convention` / `Import` /
/// `KgRelations`), each emitting 2 distinct test-path candidates.
/// Total: 6 candidates flowing into `merge_g8_test_discoveries`,
/// producing 6 `MergedG8TestCandidate` rows (every test path is
/// unique, so no group collapse).
///
/// The G7 side is intentionally absent — `check_quality`'s
/// dispatch precondition only requires `g7_sources.is_some() ||
/// g8_sources.is_some()` (server.rs line 963). With G7 absent the
/// `issues[]` projection is empty; this test focuses on the
/// `untested_functions[]` projection for the testing-side
/// assertions.
///
/// The test wraps the entire JSON-RPC exchange in
/// `tokio::time::timeout(Duration::from_secs(30), ...)` as a
/// belt-and-braces hang guard; the daemon's per-source / per-master
/// deadlines already cap inner work at well under 6 s.
///
/// # Panics
///
/// Panics with an `(SAn) ...` body on any sub-assertion failure or
/// a `(precondition) ...` body on JSON-RPC transport failures —
/// `DEC-0007` panic-message convention from
/// WO-0067/0083/0085/0089/0090/0093 frozen-test precedent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::missing_panics_doc)] // panic semantics described in the rustdoc # Panics section above.
pub async fn test_testing_pipeline_discovers_tests_in_fixture_projects() {
    tokio::time::timeout(Duration::from_secs(30), run_testing_pipeline_assertions())
        .await
        .expect("(precondition) testing-pipeline test must finish within 30 s wall-clock");
}

#[allow(clippy::too_many_lines)]
async fn run_testing_pipeline_assertions() {
    // Three sources × 2 candidates each = 6 candidates with
    // distinct test paths. Each source's `method` propagates to
    // every candidate it emits via `SeededG8Source::execute`.
    let convention_source = SeededG8Source {
        source_id: "seeded-g8-convention".to_owned(),
        method: TestDiscoveryMethod::Convention,
        candidates: vec![
            G8TestCandidate {
                test_path: PathBuf::from("tests/test_login_convention.rs"),
                source_path: Some(PathBuf::from("src/auth.rs")),
                method: TestDiscoveryMethod::Convention,
                confidence: 0.95,
            },
            G8TestCandidate {
                test_path: PathBuf::from("tests/test_logout_convention.rs"),
                source_path: Some(PathBuf::from("src/auth.rs")),
                method: TestDiscoveryMethod::Convention,
                confidence: 0.92,
            },
        ],
    };
    let import_source = SeededG8Source {
        source_id: "seeded-g8-import".to_owned(),
        method: TestDiscoveryMethod::Import,
        candidates: vec![
            G8TestCandidate {
                test_path: PathBuf::from("tests/test_session_import.rs"),
                source_path: Some(PathBuf::from("src/session.rs")),
                method: TestDiscoveryMethod::Import,
                confidence: 0.88,
            },
            G8TestCandidate {
                test_path: PathBuf::from("tests/test_token_import.rs"),
                source_path: Some(PathBuf::from("src/session.rs")),
                method: TestDiscoveryMethod::Import,
                confidence: 0.81,
            },
        ],
    };
    let kg_source = SeededG8Source {
        source_id: "seeded-g8-kg-relations".to_owned(),
        method: TestDiscoveryMethod::KgRelations,
        candidates: vec![
            G8TestCandidate {
                test_path: PathBuf::from("tests/test_cache_kg.rs"),
                source_path: Some(PathBuf::from("src/cache.rs")),
                method: TestDiscoveryMethod::KgRelations,
                confidence: 0.79,
            },
            G8TestCandidate {
                test_path: PathBuf::from("tests/test_index_kg.rs"),
                source_path: Some(PathBuf::from("src/index.rs")),
                method: TestDiscoveryMethod::KgRelations,
                confidence: 0.74,
            },
        ],
    };

    let g8_sources: Arc<Vec<Arc<dyn G8Source + Send + Sync>>> = Arc::new(vec![
        Arc::new(convention_source) as Arc<dyn G8Source + Send + Sync>,
        Arc::new(import_source) as Arc<dyn G8Source + Send + Sync>,
        Arc::new(kg_source) as Arc<dyn G8Source + Send + Sync>,
    ]);

    let server = McpServer::new().with_g8_sources(g8_sources);

    let (client_end, server_end) = duplex(16 * 1024);
    let (server_read, server_write) = split(server_end);
    let (client_read, mut client_write) = split(client_end);

    let server_task = tokio::spawn(async move { server.serve(server_read, server_write).await });

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0xD14,
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

    let untested_arr = payload
        .get("untested_functions")
        .and_then(Value::as_array)
        .expect("(precondition) parsed payload must carry untested_functions[] array");

    // ── SA1 — untested_functions[] length ≥ 3 ───────────────────────
    assert!(
        untested_arr.len() >= 3,
        "(SA1) untested_functions[] length ≥ 3; left: {}, right: 3",
        untested_arr.len()
    );

    // ── SA2 — at least one test_path matches a seeded candidate ────
    let seeded_paths: [&str; 6] = [
        "tests/test_login_convention.rs",
        "tests/test_logout_convention.rs",
        "tests/test_session_import.rs",
        "tests/test_token_import.rs",
        "tests/test_cache_kg.rs",
        "tests/test_index_kg.rs",
    ];
    let any_match = untested_arr.iter().any(|row| {
        let path = row
            .get("test_path")
            .and_then(Value::as_str)
            .unwrap_or("<missing>");
        seeded_paths.iter().any(|seed| path.contains(seed))
    });
    assert!(
        any_match,
        "(SA2) untested_functions[] contains a seeded test_path; left: <no match>, right: any of {seeded_paths:?}"
    );

    // ── SA3 — methods_found_by union covers all 3 methods ──────────
    let mut method_union: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for row in untested_arr {
        let methods = row
            .get("methods_found_by")
            .and_then(Value::as_array)
            .expect("(SA3 precondition) every untested_functions[i] must carry methods_found_by[]");
        for m in methods {
            if let Some(s) = m.as_str() {
                method_union.insert(s.to_owned());
            }
        }
    }
    let union_vec: Vec<String> = method_union.iter().cloned().collect();
    assert_eq!(
        method_union.len(),
        3,
        "(SA3) methods_found_by union covers all 3 §5.8 discovery methods (convention, import, kg_relations); left: {union_vec:?}, right: 3 distinct methods"
    );
    assert!(
        method_union.contains("convention"),
        "(SA3) methods_found_by union contains \"convention\"; left: {union_vec:?}, right: contains \"convention\""
    );
    assert!(
        method_union.contains("import"),
        "(SA3) methods_found_by union contains \"import\"; left: {union_vec:?}, right: contains \"import\""
    );
    assert!(
        method_union.contains("kg_relations"),
        "(SA3) methods_found_by union contains \"kg_relations\"; left: {union_vec:?}, right: contains \"kg_relations\""
    );
}
