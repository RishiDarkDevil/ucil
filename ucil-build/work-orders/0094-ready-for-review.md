# WO-0094 — Ready for Review

**Final commit sha**: `f488a6e6cf53bcf0fee9885eb2333e127fa3b694`
**Branch**: `feat/WO-0094-w11-pipeline-integration-tests`
**Features**: P3-W11-F13 (quality pipeline) + P3-W11-F14 (testing pipeline) + P3-W11-F16 (query pipeline + fusion)

## Summary

Lands four NEW Phase-3 Week-11 MCP-pipeline integration test binaries
under `tests/integration/`, plus three verify shell scripts and four
new `[[test]]` entries in `tests/integration/Cargo.toml`:

| File | Feature | Frozen test fn |
|------|---------|----------------|
| `tests/integration/test_quality_pipeline.rs` | P3-W11-F13 | `test_quality_pipeline_detects_severity_classified_issues` |
| `tests/integration/test_testing_pipeline.rs` | P3-W11-F14 | `test_testing_pipeline_discovers_tests_in_fixture_projects` |
| `tests/integration/test_query_pipeline.rs` | P3-W11-F16a | `test_query_pipeline_returns_fused_results_with_group_provenance` |
| `tests/integration/test_fusion.rs` | P3-W11-F16b | `test_fusion_g2_rrf_correctness`, `test_fusion_cross_group_dedup_and_provenance`, `test_fusion_g1_with_partial_source_coverage` |

Each integration test file declares `#![deny(warnings)]` +
`#![warn(clippy::all, clippy::pedantic, clippy::nursery)]` at the
file top (within `head -3`) per AC10. Frozen test fns live at module
ROOT (no nested `mod tests { ... }` wrapper) per DEC-0007 + AC11. The
`SeededG{4,7,8}Source` impls in each file are UCIL-owned trait
realisations per DEC-0008 §4 — the trait IS the dependency-inversion
seam; production impls live in `crates/ucil-daemon/`.

## Files changed

* `tests/integration/Cargo.toml` — appended 4 new `[[test]]` entries
  preserving alphabetical ordering. The full ordering is:
  `test_fusion`, `test_lsp_bridge`, `test_plugin_lifecycle`,
  `test_quality_pipeline`, `test_query_pipeline`,
  `test_testing_pipeline`.
* `tests/integration/test_fusion.rs` — NEW (370 LOC). Pure-function
  fusion engine tests covering `ucil_core::fuse_g2_rrf` +
  `ucil_core::fuse_cross_group` + `ucil_daemon::fuse_g1`.
* `tests/integration/test_quality_pipeline.rs` — NEW (~360 LOC).
  Drives `check_quality` end-to-end through `McpServer::serve` over
  a `tokio::io::duplex` pair with seeded G7+G8 sources.
* `tests/integration/test_testing_pipeline.rs` — NEW (~360 LOC).
  Drives `check_quality` to exercise the G8 (Testing) discovery
  dispatch (since `run_tests` itself currently falls through to the
  phase-1 `_meta.not_yet_implemented: true` path; its dedicated MCP
  handler is a follow-up production-wiring WO).
* `tests/integration/test_query_pipeline.rs` — NEW (~378 LOC).
  Drives `get_architecture` through the full
  `with_g4_sources` + `with_g7_sources` + `with_g8_sources` builder
  chain to assert the cross-group fan-out surface.
* `scripts/verify/P3-W11-F13.sh` — NEW, +x. Runs
  `cargo test --test test_quality_pipeline` and asserts
  `test result: ok.`.
* `scripts/verify/P3-W11-F14.sh` — NEW, +x. Runs
  `cargo test --test test_testing_pipeline` and asserts
  `test result: ok.`.
* `scripts/verify/P3-W11-F16.sh` — NEW, +x. Runs the verbatim
  feature-list selector
  `cargo test --test test_query_pipeline --test test_fusion` and
  requires BOTH binaries to print `test result: ok.`.

## What I verified locally

* AC1 — All four NEW test files exist at the canonical DEC-0010
  paths.
* AC2 — All four `[[test]]` entries present in
  `tests/integration/Cargo.toml`.
* AC3 / AC4 / AC5 / AC6 — `cargo test --test test_quality_pipeline`,
  `--test test_testing_pipeline`, `--test test_query_pipeline`,
  `--test test_fusion` all exit 0 with at least one passing test.
* AC7 — `cargo test --test test_query_pipeline --test test_fusion`
  (verbatim F16 selector) exits 0; both binaries print
  `test result: ok.`.
* AC8 — `cargo clippy -p ucil-tests-integration --tests --
  -D warnings` exits 0.
