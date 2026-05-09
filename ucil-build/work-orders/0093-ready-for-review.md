---
wo_id: WO-0093
slug: warm-processors-agent-scheduler
phase: 3
week: 10
features: ["P3-W10-F13"]
branch: feat/WO-0093-warm-processors-agent-scheduler
final_commit: 130dfed9e311b39c9bac2976aeb348753ea9f410
status: ready-for-review
created_by: executor
created_at: 2026-05-09T13:00:00Z
---

# WO-0093 — ready for review

The four warm-tier promotion processors landed in
`crates/ucil-daemon/src/agent_scheduler.rs` (NEW file, ~1900 LOC
including the SA1..SA8 frozen test and the in-memory
`TestWarmProcessorSource`).

## What I verified locally

### Build / lint / format / test

* `cargo check -p ucil-daemon` — exits 0.
* `cargo clippy -p ucil-daemon -- -D warnings` — exits 0 (production).
* `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — exits 0
  (production + tests; `#[allow(...)]` annotations on the frozen test
  cover the `clippy::too_many_lines` / `missing_panics_doc` /
  `uninlined_format_args` / `items_after_statements` /
  `significant_drop_in_scrutinee` / `significant_drop_tightening` /
  `no_effect_underscore_binding` lints).
* `cargo fmt --check -p ucil-daemon` — exits 0.
* `cargo test -p ucil-daemon agent_scheduler::test_warm_processors`
  — `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 172
  filtered out; finished in 0.00s`.
* `scripts/verify/P3-W10-F13.sh` — exits 0; the script sanity-greps
  the lib.rs declaration + source file, asserts `cargo test
  ... -- --list` resolves the selector to exactly 1 test, then runs
  the full test asserting `test result: ok. 1 passed`.

### Per-AC grep coverage

All grep-based acceptance criteria pass:

| AC | What it asserts | Result |
|---|---|---|
| AC1  | `crates/ucil-daemon/src/agent_scheduler.rs` exists | PASS |
| AC2  | `^pub mod agent_scheduler;` in lib.rs | PASS |
| AC3  | `^pub const OBSERVATION_PROCESSOR_INTERVAL:` | PASS |
| AC4  | `^pub const CONVENTION_SIGNAL_PROCESSOR_INTERVAL:` | PASS |
| AC5  | `^pub const ARCHITECTURE_DELTA_PROCESSOR_INTERVAL:` | PASS |
| AC6  | `^pub const DECISION_LINKER_INTERVAL:` | PASS |
| AC7  | `^pub const CONVENTION_MIN_EVIDENCE:` | PASS |
| AC8  | `^pub const OBSERVATION_DEDUP_THRESHOLD:` | PASS |
| AC9  | `^pub enum WarmProcessorKind\b` | PASS |
| AC10 | `\bObservation\b` variant present | PASS |
| AC11 | `\bConventionSignal\b` variant present | PASS |
| AC12 | `\bArchitectureDelta\b` variant present | PASS |
| AC13 | `\bDecisionLinker\b` variant present | PASS |
| AC14 | `^pub trait WarmProcessorSource\b` | PASS |
| AC15 | `^pub struct AgentScheduler\b` | PASS |
| AC16 | `^pub struct AgentSchedulerHandle\b` | PASS |
| AC17 | `^pub struct AgentSchedulerStats\b` | PASS |
| AC18 | `^pub struct WarmPromotionResult\b` | PASS |
| AC19 | `pub async fn run_observation_processor` | PASS |
| AC20 | `pub async fn run_convention_signal_processor` | PASS |
| AC21 | `pub async fn run_architecture_delta_processor` | PASS |
| AC22 | `pub async fn run_decision_linker_processor` | PASS |
| AC23 | `name = "ucil.agent.warm_processor"` | PASS |
| AC24 | `pub async fn test_warm_processors` (module-root) | PASS |
| AC25 | `tokio::test([..]start_paused = true[..])` | PASS |
| AC26 | `cargo test ... -- --list` ⇒ 1 match | PASS (verifier note: AC literal omits `--` separator; modern cargo requires it per WO-0089 §B carry) |
| AC27 | `cargo test ... \| grep 'test result: ok. 1 passed'` | PASS |
| AC28 | `cargo clippy -p ucil-daemon -- -D warnings` | PASS |
| AC29 | `cargo fmt --check -p ucil-daemon` | PASS |
| AC30 | NO `mock\|fake\|stub` (case-insensitive) | PASS |
| AC31 | NO `todo!()\|unimplemented!()\|NotImplementedError` | PASS |
| AC32 | NO `#[ignore]` | PASS |
| AC33 | NO `std::process::Command` | PASS |
| AC34 | NO `unsafe { ... }` / `unsafe fn` | PASS |
| AC35 | `scripts/verify/P3-W10-F13.sh` exists | PASS |
| AC36 | verify script is `chmod +x` | PASS |
| AC37 | verify script exits 0 | PASS |

## Mutation contract (M1 / M2 / M3)

Each mutation was applied in-place via `Edit`, the test was run, the
expected SA panic was observed, then the file was restored via
`git checkout -- crates/ucil-daemon/src/agent_scheduler.rs`. The pre-
mutation md5 (`054996053c367ad1a3fbe361d6ba878b`) matches the post-
restore md5 in every case.

### M1 — `OBSERVATION_PROCESSOR_INTERVAL: Duration::from_secs(60)` → `Duration::from_secs(7200)`

```diff
-pub const OBSERVATION_PROCESSOR_INTERVAL: Duration = Duration::from_secs(60);
+pub const OBSERVATION_PROCESSOR_INTERVAL: Duration = Duration::from_secs(7200);
```

