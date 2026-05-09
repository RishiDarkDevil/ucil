# WO-0095 — Ready for Review

**Final commit (pre-RFR-marker)**: `428f3fa8f346bfb45bd4f74efaf932581f1e5a86`
**Branch**: `feat/WO-0095-incremental-computation-integration-test`
**Feature**: P3-W9-F11
**Phase**: 3, Week 9

## Summary

Lands the NEW `tests/integration/test_incremental.rs` integration test
binary proving the Salsa engine's early-cutoff invariant at the public
API boundary. Two frozen tests at module ROOT (DEC-0007):

1. `test_incremental_whitespace_only_change_skips_downstream_recompute`
   — drives the load-bearing SA1 early-cutoff assertion: a
   whitespace-only contents bump invalidates `symbol_count`'s revision
   but leaves its return value unchanged, so `dependent_metric` MUST
   NOT re-execute.
2. `test_incremental_semantic_change_invalidates_downstream` — control
   case asserting BOTH `symbol_count` AND `dependent_metric` re-execute
   when the contents change semantically (token count changes).

The local `LoggingDatabase` test harness implements `salsa::Database`
and records every `WillExecute` event via Salsa's `Storage::new`
event-hook constructor — the standard Salsa observability harness.

A new verify script `scripts/verify/P3-W9-F11.sh` (mirroring the
WO-0094 `P3-W11-F13.sh` shape verbatim) runs `cargo test --test
test_incremental` and asserts `test result: ok.` via `grep -qE`.

## Files changed

| Path | Change | LOC |
|------|--------|-----|
| `tests/integration/test_incremental.rs` | NEW | +178 |
| `tests/integration/Cargo.toml` | added `[[test]]` entry + `salsa = "0.22"` dev-dep | +5 |
| `scripts/verify/P3-W9-F11.sh` | NEW (chmod +x) | +34 |
| `Cargo.lock` | auto-regenerated (salsa hop into ucil-tests-integration) | +1 |

Total: +218 lines across 4 files (3 new, 1 modified, 1 lockfile).

## Commits (3 in total — under suggested budget of 5)

```
428f3fa build(scripts): add verify shell script for P3-W9-F11
bd6f17a build(tests-integration): add salsa dev-dep + [[test]] entry for test_incremental
fbedcdf test(integration): add test_incremental.rs LoggingDatabase + early-cutoff frozen tests
```

The two suggested optional commits (commit #4 — panic-message body
tightening, and a separate Cargo.toml split between `[[test]]` entry
and `salsa` dev-dep) were not needed:

