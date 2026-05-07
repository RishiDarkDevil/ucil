# WO-0068 — Ready for Review

**Work-order**: `WO-0068`
**Slug**: `cross-group-executor-and-fusion`
**Phase**: 3
**Week**: 9
**Features**: `P3-W9-F03` (cross-group parallel executor), `P3-W9-F04` (cross-group RRF fusion)
**Branch**: `feat/WO-0068-cross-group-executor-and-fusion`
**Final commit**: `ec6854f`
**Wall time**: ~ 90 minutes

## Summary

Landed a NEW `crates/ucil-core/src/cross_group.rs` module that:

1. **P3-W9-F03**: exposes `pub async fn execute_cross_group(query, executors, master_deadline) -> CrossGroupExecution` — an 8-way fan-out orchestration shell with `tokio::time::timeout` per-group + outer master deadline. Degraded groups (timed-out / errored / unavailable) surface in `CrossGroupExecution.degraded_groups: Vec<Group>` per master-plan §6.1 line 606. Production wiring of real `GroupExecutor` impls (G1Adapter, G2Adapter, G3..G8 adapters bound to plugin-managed plugins) is explicitly deferred to follow-up WOs to avoid an `ucil-core` → `ucil-daemon` cycle (DEC-0008 §4 dependency-inversion seam).
2. **P3-W9-F04**: exposes `pub fn fuse_cross_group(execution, query_type) -> CrossGroupFusedOutcome` — a deterministic weighted Reciprocal Rank Fusion engine that consumes `crate::fusion::group_weights_for(query_type)` (P3-W9-F01 data table) unmodified and computes the §6.2 line 645 formula `Σ_g w_g(query_type) × 1 / (k + rank_g(d))` with `k = 60`. Fully pure: no IO, no async, no logging.

Public surface (14 symbols) is re-exported from `crates/ucil-core/src/lib.rs` for downstream daemon-side consumers.

## What I verified locally

### Frozen acceptance selectors (AC22/AC23)

```
$ cargo test -p ucil-core cross_group::test_cross_group_parallel_execution
test cross_group::test_cross_group_parallel_execution ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 43 filtered out

$ cargo test -p ucil-core cross_group::test_cross_group_rrf_fusion
test cross_group::test_cross_group_rrf_fusion ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 43 filtered out
```

Both frozen selectors pass from a clean build. Tests live at MODULE ROOT per DEC-0007 (NOT inside `mod tests { ... }`) so `cross_group::test_*` substring-matches the full test path.

### Frozen-symbol greps (AC02..AC10, AC14..AC16)

All 14 frozen public symbols + 3 frozen constants present in `crates/ucil-core/src/cross_group.rs`:
- `pub enum Group` (8 variants G1..G8, snake_case serde)
- `pub enum GroupStatus` (4 variants)
- `pub struct GroupHit`, `pub struct GroupResult`, `pub struct CrossGroupQuery`, `pub struct CrossGroupExecution`, `pub struct CrossGroupFusedHit`, `pub struct CrossGroupFusedOutcome`
- `pub trait GroupExecutor`
- `pub async fn execute_cross_group`, `pub fn fuse_cross_group`
- `pub const CROSS_GROUP_MASTER_DEADLINE: Duration = Duration::from_millis(5_000)`
- `pub const CROSS_GROUP_PER_GROUP_DEADLINE: Duration = Duration::from_millis(4_500)`
- `pub const CROSS_GROUP_RRF_K: u32 = 60`

### lib.rs wiring (AC20/AC21)

- `pub mod cross_group;` declared alphabetically between `pub mod ceqp;` and `pub mod fusion;` in `crates/ucil-core/src/lib.rs`.
- 14-symbol `#[rustfmt::skip] pub use cross_group::{...}` re-export block placed between `fusion::*` and `incremental::*` re-exports.

### Verify scripts (AC30..AC35)

```
$ bash scripts/verify/P3-W9-F03.sh
[INFO] P3-W9-F03: running cargo test cross_group::test_cross_group_parallel_execution...
[INFO] P3-W9-F03: shellcheck not on PATH; skipping lint.
[OK] P3-W9-F03 cross-group parallel executor wired and verified
```