* **Expected SA failure**: SA1 (observation processor never fires
  before t=120s).
* **Observed**: `panicked at crates/ucil-daemon/src/agent_scheduler.rs:1538:5:`
  `assertion 'left == right' failed: (SA1) observation processor fires`
  `once at t=60s; left: None, right: Some(1)`.
* **Restore**: `git checkout -- crates/ucil-daemon/src/agent_scheduler.rs`
  → md5 match.

### M2 — `if group.len() >= CONVENTION_MIN_EVIDENCE` → `if group.len() >= CONVENTION_MIN_EVIDENCE + 5`

```diff
-        if group.len() >= CONVENTION_MIN_EVIDENCE {
+        if group.len() >= CONVENTION_MIN_EVIDENCE + 5 {
```

(in `run_convention_signal_processor`)

* **Expected SA failure**: SA2 (P1 group, evidence=3, falls below
  the 8-threshold, so 0 warm rows are inserted instead of 1).
* **Observed**: `panicked at crates/ucil-daemon/src/agent_scheduler.rs:1606:9:`
  `assertion 'left == right' failed: (SA2) 1 warm_convention inserted`
  `(P1 group only, 3 ≥ CONVENTION_MIN_EVIDENCE); left: 0, right: 1`.
* **Restore**: `git checkout -- crates/ucil-daemon/src/agent_scheduler.rs`
  → md5 match.

### M3 — group-by `(change_type, file_path)` → group-by `change_type` only

```diff
     let mut groups: BTreeMap<(String, String), Vec<HotArchitectureDeltaRow>> = BTreeMap::new();
     for row in &hot_rows {
         groups
-            .entry((row.change_type.clone(), row.file_path.clone()))
+            .entry((row.change_type.clone(), String::new()))
             .or_default()
             .push(row.clone());
     }
```

(in `run_architecture_delta_processor`)

* **Expected SA failure**: SA3 (warm summary becomes ambiguous across
  files; the `file_path` substring assertion fails because the
  summary's `file_path` slot is empty).
* **Observed**: `panicked at crates/ucil-daemon/src/agent_scheduler.rs:1723:9:`
  `(SA3) summary mentions file_path src/a.rs; left: "2 delta(s) of`
  `type add on "`.
* **Restore**: `git checkout -- crates/ucil-daemon/src/agent_scheduler.rs`
  → md5 match.

## Carries from scope_in / scope_out

* **scope_out #1-#5**: production wiring of a real-`KnowledgeGraph`
  `WarmProcessorSource` impl + daemon `lifecycle.rs` startup wiring +
  `tier_merger` integration + KG schema additions + the
  `ucil-core/src/warm_processors.rs` layering relocation are all
  deferred to follow-up WOs per the work-order's explicit scope-out.
* **scope_out #6**: NO criterion benchmarks — the feature has no
  performance bound; bench follow-up WO if needed.
* **scope_out #7-#8**: standing coverage-gate sccache workaround +
  effectiveness-gate flake remediation are unchanged from prior WOs;
  bucket-B candidates.
* **scope_in #15** (Cargo.toml deps): NO new external crate added.
  `tokio-util` was avoided by using `tokio::sync::watch` for the
  cancel signal + `tokio::task::JoinSet` for task management — both
  in core tokio. The hand-rolled Jaccard token-overlap (~30 LOC)
  replaces a `strsim` / `levenshtein` dep per
  `.claude/rules/rust-style.md` §`Crate layout`. The only Cargo.toml
  delta is a dev-dep entry: `tokio = { workspace = true, features =
  ["test-util"] }`, which unions `test-util` onto the workspace's
  `full` feature set so the frozen test's
  `tokio::time::pause` / `advance` + `start_paused = true` resolve.

## Span / observability notes

* Each `run_<kind>_processor` carries a `#[tracing::instrument(name =
  "ucil.agent.warm_processor", skip(source), fields(kind = "..."))]`
  span (the literal `name = "ucil.agent.warm_processor"` is asserted
  by AC23).
* The per-tick body inside `processor_task` opens a
  `tracing::info_span!("ucil.agent.warm_processor.tick", kind = ?kind)`
  span — captured by the SA7 `SpanCounter` tracing-subscriber Layer
  in the frozen test (the test asserts ≥ 4 spans observed by t=240s).

## Files touched

```
crates/ucil-daemon/Cargo.toml                            (+9 lines: dev-dep + comment)
crates/ucil-daemon/src/agent_scheduler.rs                (+1858 lines, NEW file)
crates/ucil-daemon/src/lib.rs                            (+3 lines: pub mod + pub use)
scripts/verify/P3-W10-F13.sh                             (+39 lines, NEW file, chmod +x)
ucil-build/work-orders/0093-ready-for-review.md          (this file)
```

NO modification to:

* `ucil-build/feature-list.json` (frozen oracle)
* `ucil-master-plan-v2.1-final.md` (immutable spec)
* `tests/fixtures/**`
* any of the forbidden_paths siblings under `crates/ucil-daemon/src/`
* `crates/ucil-core/**` / `crates/ucil-treesitter/**` / etc.

## Branch state

* Branch: `feat/WO-0093-warm-processors-agent-scheduler`
* Commits ahead of `main`: 5 (skeleton, processors, scheduler, test,
  re-export+verify).
* Final commit sha: `130dfed9e311b39c9bac2976aeb348753ea9f410`.
* Working tree clean; branch is up-to-date with origin.

The pre-merge `feat/WO-0093-...` branch is fast-forwardable onto
`main` (no merges introduced; the AC `git log feat/... ^main --merges`
returns empty).
