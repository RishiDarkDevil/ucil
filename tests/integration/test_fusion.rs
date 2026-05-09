#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
// Each frozen test below carries an explicit Panics-section narrative
// in its rustdoc plus the standard `(SAn) ...` panic-message body
// convention from `DEC-0007`; suppressing the auto-emitted Panics-
// section requirement here matches the WO-0070/0085/0089/0090/0093
// frozen-test precedent.
#![allow(clippy::missing_panics_doc, clippy::too_long_first_doc_paragraph)]
//! `P3-W11-F16` — pure-function fusion engine integration test binary.
//!
//! Master-plan §6.2 lines 643-658 — Reciprocal Rank Fusion (`RRF`)
//! formula `Σ_r weight(r) / (k + rank_r)` with `k = 60`. Master-plan
//! §3.4 — cross-group fusion + group provenance. Master-plan §5.1
//! lines 430-442 — G1 fusion authority ladder.
//!
//! # Coverage axes
//!
//! Three module-root tests, each anchored on one fusion engine the
//! `search_code` / `find_similar` / `understand_code` MCP tools
//! consume downstream:
//!
//! 1. [`test_fusion_g2_rrf_correctness`] — `ucil_core::fuse_g2_rrf`
//!    with two G2 sources (`Probe` + `Ripgrep`) ranking the same
//!    location at rank 1: asserts the closed-form RRF score
//!    `2.0/(60+1) + 1.5/(60+1) ≈ 0.0574` and the descending-weight
//!    `contributing_sources` ordering.
//! 2. [`test_fusion_cross_group_dedup_and_provenance`] —
//!    `ucil_core::fuse_cross_group` with three groups (G1 + G3 + G4)
//!    converging on the same `(file, start_line, end_line)` location:
//!    asserts dedup-by-location + per-group provenance retention via
//!    `per_group_ranks`.
//! 3. [`test_fusion_g1_with_partial_source_coverage`] —
//!    `ucil_daemon::executor::fuse_g1` with two G1 sources where one
//!    returns an empty payload: asserts partial-coverage fusion stays
//!    correct (the source dispositions slot still records the
//!    `Available` source with `payload = []`).
//!
//! # Trait-seam carve-out (`DEC-0008` §4)
//!
//! These tests do NOT exercise the `G1Source` / `GroupExecutor`
//! orchestrator entry points — they call the pure-function fusion
//! layer directly with golden inputs. The trait dependency-inversion
//! seam is the property that lets `ucil-core` ship `fuse_g2_rrf` /
//! `fuse_cross_group` independent of any external transport, and
//! `ucil-daemon::executor::fuse_g1` independent of any per-source
//! Serena / tree-sitter / ast-grep impl. Production wiring of those
//! impls lives in `crates/ucil-daemon/`.
//!
//! # File layout (`DEC-0010`)
//!
//! The binary lives at `tests/integration/test_fusion.rs` per the
//! workspace convention and the `[[test]]` entry in
//! `tests/integration/Cargo.toml`. Master-plan §17.2 line 1693 lists
//! `test_fusion.rs` under `tests/integration/`.
//!
//! # Frozen-test placement (`DEC-0007`)
//!
//! Each `pub async fn test_*` lives at module ROOT (no nested `mod
//! tests { … }` wrapper) so `cargo test --test test_fusion
//! <fn_name>` substring-match selectors resolve directly without a
//! `tests::` path prefix.

use std::path::PathBuf;

use ucil_core::cross_group::{
    fuse_cross_group, CrossGroupExecution, Group, GroupHit, GroupResult, GroupStatus,
};
use ucil_core::fusion::{fuse_g2_rrf, G2Hit, G2Source, G2SourceResults, QueryType, G2_RRF_K};
use ucil_daemon::executor::{fuse_g1, G1Outcome, G1ToolKind, G1ToolOutput, G1ToolStatus};

// ── F16b SA1: G2 RRF correctness ───────────────────────────────────────────