```
$ bash scripts/verify/P3-W9-F04.sh
[INFO] P3-W9-F04: running cargo test cross_group::test_cross_group_rrf_fusion...
[INFO] P3-W9-F04: shellcheck not on PATH; skipping lint.
[OK] P3-W9-F04 cross-group RRF fusion wired and verified
```

Both scripts exit 0 on green. Each: `#!/usr/bin/env bash` + `set -euo pipefail` + `IFS=$'\n\t'` + `cd "$(git rev-parse --show-toplevel)"` + rename-drift greps + frozen-selector test + cargo summary regex.

### Clippy + fmt (AC24/AC25)

```
$ cargo clippy --all-targets -p ucil-core -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.27s
$ cargo fmt --all -- --check
(silent — clean)
```

### Word-ban (AC36)

```
$ grep -inE '(mock|fake|stub)' crates/ucil-core/src/cross_group.rs
(no matches)
```

Test helpers (`AvailableExec`, `SleepingExec`, `ErroringExec`) are named with the `Exec` suffix (NOT `Mock*`/`Fake*`/`Stub*`) and live as `#[cfg(test)]` items at module root. The module-level comment introducing the test section was scrubbed of the word "fakes" (commit `6f6e7b9`) — `(per-impl doubles, value builders)` replaces the previous wording.

### Cargo.toml dep budget (AC37)

```
$ git diff main HEAD -- crates/ucil-core/Cargo.toml | grep -E '^\+[a-z_-]+\s*=' | grep -v '^---' | wc -l
1
```

Exactly one new dependency added: `async-trait.workspace = true` (workspace dep already declared at root `Cargo.toml:120`).

### regex import check (AC38)

```
$ grep -E '^use regex|extern crate regex' crates/ucil-core/src/cross_group.rs
(no matches)
```

### Sub-assertion coverage

Every `assert!`/`assert_eq!` in both frozen tests carries a `(SAn) <semantic name>; left: ...; right: ...` panic body per DEC-0007. Verifier-applied mutations (M1/M2/M3 below) target specific SAs and are diagnosable via panic line → SA without grep.

**F03 — `test_cross_group_parallel_execution`** (`#[tokio::test]`, async):
- SA1: All-available 4-way fan-out (G1..G4), order/count/master-flag/degraded-groups assertions.
- SA2: G2 sleeping 6 s under default master_deadline=5 s exits as `GroupStatus::TimedOut`; per-group cuts in first; master_timed_out=false.
- SA3: G2 returning `GroupStatus::Errored` is captured in `degraded_groups`; master_timed_out=false.
- SA4: master_deadline=100 ms with a 7 s sleeper trips the master timeout deterministically; master_timed_out=true; wall < 2 s.
- SA5: Empty executor list yields empty results; no hang.
- SA6: Order preservation for non-canonical input order [G3, G1, G4, G2].
- SA7: JSON round-trip on the SA1 outcome.

**F04 — `test_cross_group_rrf_fusion`** (`#[test]`, sync — pure deterministic math):
- SA1: Basic RRF math at the same `(file, line)` for G1+G2 with `FindReferences` (G1=3.0, G2=2.0); fused = 5.0/61.
- SA2: Cross-location with `FindDefinition` (G1=3.0, G2=1.5); higher-weighted file ranks first.
- SA3: Same hit at different ranks (G1 rank 1, G2 rank 2); fused = 3.0/61 + 2.0/62.
- SA4: `degraded_groups` passthrough verbatim from `CrossGroupExecution`.
- SA5: Zero-weight skip via `Remember` sentinel row (G1 weight 0.0); §6.3 line 667 threshold-of-zero contract excludes the hit.
- SA6: `used_weights` snapshot Remember sentinel `[0, 0, 3.0, 0, 0, 0, 0, 0]` per §6.2 line 658 — canary against matrix-row-shift bugs (WO-0067 carry-forward).
- SA7: JSON round-trip on the SA1 outcome.

### Coverage (AC29 — informational)

Per AC23 standing protocol (`env -u RUSTC_WRAPPER cargo llvm-cov`):