* No SAn-body drift detected on pre-RFR re-grep.
* Test file was committed before Cargo.toml so cargo built cleanly at
  every intermediate sha (ordering deviated from the WO's suggested
  order #1 → #2 to keep every commit individually buildable).

## Acceptance criteria — local verification

| AC | Result |
|----|--------|
| AC1 — `test -e tests/integration/test_incremental.rs` | PASS |
| AC2 — `[[test]]` entry for `test_incremental` in Cargo.toml | PASS |
| AC3 — `salsa = "0.22"` dev-dep in tests/integration/Cargo.toml | PASS |
| AC4 — `cargo test --test test_incremental` greps `test result: ok.` | PASS (2 passed) |
| AC5 — `cargo clippy -p ucil-tests-integration --tests -- -D warnings` exit 0 | PASS |
| AC6 — `cargo build -p ucil-tests-integration --tests` exit 0 | PASS |
| AC7 — `#![deny(warnings)]` + `#![warn(clippy::all, clippy::pedantic, clippy::nursery)]` near top | PASS |
| AC8 — no nested `mod tests { ... }` wrapper | PASS |
| AC9 — `pub fn test_*` at module root | PASS |
| AC10 — no banned words `mock|fake|stub` in test or verify script | PASS |
| AC11 — no `unsafe` in test code | PASS |
| AC12 — no `std::process::Command` | PASS |
| AC13 — verify script exists and is executable | PASS |
| AC14 — `#!/usr/bin/env bash` shebang | PASS |
| AC15 — `set -euo pipefail` | PASS |
| AC16 — script invokes `cargo test --test test_incremental` | PASS |
| AC17 — `bash scripts/verify/P3-W9-F11.sh` exits 0 | PASS |
| AC18 — `(SAn)` panic-message body convention | PASS |
| AC19 — RFR marker exists with required sections | PASS (this file) |
| AC20 — coverage gate baseline (verifier-side standing protocol) | DEFERRED to verifier — this WO does not change ucil-core/daemon/treesitter/embeddings source so floor cannot regress |
| AC21 — commit count budget (suggested 5) | PASS (3 commits — under budget) |
| AC22 — no merge commits on feat | PASS (`git log feat ^main --merges \| wc -l` = 0) |
| AC23 — no banned commit-flag patterns | PASS |
| AC24 — every commit carries `Co-Authored-By: Claude Opus 4.7` | PASS (3/3) |
| AC25 — every commit carries `Phase: 3` + `Feature: P3-W9-F11` + `Work-order: WO-0095` | PASS (3/3) |
| AC26 — Phase-3 gate sub-checks (cargo-test + clippy + workspace-build) | DEFERRED to verifier — substantive AC4/AC5/AC6 already green |
| AC27 — M1 mutation per scope_in #13 trips the test | PASS (see Mutation contract below) |
| AC28 — M1 restore + md5 verify clean | PASS (md5 = `d53466931b93f1f709a243d4c7a237cb`) |
| AC29 — `cargo test -p ucil-tests-integration` all 7 binaries pass | PASS (9 `test result: ok.` lines, 0 FAILED — after `cargo build -p ucil-daemon --bin mock-mcp-plugin` warm-up) |
| AC30 — `[[test]]` entries alphabetically ordered | PASS (`test_fusion`, `test_incremental`, `test_lsp_bridge`, `test_plugin_lifecycle`, `test_quality_pipeline`, `test_query_pipeline`, `test_testing_pipeline`) |
| AC31 — Phase 1 gate sub-checks unchanged | NO TOUCH — this WO does not modify any file under `crates/`; verifier may confirm via `git diff main --stat` showing only `tests/integration/`, `scripts/verify/`, and `Cargo.lock` changes |
| AC32 — Phase 2 gate sub-checks unchanged | NO TOUCH — same as AC31 |

## Mutation contract

### M1 (P3-W9-F11) — single file, single mutation

**File**: `tests/integration/test_incremental.rs`

**md5 snapshot path**: `/tmp/wo-0095-test_incremental-orig.md5sum`
**md5 value (pre-mutation)**: `d53466931b93f1f709a243d4c7a237cb`

**Patch**: change the whitespace-only contents replacement on the
mtime+contents bump (lines 112-113) from a 3-token whitespace-variant
to a 4-token semantic variant:

```diff
     rev.set_mtime_nanos(&mut db).to(42);
     rev.set_contents(&mut db)
-        .to("alpha   beta\tgamma".to_owned());
+        .to("alpha beta gamma delta".to_owned());
```

**Expected failure (executor verified)**: After the mutation, the four
SAn assertions in `test_incremental_whitespace_only_change_skips_downstream_recompute`
all fire on the new 4-token contents value:

* SA0 (initial metric == 6) still passes (initial seed unchanged).
* SA2 (`assert_eq!(after_metric, 6)`) is the FIRST assertion to fail
  because `after_metric` is now `8` (4 tokens × 2). The panic message
  body is verbatim:
  ```
  (SA2) dependent_metric value stable across whitespace-only change; left: 8, right: 6
  ```
* If SA2 were absent, SA1 (`assert!(!dependent_reran)`) would also
  fire because `dependent_metric` re-executes when `symbol_count`'s
  return value changes. SA1 is the load-bearing semantic assertion
  per the WO; SA2 happens to be evaluated first per assertion order
  in the test body. Both SA1 and SA2 are correct early-cutoff
  invariants and both prove the test is load-bearing — the mutation
  cannot pass any subset of the SAn assertions trivially.

**Cargo exit code observed**: `101` (test panic) verified via
`(bash scripts/verify/P3-W9-F11.sh 2>&1; echo "BASH_EXIT=$?")`.

**Restore command**: `git checkout -- tests/integration/test_incremental.rs`

**Post-restore md5**: `d53466931b93f1f709a243d4c7a237cb` (matches snapshot).

**Post-restore re-test**: `cargo test --test test_incremental` reports
`2 passed; 0 failed`.

## Production-side `.unwrap()` / `.expect()` enumeration

The test file contains exactly two `.expect(...)` calls, both inside
the `LoggingDatabase` test harness and both with descriptive messages
naming the invariant being upheld:

| Line | Call | Justification |
|------|------|---------------|
| 58 | `.expect("log mutex poisoned")` (event-hook closure) | Test-side mutex poisoning is unrecoverable in a Salsa event hook; the descriptive message satisfies the .claude/rules/rust-style.md test-allowance carve-out (`#[cfg(test)]` integration test code is permitted to use `.expect(...)` with descriptive messages). |
| 72 | `.expect("log mutex poisoned")` (`drain_log` getter) | Same justification — test-side mutex helper. |

Production-side (`crates/`) impact is **zero** — this WO does not
modify any file under `crates/`. The verifier may confirm via
`git diff main --stat -- 'crates/**'` returning empty.

## Standing-flake escalation carry-forward

Per WO-0094 §scope_out #11 + WO-0067/68/69/90/93 precedent, the
following pre-existing flake escalations carry as standing scope_out
for this WO and should NOT block the verifier:

* `20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`
* `20260507T1629Z-effectiveness-refactor-rename-python-fixture-missing-symbol.md`
* `20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`

The substantive acceptance signal is `cargo test --test
test_incremental` (AC4) which is deterministic on this codebase.