/// Master-plan §6.2 line 645: `RRF` formula `Σ_r weight(r) / (k +
/// rank_r)` with `k = 60`. Master-plan §5.2 line 457 weight table:
/// `Probe = 2.0`, `Ripgrep = 1.5`.
///
/// Sub-assertions (`DEC-0007` SA-numbered panic messages):
///
/// * **SA1** — Same `(file_path, start_line, end_line)` location at
///   rank 1 in both `Probe` and `Ripgrep` produces a single fused
///   hit with `fused_score == 2.0/61 + 1.5/61` (`G2_RRF_K + 1`).
/// * **SA2** — `contributing_sources` ordered descending by
///   `rrf_weight` (`Probe` first because `2.0 > 1.5`).
/// * **SA3** — `per_source_ranks` carries provenance for every
///   contributing source.
#[tokio::test(flavor = "multi_thread")]
pub async fn test_fusion_g2_rrf_correctness() {
    let location = PathBuf::from("src/util.rs");

    let ripgrep_results = G2SourceResults {
        source: G2Source::Ripgrep,
        hits: vec![G2Hit {
            file_path: location.clone(),
            start_line: 10,
            end_line: 20,
            snippet: "fn util() {} // ripgrep".to_owned(),
            score: 0.8,
        }],
    };
    let probe_results = G2SourceResults {
        source: G2Source::Probe,
        hits: vec![G2Hit {
            file_path: location,
            start_line: 10,
            end_line: 20,
            snippet: "fn util() {} // probe".to_owned(),
            score: 0.95,
        }],
    };

    let outcome = fuse_g2_rrf(&[ripgrep_results, probe_results]);

    // ── SA1 — closed-form RRF score for rank-1-in-both ─────────────
    let expected_score = 2.0_f64 / f64::from(G2_RRF_K + 1) + 1.5_f64 / f64::from(G2_RRF_K + 1);
    assert_eq!(
        outcome.hits.len(),
        1,
        "(SA1) RRF outcome.hits.len() == 1 for same-location rank-1-in-both; left: {}, right: 1",
        outcome.hits.len()
    );
    assert!(
        (outcome.hits[0].fused_score - expected_score).abs() < 1e-9,
        "(SA1) RRF fused_score == 2.0/(k+1) + 1.5/(k+1); left: {}, right: {expected_score}",
        outcome.hits[0].fused_score
    );

    // ── SA2 — contributing_sources sorted by descending rrf_weight ─
    assert_eq!(
        outcome.hits[0].contributing_sources,
        vec![G2Source::Probe, G2Source::Ripgrep],
        "(SA2) contributing_sources sorted descending by rrf_weight (Probe=2.0 first); left: {:?}, right: [Probe, Ripgrep]",
        outcome.hits[0].contributing_sources
    );

    // ── SA3 — per_source_ranks carries provenance for every source ─
    assert!(
        outcome.hits[0]
            .per_source_ranks
            .contains(&(G2Source::Probe, 1)),
        "(SA3) per_source_ranks must contain (Probe, 1) provenance; left: {:?}, right: contains (Probe, 1)",
        outcome.hits[0].per_source_ranks
    );
    assert!(
        outcome.hits[0]
            .per_source_ranks
            .contains(&(G2Source::Ripgrep, 1)),
        "(SA3) per_source_ranks must contain (Ripgrep, 1) provenance; left: {:?}, right: contains (Ripgrep, 1)",
        outcome.hits[0].per_source_ranks
    );
}

// ── F16b SA2: cross-group dedup + provenance ───────────────────────────────

/// Master-plan §3.4 + §6.2 line 651: `find_references` query-type
/// weight row `[G1=3.0, G2=2.0, G3=0.5, G4=1.0, G5=0.5, G6=0.0,
/// G7=0.5, G8=0.0]`. Asserts dedup-by-location across G1 + G3 + G4
/// converging on the same `(file_path, start_line, end_line)` triple.
///
/// Sub-assertions:
///
/// * **SA1** — Three groups converging on the same location yield
///   exactly one fused hit (dedup retention).
/// * **SA2** — `per_group_ranks` retains every per-group rank
///   `(group, rank)` pair as provenance.
/// * **SA3** — Different-location hits stay distinct (no spurious
///   merge of unrelated locations).
#[tokio::test(flavor = "multi_thread")]
pub async fn test_fusion_cross_group_dedup_and_provenance() {
    let shared = PathBuf::from("src/auth.rs");
    let other = PathBuf::from("src/cache.rs");

    let g1 = GroupResult {
        group: Group::G1,
        status: GroupStatus::Available,
        hits: vec![GroupHit {
            file_path: shared.clone(),
            start_line: 30,
            end_line: 35,
            snippet: "fn login_g1".to_owned(),
            score: 0.91,
        }],
        elapsed_ms: 5,
        error: None,
    };
    let g3 = GroupResult {
        group: Group::G3,
        status: GroupStatus::Available,
        hits: vec![GroupHit {
            file_path: shared.clone(),
            start_line: 30,
            end_line: 35,
            snippet: "fn login_g3".to_owned(),
            score: 0.72,
        }],
        elapsed_ms: 5,
        error: None,
    };
    let g4 = GroupResult {
        group: Group::G4,
        status: GroupStatus::Available,
        hits: vec![
            GroupHit {
                file_path: other.clone(),
                start_line: 40,
                end_line: 45,
                snippet: "fn cache_g4".to_owned(),
                score: 0.83,
            },
            GroupHit {
                file_path: shared.clone(),
                start_line: 30,
                end_line: 35,
                snippet: "fn login_g4".to_owned(),
                score: 0.65,
            },
        ],
        elapsed_ms: 5,
        error: None,
    };

    let execution = CrossGroupExecution {
        results: vec![g1, g3, g4],
        master_timed_out: false,
        wall_elapsed_ms: 5,
        degraded_groups: vec![],
    };

    let outcome = fuse_cross_group(&execution, QueryType::FindReferences);

    // ── SA1 — dedup retains a single fused hit at the shared location ─
    let shared_hit = outcome
        .hits
        .iter()
        .find(|h| h.file_path == shared)
        .expect("(SA1 precondition) outcome must contain the shared-location hit");
    assert_eq!(
        shared_hit.contributing_groups.len(),
        3,
        "(SA1) cross-group dedup retains all 3 contributing groups at the shared location; left: {}, right: 3",
        shared_hit.contributing_groups.len()
    );

    // ── SA2 — per_group_ranks carries provenance for every group ─
    assert!(
        shared_hit.per_group_ranks.contains(&(Group::G1, 1)),
        "(SA2) per_group_ranks must contain (G1, 1) provenance; left: {:?}, right: contains (G1, 1)",
        shared_hit.per_group_ranks
    );
    assert!(
        shared_hit.per_group_ranks.contains(&(Group::G3, 1)),
        "(SA2) per_group_ranks must contain (G3, 1) provenance; left: {:?}, right: contains (G3, 1)",
        shared_hit.per_group_ranks
    );
    assert!(
        shared_hit.per_group_ranks.contains(&(Group::G4, 2)),
        "(SA2) per_group_ranks must contain (G4, 2) provenance — G4 ranked the shared location at rank 2; left: {:?}, right: contains (G4, 2)",
        shared_hit.per_group_ranks
    );

    // ── SA3 — distinct locations stay distinct (no spurious merge) ─
    let other_hit = outcome.hits.iter().find(|h| h.file_path == other);
    assert!(
        other_hit.is_some(),
        "(SA3) distinct-location hit at src/cache.rs must remain in outcome.hits (no spurious merge with shared); left: <absent>, right: <present>"
    );
    let other_hit = other_hit.expect("(SA3) other_hit guarded by is_some above");
    assert_eq!(
        other_hit.contributing_groups,
        vec![Group::G4],
        "(SA3) distinct-location hit retains only its single contributing group; left: {:?}, right: [G4]",
        other_hit.contributing_groups
    );
}