* AC9 — `cargo build -p ucil-tests-integration --tests` exits 0
  without warnings.
* AC10 — `head -3` of each new file includes
  `#![deny(warnings)]`.
* AC11 — None of the new files contain a nested
  `mod tests { ... }` wrapper.
* AC12 — Each new file has `pub async fn test_*` at module ROOT.
* AC13 — Banned-word scrub
  `! grep -qiE 'mock|fake|stub' <files>` returns empty across all
  four new test files AND all three new verify scripts.
* AC14 — No `unsafe { ... }` / `unsafe fn` in any new test file.
* AC15 — No `std::process::Command` in any new test file.
* AC16 / AC17 / AC18 / AC19 — All three new verify scripts are
  `chmod +x`, carry shebang `#!/usr/bin/env bash`, declare
  `set -euo pipefail`, and invoke their respective
  `cargo test --test ...` selectors.
* AC20 — `bash scripts/verify/P3-W11-F13.sh / F14.sh / F16.sh` each
  exits 0.
* AC21 — `grep -qE '\(SA[0-9]+\)' tests/integration/test_*.rs`
  returns true for every new file (every assertion wears the
  `(SAn) ...` panic-message body convention).
* AC25 — `cargo test -p ucil-tests-integration` exits 0 with all 6
  test binaries (3 existing + 3 new — `test_fusion`,
  `test_lsp_bridge`, `test_plugin_lifecycle`,
  `test_quality_pipeline`, `test_query_pipeline`,
  `test_testing_pipeline`) green.
* AC26 — `cargo test --no-run -p ucil-tests-integration` completes
  well under 60 s after a warm `target/` (incremental link is the
  dominant cost; the four new binaries each link against ucil-daemon
  in parallel).

## Mutation contract

Per scope_in #13 + #28 + AC30/31/32 the mutation contract targets
THREE DIFFERENT files (one per feature). The verifier MUST:

1. Snapshot `md5sum tests/integration/test_quality_pipeline.rs > /tmp/wo-0094-test_quality_pipeline-orig.md5sum`,
   `md5sum tests/integration/test_testing_pipeline.rs > /tmp/wo-0094-test_testing_pipeline-orig.md5sum`,
   `md5sum tests/integration/test_query_pipeline.rs > /tmp/wo-0094-test_query_pipeline-orig.md5sum` BEFORE applying any mutation.
2. Apply each mutation via `Edit` (in-place; no `git stash`).
3. Run the targeted `bash scripts/verify/P3-W11-F<NN>.sh` and
   confirm the targeted SA fires with the verbatim panic-body shown
   below.
4. Restore via `git checkout -- <file>` and confirm md5 matches the
   pre-mutation snapshot.

### M1 (F13 — `test_quality_pipeline.rs` SA1 trip)

* **Target file**: `tests/integration/test_quality_pipeline.rs`
* **Patch**: shrink the seeded G7 issue list from 5 issues to 1.
  Replace the `let issues = vec![ ... five entries ... ];` block in
  `run_quality_pipeline_assertions` with `let issues = vec![ ... only the first
  Critical-severity entry ... ];`. (Concretely: keep the first
  `G7Issue { ... severity: Severity::Critical, ... }` entry; delete
  the four entries below it.)
* **Expected**: `bash scripts/verify/P3-W11-F13.sh` exits non-zero;
  the test panic body reads
  `(SA1) issues[] length ≥ 3; left: 1, right: 3`.
* **Restore**: `git checkout -- tests/integration/test_quality_pipeline.rs`
  and re-run `md5sum` against `/tmp/wo-0094-test_quality_pipeline-orig.md5sum`
  to confirm.

### M2 (F14 — `test_testing_pipeline.rs` SA3 trip)

* **Target file**: `tests/integration/test_testing_pipeline.rs`
* **Patch**: in `run_testing_pipeline_assertions`, change the
  `convention_source.method` value from
  `TestDiscoveryMethod::Convention` to `TestDiscoveryMethod::Import`.
  (`SeededG8Source::execute` stamps `self.method` onto every emitted
  candidate, so this propagates through the merge layer's
  `candidate.method` read — see scope_in #13 + the `SeededG8Source`
  rustdoc.)
* **Expected**: `bash scripts/verify/P3-W11-F14.sh` exits non-zero;
  the test panic body reads
  `(SA3) methods_found_by union covers all 3 §5.8 discovery methods (convention, import, kg_relations); left: ["import", "kg_relations"], right: 3 distinct methods`
  (the `convention` method drops out of the union because the
  former Convention source now stamps `Import`).