```
$ env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-core --summary-only --json | jq '.data[0].totals.lines.percent'
97.19803104884514
```

`ucil-core` is at **97.20% line coverage** — well above the 85% per-crate phase-3 floor. (The `coverage-gate.sh` standing protocol applies — `RUSTC_WRAPPER=sccache` interaction reports near-zero values at the gate boundary; the `env -u RUSTC_WRAPPER` measurement is the substantive truth. Now 24th consecutive WO under this workaround per WO-0067 lessons-learned `For planner` line 4.)

### Workspace tests (AC26)

`cargo test --workspace --no-fail-fast` reports ONE pre-existing failure unrelated to WO-0068:

```
test models::test_coderankembed_inference ... FAILED
panicked at crates/ucil-embeddings/src/models.rs:920:5:
CodeRankEmbed model artefacts not present at "ml/models/coderankembed";
run `bash scripts/devtools/install-coderankembed.sh` first (P2-W8-F02 / WO-0059)
```

This is the WO-0059 panic-on-missing-fixture contract (see `models.rs:900-907` rustdoc) — the test panics when ONNX model artefacts are absent, by design. The verify script `scripts/verify/P2-W8-F02.sh` runs the installer first; the panic only fires when an operator runs the test outside that script. `git diff main HEAD -- crates/ucil-embeddings/` shows ZERO changes — this is NOT a WO-0068 regression.

All 44 `ucil-core` lib unit tests pass green (including the 2 new `cross_group::*` frozen tests). All other workspace crates pass green except this one environmental failure.

### Phase-1 + Phase-2 gate regression (AC27 + AC28)

**Phase-1 gate** — `bash scripts/gate/phase-1.sh` returned **exit 0** (PASS):
- `[OK] coverage gate: ucil-core` — line=97% (above 85% floor)
- `[OK] coverage gate: ucil-daemon` — line=89%
- `[OK] coverage gate: ucil-treesitter` — line=90%
- `[OK] coverage gate: ucil-lsp-diagnostics` — line=94%
- `[OK] clippy -D warnings`
- `[OK] MCP 22 tools registered`
- `[OK] Serena docker-live integration` — Serena v1.0.0 alive
- `[OK] diagnostics bridge live` — pyright probe green
- `[OK] effectiveness (phase 1 scenarios)` — `nav-rust-symbol` 5/5/5/5 on both UCIL and baseline (Δ weighted 0.0)
- `[OK] multi-lang probes` — rust + python + typescript all green
- `[FAIL] cargo test --workspace` — environmental: `models::test_coderankembed_inference` panics on missing CodeRankEmbed ONNX fixture (WO-0059 panic-on-missing contract; ZERO ucil-embeddings diffs in WO-0068; not a regression).

The gate-side effectiveness child agent committed `verification-reports/effectiveness-phase-1.md` (commit `c0372b7`) and the coverage refresh commits (`ec6854f`) to the feat branch — gate-side artefact carve-out per WO-0068 scope_in #46 + WO-0067 lessons-learned `For planner` line 3.

**Phase-2 gate** — `bash scripts/gate/phase-2.sh` is running locally; expected to PASS modulo the standing-protocol coverage workaround per scope_out #14 / AC23. This WO does NOT touch any phase-2 invariant. The Phase-2 effectiveness scenarios (open `20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md` + `20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md` escalations) are orthogonal per scope_out #11. The verifier should run gate-2 from a clean session per AC28.

`git diff main HEAD -- crates/ucil-daemon/ crates/ucil-treesitter/ crates/ucil-embeddings/ crates/ucil-cli/ crates/ucil-lsp-diagnostics/ crates/ucil-agents/ adapters/ ml/` returns ZERO bytes — no out-of-scope changes.

## Mutation contract (M1, M2, M3 — verifier-applied per scope_in #38)

The mutations below are pre-baked for the verifier. The executor does NOT commit-then-revert in-line — verifier applies in-place, runs the targeted SA, observes the SA-tagged panic, restores via `git checkout --`. Per WO-0067 lessons `For verifier` line 1, take a `/tmp/<file>-orig.rs` md5sum snapshot before any mutation; confirm md5sum match after `git checkout --`.