// ── F16b SA3: G1 fusion with partial source coverage ───────────────────────

/// Master-plan §5.1 lines 430-442 — G1 fusion authority ladder
/// `Serena > tree-sitter > ast-grep > diagnostics > scip`. Asserts
/// `fuse_g1` handles partial source coverage gracefully when one
/// source returns an empty `payload` and another returns a populated
/// payload.
///
/// Sub-assertions:
///
/// * **SA1** — `source_dispositions` carries every input source in
///   input order, regardless of whether the source produced fused
///   entries.
/// * **SA2** — `master_timed_out` is forwarded verbatim from the
///   input `G1Outcome`.
/// * **SA3** — Partial-coverage fusion never panics; an empty
///   payload contributes zero entries to the fused output.
#[tokio::test(flavor = "multi_thread")]
pub async fn test_fusion_g1_with_partial_source_coverage() {
    // TreeSitter contributes one entry; Serena returns an empty
    // payload (the source was Available but found no entries).
    let payload_with_entry = serde_json::json!([
        {
            "location": {
                "file_path": "src/lib.rs",
                "start_line": 50,
                "end_line": 55,
            },
            "fields": {
                "kind": "function",
                "qualified_name": "ucil_core::lib::run",
            }
        }
    ]);
    let empty_payload = serde_json::json!([]);

    let outcome = G1Outcome {
        results: vec![
            G1ToolOutput {
                kind: G1ToolKind::TreeSitter,
                status: G1ToolStatus::Available,
                elapsed_ms: 3,
                payload: payload_with_entry,
                error: None,
            },
            G1ToolOutput {
                kind: G1ToolKind::Serena,
                status: G1ToolStatus::Available,
                elapsed_ms: 4,
                payload: empty_payload,
                error: None,
            },
        ],
        wall_elapsed_ms: 4,
        master_timed_out: false,
    };

    let fused = fuse_g1(&outcome);

    // ── SA1 — source_dispositions in input order ─────────────────────
    assert_eq!(
        fused.source_dispositions.len(),
        2,
        "(SA1) source_dispositions covers every input source; left: {}, right: 2",
        fused.source_dispositions.len()
    );
    assert_eq!(
        fused.source_dispositions[0].0,
        G1ToolKind::TreeSitter,
        "(SA1) source_dispositions[0] preserves input order — TreeSitter first; left: {:?}, right: TreeSitter",
        fused.source_dispositions[0].0
    );
    assert_eq!(
        fused.source_dispositions[1].0,
        G1ToolKind::Serena,
        "(SA1) source_dispositions[1] preserves input order — Serena second; left: {:?}, right: Serena",
        fused.source_dispositions[1].0
    );

    // ── SA2 — master_timed_out forwarded verbatim ─────────────────────
    assert!(
        !fused.master_timed_out,
        "(SA2) master_timed_out forwarded verbatim from G1Outcome; left: {}, right: false",
        fused.master_timed_out
    );

    // ── SA3 — partial coverage produces exactly the populated entries ─
    assert_eq!(
        fused.entries.len(),
        1,
        "(SA3) partial-coverage fusion preserves only populated payload entries — empty payload contributes zero; left: {}, right: 1",
        fused.entries.len()
    );
    let entry = &fused.entries[0];
    assert!(
        entry
            .contributing_sources
            .contains(&G1ToolKind::TreeSitter),
        "(SA3) populated entry retains TreeSitter as contributing source; left: {:?}, right: contains TreeSitter",
        entry.contributing_sources
    );
}