* **Restore**: `git checkout -- tests/integration/test_testing_pipeline.rs`
  and re-run `md5sum` against `/tmp/wo-0094-test_testing_pipeline-orig.md5sum`
  to confirm.

### M3 (F16a — `test_query_pipeline.rs` SA1 trip)

* **Target file**: `tests/integration/test_query_pipeline.rs`
* **Patch**: in `run_query_pipeline_assertions`, drop the
  `.with_g4_sources(Arc::new(vec![g4_source]))` builder call from
  the `let server = McpServer::new() ... ;` chain. (Keep the
  `with_g7_sources` + `with_g8_sources` calls — they are not
  load-bearing for SA1 but their presence proves the multi-builder
  chain still compiles.)
* **Expected**: `bash scripts/verify/P3-W11-F16.sh` exits non-zero;
  the test panic body reads
  `(SA1) _meta.modules present (only emitted when `with_g4_sources(...)` wired into McpServer); left: <absent>, right: <present array>`
  (the dispatcher falls through to the phase-1
  `_meta.not_yet_implemented: true` path which emits `_meta.tool`
  but no `_meta.modules`).
* **Restore**: `git checkout -- tests/integration/test_query_pipeline.rs`
  and re-run `md5sum` against `/tmp/wo-0094-test_query_pipeline-orig.md5sum`
  to confirm.

## Production-side `.unwrap()` / `.expect()` enumeration

Per WO-0085 / WO-0090 / WO-0093 lessons (§executor — enumerate every
`.unwrap()` / `.expect()` honestly, even when zero), the four new
test files use:

* `tests/integration/test_fusion.rs` — every `.expect(...)` carries
  an `(SAn precondition) ...` body and never appears outside a test
  fn body. No `.unwrap()`.
* `tests/integration/test_quality_pipeline.rs` — every `.expect(...)`
  carries either `(SAn precondition) ...`, `(precondition) ...`, or
  `(SA4 precondition) ...` body. The `unwrap_or("<missing>")` calls
  in the SA assertions are typed-narrowing fallbacks for `serde_json`
  optional reads. No `.unwrap()` (without a fallback).
* `tests/integration/test_testing_pipeline.rs` — same pattern. The
  `unwrap_or("<missing>")` fallback is used inside the SA2
  any-match scan to avoid an extra `expect` step. No bare
  `.unwrap()`.
* `tests/integration/test_query_pipeline.rs` — same pattern. No bare
  `.unwrap()`.

These are integration-test files, not production source code; no
new production-side `.unwrap()` / `.expect()` are introduced by this
WO.

## Standing carry-forward

* The `effectiveness-gate.sh` claude-p sub-session timeout flake
  carry-forward (`20260507T0357Z` / `20260507T1629Z` /
  `20260507T1930Z` standing escalations) applies to this WO per
  scope_out #13. Verifier may skip the effectiveness-gate.sh step
  if the sub-session times out; the substantive AC is the
  `cargo test --test test_*` invocations which are deterministic.
* The `coverage-gate.sh` `RUSTC_WRAPPER` workaround
  (`env -u RUSTC_WRAPPER cargo llvm-cov ...`) carries forward per
  scope_out #14 — substantive AC23 measure path is the binding
  floor.

## Commits on this branch (8)

```
f488a6e test(integration): rephrase rustdoc fall-through line to clear AC13 word-ban (P3-W11-F14)
d942deb build(scripts): add three verify shell scripts for P3-W11-F13/F14/F16
31170db test(integration): add test_query_pipeline.rs for full-pipeline cross-group fusion (P3-W11-F16)
89326ac test(integration): add test_testing_pipeline.rs for run_tests discovery (P3-W11-F14)
f83bc12 test(integration): add test_quality_pipeline.rs for check_quality severity (P3-W11-F13)
8e22582 test(integration): satisfy strict clippy on test_fusion (P3-W11-F16)
f6a3fe8 test(integration): add test_fusion.rs covering RRF + cross-group fusion (P3-W11-F16)
ffc2488 build(tests-integration): add four [[test]] entries for W11 pipeline integration tests
```

The 8th commit (`f488a6e`) is the rephrase commit that landed an
AC13-required word-ban fix discovered after the initial F14 commit;
it adjusts only the module-header rustdoc and does not touch any
test logic. The 7-commit estimate from the work-order remains the
nominal target — the +1 is a small cleanup commit.

No merge commits introduced on the feat branch
(`git log feat/WO-0094-w11-pipeline-integration-tests ^main --merges | wc -l == 0`).
Every commit ends with `Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>`
and references `Phase: 3` + at least one `Feature: P3-W11-F<NN>`
trailer + `Work-order: WO-0094`.