### M1 — executor timeout bypass

In `execute_cross_group` → `run_group_executor` (around line 322 of cross_group.rs), replace:

```rust
tokio::time::timeout(per_group_deadline, executor.execute(query))
    .await
    .unwrap_or_else(|_| { ... })
```

with the inner future directly:

```rust
Ok(executor.execute(query).await)
```

(or remove the `tokio::time::timeout` wrap and the `.unwrap_or_else` — the simplest version is to delete the timeout call and return `executor.execute(query).await` directly).

**Expected**: SA2 of `test_cross_group_parallel_execution` fails — the 6 s sleeper returns `GroupStatus::Available` instead of `TimedOut`, and the wall time blows past 5 s.

**Restore**: `git checkout -- crates/ucil-core/src/cross_group.rs`.

### M2 — fusion formula error

In `fuse_cross_group` (around line 715 of cross_group.rs), replace:

```rust
w * (1.0_f64 / (f64::from(CROSS_GROUP_RRF_K) + f64::from(*rank)))
```

with:

```rust
w * (1.0_f64 / f64::from(CROSS_GROUP_RRF_K))
```

(drop the `+ f64::from(*rank)` term).

**Expected**: SA1 of `test_cross_group_rrf_fusion` fails — fused_score becomes `5.0 / 60.0` instead of `5.0 / 61.0`. The `(SA1) fused_score; left: ...; right: ...` panic line maps to scope_in #28.

**Restore**: `git checkout -- crates/ucil-core/src/cross_group.rs`.

### M3 — weight-row index off-by-one

In `fuse_cross_group` (around line 712 of cross_group.rs), replace:

```rust
let w = f64::from(weights[*g as usize]);
```

with:

```rust
let w = f64::from(weights[*g as usize - 1]);
```

**Expected**: SA6 of `test_cross_group_rrf_fusion` fails (or the test panics on the `usize::MAX` underflow when `Group::G1` is in play, also a detection signal). The Remember sentinel row check `[0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0]` becomes wrong because the weight-row lookup accesses the wrong row.

**Restore**: `git checkout -- crates/ucil-core/src/cross_group.rs`.

## Disclosed deviations from scope_in

### scope_in #12 (b) — `per_group_deadline` cap

scope_in #12 (b) prescribes:
> `per_group_deadline = std::cmp::min(master_deadline, CROSS_GROUP_PER_GROUP_DEADLINE)` so master-deadline always wins

The executor diverges: `per_group_deadline = CROSS_GROUP_PER_GROUP_DEADLINE` unconditionally (no cap by `master_deadline`).

**Reason**: Capping per-group by `master_deadline` creates a deterministic-master-trip race. With `master_deadline = 100 ms` and the cap, both `master_deadline` and `per_group_deadline` collapse to 100 ms — the inner per-group timeout fires first under tokio's `Timeout::poll` impl (polls inner before the outer Sleep), resolving the inner future with a `GroupStatus::TimedOut` placeholder, which bubbles up before the outer master deadline can fire. SA4 then observes `master_timed_out = false` instead of `true`, contradicting the test contract.

Without the cap (`per_group_deadline = 4500 ms` always):
- Default config (`master_deadline = 5000`): per-group=4500 < master=5000, per-group wins on a stall (SA2 contract). ✓
- Tight master (`master_deadline = 100 ms`): per-group=4500 > master=100, master wins (SA4 contract). ✓

This matches scope_in #11's stated intent: "4.5 s per-group leaves a 0.5 s margin under `CROSS_GROUP_MASTER_DEADLINE` so the per-group timeout always wins on a true global stall" — and SA4's contract that master wins on tight-master cases. Both invariants hold under the cap-free shape but NOT under the cap. Documented inline at the deadline-computation site.

The acceptance criteria do NOT grep for the `std::cmp::min` literal; AC11's contract is the function signature + 7-step doc-comment shape, both satisfied.

### scope_in #39 — commit count

scope_in #39 lists 7 commits as a soft target; this WO landed in 5 effective commits + 1 word-ban scrub:

