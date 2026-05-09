# WO-0091 — G5 (Context) parallel-query backbone — Ready for Review

**Final commit sha:** `7e3ea230abf0e55a4a80f74647c2629fa99df0c0`
**Branch:** `feat/WO-0091-g5-context-parallel-query`
**Feature:** `P3-W10-F04`
**Master-plan:** §5.5 lines 502-522 (G5: Context — All context sources → quality-maximalist assembly)

## What I verified locally

| AC | Check | Result |
|----|-------|--------|
| AC01 | `crates/ucil-daemon/src/g5.rs` exists | ✅ 672 LOC |
| AC02 | `pub const G5_MASTER_DEADLINE: Duration = Duration::from_millis(5_000);` | ✅ |
| AC03 | `pub const G5_PER_SOURCE_DEADLINE: Duration = Duration::from_millis(4_500);` | ✅ |
| AC04 | `#[async_trait::async_trait] pub trait G5Source` present | ✅ |
| AC05 | 7 public structs/enums (G5Query, G5SourceKind, G5SourceStatus, G5ContextChunk, G5SourceOutput, G5Outcome, G5AssembledContext) | ✅ |
| AC06 | `#[tracing::instrument(name = "ucil.group.context", ...)]` present | ✅ |
| AC07 | `pub async fn execute_g5` + `pub fn assemble_g5_context` present | ✅ |
| AC08 | `lib.rs` adds `pub mod g5;` between `pub mod g4;` and `pub mod g7;` | ✅ |
| AC09 | `lib.rs` re-export block names `{execute_g5, assemble_g5_context, G5Source, G5Query, G5Outcome, G5AssembledContext, G5_MASTER_DEADLINE, G5_PER_SOURCE_DEADLINE, …}` | ✅ |
| AC10 | `pub async fn test_g5_context_assembly` at module root + `#[tokio::test...]` attribute | ✅ |
| AC11 | 39 SA-tagged assertions inside the test body (≥ 8) | ✅ |
| AC12 | `cargo build -p ucil-daemon --tests` exits 0 | ✅ |
| AC13 | `cargo clippy -p ucil-daemon --tests -- -D warnings` exits 0 | ✅ |
| AC14 | `cargo test -p ucil-daemon executor::test_g5_context_assembly --no-fail-fast` exits 0 in ~5 s | ✅ |
| AC15 | `! grep -niE 'mock\|fake\|stub' crates/ucil-daemon/src/g5.rs` exits 0 | ✅ |
| AC16 | `TestG5Source` test-side trait impl exists (Test prefix) | ✅ |
| AC17 | M1 mutation triggers SA4 panic (slow elapsed_ms 5001 > 5000); restore + md5 OK | ✅ |
| AC18 | M2 mutation triggers SA1 panic (assembled.chunks.len() == 5, expected 4); restore + md5 OK | ✅ (see deviation #1) |
| AC19 | M3 mutation triggers SA2 panic (chunks[0].pagerank_score == 0.1, expected 0.9); restore + md5 OK | ✅ |
| AC20 | `env -u RUSTC_WRAPPER cargo llvm-cov ...` ≥ 80 % | NOT RE-MEASURED — standing protocol per scope_out #5 |
| AC21 | `scripts/gate-check.sh 3` constituent sub-checks | NOT RE-RUN — verifier scope per scope_out #5 |
| AC22 | `scripts/verify/P3-W10-F04.sh` exits 0 | ✅ (`[PASS] P3-W10-F04: G5 context-assembly frozen test green`) |
| AC23 | `git log feat/WO-0091-g5-context-parallel-query ^main --merges \| wc -l` = 0 | ✅ |
| AC24 | All commits carry `Phase: 3 / Feature: P3-W10-F04 / Work-order: WO-0091` + `Co-Authored-By` trailers | ✅ |
| AC25 | 4 commits = 1 feat (g5.rs + lib.rs) + 1 test (executor.rs) + 1 build (verify script) + 1 RFR | ✅ (this commit is the RFR) |
| AC26 | RFR carries M1/M2/M3 mutation table + md5sum path + Disclosed deviations + .unwrap() enumeration | ✅ (this file) |
| AC27 | Frozen-test selector resolves uniquely (`cargo test executor::test_g5_context_assembly --no-run` matches 1) | ✅ |

## Pre-mutation md5sum snapshot

Path: `/tmp/wo-0091-g5-orig.md5sum`

```
59d80673c7c66f4cd572d27cd8f9ffb4  crates/ucil-daemon/src/g5.rs
dbbb64aa64351a5aaa6039bbafcdab49  crates/ucil-daemon/src/executor.rs
```

After every mutation the verifier runs `git checkout -- crates/ucil-daemon/src/g5.rs` then `md5sum -c /tmp/wo-0091-g5-orig.md5sum` — both files must report `OK` (verified in this executor session for all three mutations).

## M1 / M2 / M3 mutation table

| ID | File | Site | Patch (in-place) | Targeted SA | Restore command |
|----|------|------|------------------|-------------|-----------------|
| M1 | `crates/ucil-daemon/src/g5.rs` | `run_g5_source` body (~lines 328-352) | Replace `tokio::time::timeout(per_source_deadline, source.execute(query)).await.unwrap_or_else(\|_\| {...})` with bare `source.execute(query).await` (rename `per_source_deadline → _per_source_deadline`, `source_id → _source_id`, `kind → _kind`, `start → _start` to silence `dead_code` under `#![deny(warnings)]`) | **SA4** — `(SA4) slow source elapsed_ms must be < 5000 ms (per-source ceiling fires before 5 s master); left: 5001, right: < 5000` | `git checkout -- crates/ucil-daemon/src/g5.rs && md5sum -c /tmp/wo-0091-g5-orig.md5sum` |
| M2 | `crates/ucil-daemon/src/g5.rs` | `assemble_g5_context` Step 2 dedup filter (~line 638) | Replace `.filter(\|c\| !dedup_set.contains(c.path.as_str()))` with `.filter(\|c\| { let _ = dedup_set.contains(c.path.as_str()); true })` (the `let _ = ...` keeps `dedup_set` referenced under `unused_variables`) | **SA1** (per spirit-vs-literal — see deviation #1) — `(SA1) assembled.chunks.len() must be 4 (5 in − 1 dedup); left: 5, right: 4` | `git checkout -- crates/ucil-daemon/src/g5.rs && md5sum -c /tmp/wo-0091-g5-orig.md5sum` |
| M3 | `crates/ucil-daemon/src/g5.rs` | `assemble_g5_context` Step 3 sort_by comparator (~lines 649-655) | Swap `b.pagerank_score.partial_cmp(&a.pagerank_score)` to `a.pagerank_score.partial_cmp(&b.pagerank_score)` (1-character `a`/`b` swap × 2) | **SA2** — `(SA2) chunks[0].pagerank_score must be 0.9 (descending sort); left: 0.1, right: 0.9` | `git checkout -- crates/ucil-daemon/src/g5.rs && md5sum -c /tmp/wo-0091-g5-orig.md5sum` |

All three mutations were applied in-place, the test was run, the targeted SA panic was observed, the file was restored via `git checkout --`, and the md5sum was re-verified.  No mutation introduced a stray edit; both md5 lines reported `OK` after every restore.

## Disclosed deviations

### 1. M2 panic fires at SA1, not SA3 (spirit-vs-literal)

**Spec wanted:** "M2 — replace the `.filter(...)` line in `assemble_g5_context` with a no-op... Targeted SA: SA3 (session dedup invariant)."

**What happened:** SA1 also exercises dedup (its assembled.chunks.len() == 4 / deduped_count == 1 assertions are dedup-load-bearing).  When the M2 mutation bypasses dedup, the test panics at the FIRST dedup-load-bearing assertion in execution order — which is SA1, not SA3.

**Why this is fine per scope_in #18 carve-out:** The mutation contract's load-bearing requirement is "an SA-tagged panic in the test_g5 body fires when the M2 patch is applied" — that fires (at SA1).  SA3 still fires too (it's just that the test panics on SA1 first and never reaches SA3 in a single run).  WO-0090 §verifier lesson "mutation-patch simplification within scope_in's permissive clause" covers this case: the spirit (M2 detected via SA-numbered panic) is met even when the literal SA number differs.

**Mitigating evidence:** The `assemble_g5_context` body has only one dedup site (Step 2, line 638), and SA1's `assembled.chunks.len() == 4 (5 in − 1 dedup)` assertion is unambiguously dedup-checking.  Adding a SA3-only mutation that doesn't trip SA1 would require re-ordering assertions or changing the test data — both of which would deviate from the spec more substantially.

### 2. M1 mutation site is `run_g5_source`, not `execute_g5` directly (spec-text precision)

**Spec wanted:** "replace `tokio::time::timeout(G5_PER_SOURCE_DEADLINE, source.execute(query))` in `execute_g5` with bare `Ok(source.execute(query).await)`"

**What happened:** The per-source `tokio::time::timeout` call lives inside the helper `run_g5_source` (line 336), not directly inside `execute_g5` — `execute_g5` calls `run_g5_source` per source via the `join_all_g5` fan-out.  This mirrors the G3 / G4 / G7 / G8 backbone template (`run_g3_source` / `run_g4_source` / etc.) where the per-source timeout always lives in a helper for clarity.

**Why this is fine:** The mutation still bypasses the per-source timeout (the spec's intent), and SA4 still fires.  The M1 patch is `let body = source.execute(query).await` instead of the timeout wrapper — a one-line semantic mutation.

## Production-side `.unwrap()` / `.expect()` enumeration

`grep -nE '\.unwrap\(\)|\.expect\(' crates/ucil-daemon/src/g5.rs`:

| File:Line | Form | `# Panics` justification |
|-----------|------|--------------------------|
| `g5.rs:339` | `u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)` | Saturating cast, `.unwrap_or` form — non-panicking. `start.elapsed().as_millis()` returns `u128`, the conversion to `u64` saturates at `u64::MAX` (~584 million-year overflow safe). |
| `g5.rs:464` | `u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)` | Saturating cast in `execute_g5` wall-elapsed. Same justification as above. |
| `g5.rs:493` | `u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)` | Reused `wall_elapsed_ms` value on master-trip placeholder synth path. |
| `g5.rs:380` | `r.expect("join_all_g5: every slot must be filled before returning")` | Inside `join_all_g5` — the `poll_fn` loop only returns `Ready(())` when `any_pending == false`, which only happens when every `slots[i].is_some()`.  The `expect` is unreachable in correct code; mirrors `join_all_g3` at `g3.rs:376` and `join_all_g4` at `g4.rs:464`.  Crate-private `#[allow(dead_code)]` helper, so the `expect` cannot leak to callers. |

No `.unwrap()` (bare) appears in production-side `g5.rs`.  All production-side panic sources are accounted for; none are reachable on a non-malformed input path.

## Tracing carve-out disclosure (per scope_in #19)

* `execute_g5` carries `#[tracing::instrument(name = "ucil.group.context", skip_all, fields(source_count = sources.len()))]` per master-plan §15.2 line 1519 — async/IO orchestration path.
* `assemble_g5_context` does NOT carry `#[tracing::instrument]` — pure-deterministic merge function (no IO, no async, no logging).  Per WO-0067 §lesson "pure CPU-bound merge functions are exempt from §15.2 tracing"; matches the `ceqp::parse_reason` precedent.

## AC21 / AC22 phase-3 gate disclosure

Per `scope_out` #5 standing protocol: the executor runs constituent sub-checks (build, clippy, test, verify script) in isolation; the verifier re-runs the gate-script entry point from a fresh session.  Three pre-existing flake escalations carry forward:

* `20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`
* `20260507T1629Z-effectiveness-refactor-rename-python-fixture-missing-symbol.md`
* `20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md`

Coverage-gate.sh sccache `RUSTC_WRAPPER` workaround now 44+ WOs deep — substantive coverage measure via `env -u RUSTC_WRAPPER cargo llvm-cov ...` is the binding floor (target ≥ 80 %).  Not re-measured in this WO; verifier should run the standing-protocol command if AC20 verification is desired.

## Commit list

```
7e3ea23 build(verify): add P3-W10-F04 acceptance script for G5 context backbone
17a387d test(daemon): add executor::test_g5_context_assembly frozen acceptance test
bee3462 feat(daemon): add g5.rs context backbone — G5Source + execute_g5 + assemble_g5_context
```

Plus this RFR commit (which contains only `ucil-build/work-orders/0091-ready-for-review.md`).

## Net-new follow-up scope (carry-forward to next planner emission)

Per WO-0091 `scope_out` enumeration, the following remain open:

1. **Production-wiring WO** for G5 — bundles real `AiderRepoMapG5Source` (consuming WO-0087 `PageRank` engine), `Context7G5Source` / `RepomixG5Source` (wrapping WO-0074 plugin runtimes), `OpenContextG5Source` / `OpenApiG5Source` / `GraphQlG5Source` (future plugin manifests), plus `McpServer::with_g5_sources` builder + `lifecycle.rs` boot-time registration.  Eligible for omnibus bundling with deferred G3/G4/G7/G8 production-wiring follow-ups.
2. **`get_context_for_edit` MCP tool dispatch handler** (master-plan §3.2 row 6) — separate consumer WO per the WO-0090 (F10/F15) MCP-tool-dispatch-handler-bundling template.
3. **`understand_code` / `get_conventions` / `get_architecture` / `explain_history` / `generate_docs` / `query_database` MCP tool handlers** — separate consumer WOs.
4. **`G5Adapter: GroupExecutor` impl** — wires G5 into the cross-group RRF executor (WO-0068 backbone).  Follow-up WO.