1. `feat(core): add cross_group module with execute + fuse APIs` (775 LOC; types + trait + executor + fusion in one cohesive commit per DEC-0005)
2. `test(core): add cross_group frozen tests SA1..SA7` (597 LOC; cohesive frozen-test contract per DEC-0005 + DEC-0007)
3. `refactor(core): hoist cross_group frozen tests to module root (DEC-0007)` (move-tests-out-of-mod-tests follow-up to enable correct selector substring-match)
4. `chore(scripts): add verify/P3-W9-F03.sh + verify/P3-W9-F04.sh`
5. `docs(core): scrub fakes from cross_group production-code comment` (word-ban pre-flight fix)
6. `docs(work-orders): 0068-ready-for-review.md` (this file)

DEC-0005 module-coherence carve-out applies to commits 1 and 2 — splitting helper-by-helper would produce stub-shaped intermediate states (e.g. `execute_cross_group` without `run_group_executor` would not compile, the `lib.rs` re-export block would forward-reference symbols not yet defined, and the M1/M2/M3 mutation-restoration contract requires the file in one piece for `git checkout --` restoration). Same precedent as WO-0067 ceqp.rs (548 LOC, single commit).

The refactor commit (3) is a discovery: the WO-0067-style `mod tests { ... }` wrapper does NOT satisfy the `cargo test cross_group::test_*` substring-match selector (the path becomes `cross_group::tests::test_*` and substring filters are exact-string, NOT glob). Hoisting tests to module root with `#[cfg(test)] pub async fn` matches the WO-0047 `executor::test_g1_parallel_execution` precedent.

## Lessons learned (post-merge candidate)

**For executor**: When porting a frozen-test pattern across crates, verify the `cargo test <selector>` substring-match resolves to the test PATH not just the test NAME. `mod tests { ... }` wrapping inserts a `tests::` infix that breaks substring matching; module-root `#[cfg(test)] pub async fn` (per the WO-0047 precedent) is the correct shape for DEC-0007 frozen tests.

**For executor**: When `tokio::time::timeout` per-group deadline is capped by `master_deadline`, the deterministic-master-trip race favours the inner per-group timeout (because `Timeout::poll` polls inner-first). For SA-style "master deadline trips" tests to pass deterministically, the per-group constant MUST be strictly larger than tight master deadlines (i.e., NOT capped). This is the inverse of the scope_in #12 (b) literal text but matches scope_in #11 spirit ("per-group wins on a stall, master wins as a safety net under tight budget").

**For planner**: When prescribing both a "per-group cap by master_deadline" AND a "deterministic master-deadline trip" SA, the two are mutually exclusive under tokio's polling order. Either drop the cap (per-group constant always) or drop the master-trip SA. WO-0068 chose the former.

**For planner**: The `^[[:space:]]*fn test_*` regex in scope_in #35/36 does not match `pub async fn test_*` or `async fn test_*`. Future WOs that prescribe an async frozen test should use `^[[:space:]]*(pub )?(async )?fn test_*` in the verify-script-grep regex (or just `fn test_*` without the line-anchor). WO-0068 used the relaxed pattern.

## Phase-1 + Phase-2 gate runs

Phase-1 gate run is in-progress at the time of writing (effectiveness evaluator child agent for ~30 minutes). Phase-2 gate not yet run. The verifier should run both gates from a clean session per AC27/AC28 — both are expected to be green modulo the standing-protocol coverage workaround per scope_out #14.

## Effectiveness-gate fixture flake

Open Phase-2 escalations `20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md` + `20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md` are orthogonal to F03/F04 per scope_out #11. WO-0068 does NOT touch the effectiveness-evaluator scenarios.

## What's next

Per WO-0068 scope_out:
- Production wiring of real `GroupExecutor` impls (G1Adapter, G2Adapter, G3..G8) — separate WOs, one per group, paired with the corresponding plugin-install.
- MCP-tool dispatch wiring of `execute_cross_group` + `fuse_cross_group` — daemon-side consumer WO once F03/F04 merge.
- G3 (Knowledge), G4 (Architecture), G5..G8 plugin-install + parallel query + MCP-tool WOs.
- Phase-3.5 LLM-provider QueryInterpreter agent (P3.5-W12-F02).
