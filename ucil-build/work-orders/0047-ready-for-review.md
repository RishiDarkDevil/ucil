# WO-0047 — Ready for Review

**Feature**: P2-W7-F01 — G1 parallel-execution orchestrator
**Branch**: `feat/WO-0047-g1-parallel-execution-orchestrator`
**Final source commit (last code change)**: `5c7dfd1d9c7a5c86b8fd1ca5564188c116a63a34`
**Tip commit (this marker)**: `aeea43751556615e930743f3b2bd406fda6f8c1d`
**Status**: ready for critic + verifier

## What I verified locally

- **AC01 — `cargo build -p ucil-daemon` exits 0** ✅
- **AC02 — `cargo clippy -p ucil-daemon --all-targets -- -D warnings` exits 0** ✅
  - Pre-flight `rg -nE '^\s*///.*\b[A-Z][A-Z_0-9]+\b' crates/ucil-daemon/src/executor.rs`: hits exist but every uppercase identifier in new rustdoc is inside backticks (e.g. `` `G1ToolKind` ``, `` `G1Outcome` ``, `` `G1_MASTER_DEADLINE` ``); clippy-`doc_markdown` clean confirmed.
  - Three clippy hits surfaced and fixed during executor loop:
    - `clippy::single_match_else` + `clippy::option_if_let_else` + `clippy::unnecessary_result_map_or_else` on the `tokio::time::timeout` `match` → refactored `run_g1_source` to `unwrap_or_else(|_| ...)`.
    - `clippy::needless_collect` on `kinds: Vec<G1ToolKind>` → eliminated by re-iterating `sources` after the join (the futures vec borrows drop when `tokio::time::timeout` resolves, freeing the re-borrow path).
    - `clippy::missing_panics_doc` + `clippy::too_many_lines` on the test fn → `#[allow(...)]` on the test (assertions in tests are by design panicking; 199-line single-fn test is the WO's prescribed shape).
- **AC03 — `cargo test -p ucil-daemon executor::test_g1_parallel_execution -- --nocapture` exits 0** ✅ (1 passed; 0 failed; 4.75 s wall — sub-scenario c's 4.5 s per-source ceiling fires before the master 5 s deadline as expected).
- **AC04 — frozen selector `^pub async fn test_g1_parallel_execution` lives at module root of `crates/ucil-daemon/src/executor.rs`** ✅ (line 1793, NOT inside `mod tests {}`).
- **AC05 — dual-bound parallel-timing assertion** ✅ (`outcome.wall_elapsed_ms >= 180` AND `outcome.wall_elapsed_ms < 600` — see `executor.rs:1895` and `:1902`).
- **AC06 — Partial-Errored sub-assertion: 1 Errored + 3 Available** ✅ (`executor.rs:1937-1947`).
- **AC07 — Partial-TimedOut sub-assertion: 1 TimedOut + 3 Available, wall < 5500 ms** ✅ (`executor.rs:1985-2003`).
- **AC08 — Phase-2 Week-6 regression: `cargo test -p ucil-daemon -- plugin_manager::test_hot_cold_lifecycle plugin_manager::test_manifest_parser plugin_manager::test_lifecycle_state_machine plugin_manager::test_hot_reload plugin_manager::test_circuit_breaker`** ✅ (5 passed; 0 failed; cargo test syntax requires `--` separator, which I added vs the WO's literal command — same regression coverage).
- **AC09 — `cargo test -p ucil-daemon --test plugin_manager`** ✅ (3 passed; 0 failed).
- **AC10 — `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1 cargo test -p ucil-daemon --test plugin_manifests`** ✅ (2 passed; 0 failed).
- **AC11 — Phase-1 e2e regression: `cargo test -p ucil-daemon --test e2e_mcp_stdio --test e2e_mcp_with_kg`** ✅ (1+1 passed).
- **AC12 — `cargo test --test test_plugin_lifecycle`** ✅ (3 passed; 0 failed; warm-built `mock-mcp-plugin` first per the WO-0046 template).
- **AC13 — `cargo test --test test_lsp_bridge`** ✅ (5 passed; 0 failed).
- **AC14 — `cargo test --workspace --no-fail-fast`** ✅ (every test target reports `0 failed`).
- **AC15 — Coverage gate** ✅ (manual workaround, escalation `20260419-0152-monitor-phase1-gate-red-integration-gaps.md` still open — now 9th consecutive WO using this protocol).
  - `scripts/verify/coverage-gate.sh ucil-daemon 85 75` continues to fail with the same `RUSTC_WRAPPER` + corrupt-header `*.profraw` issue carried from WO-0039 retry-1 → WO-0046.
  - Standing protocol per WO-0046 lessons line 270:
    ```bash
    env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --summary-only
    LLVM_PROFDATA="$(rustc --print target-libdir)/../bin/llvm-profdata"
    find target/llvm-cov-target -name '*.profraw' -size 0 -delete
    find target/llvm-cov-target -name '*.profraw' -print0 \
      | while IFS= read -r -d '' f; do
          "$LLVM_PROFDATA" show "$f" >/dev/null 2>&1 || rm -f "$f"
        done
    env -u RUSTC_WRAPPER cargo llvm-cov report --package ucil-daemon --summary-only
    ```
  - Result: TOTAL line coverage **89.53%** (above 85% floor); `executor.rs` (the WO-0047 edit target) **92.82%** line / 94.44% function / 93.59% region — well above per-file floor; new G1 surface is covered by `test_g1_parallel_execution`.
- **AC16 — Stub-scan: zero new `todo!()` / `unimplemented!()` / `panic!("...not yet")` / `TODO` / `FIXME` hits in `executor.rs`** ✅ (HEAD: 0; `main`: 0; delta: 0).
- **AC17 — Allow-list verification (three-dot diff)** ✅:
  ```
  $ git diff --name-only main...HEAD
  crates/ucil-daemon/src/executor.rs
  crates/ucil-daemon/src/lib.rs
  scripts/verify/P2-W7-F01.sh
  ```
  No `Cargo.lock` change (no new deps added).
- **AC18 — `Cargo.toml` files NOT modified** ✅ (`git diff --name-only main...HEAD -- '*.toml'` returns empty). `cargo tree -p ucil-daemon | rg '^futures '` confirmed `futures` is NOT a direct dep — used `std::future::poll_fn` + `Pin<Box<dyn Future<Output=T> + Send>>` to implement the `join_all_g1` helper without pulling `futures::future::join_all`.
- **AC19 — `lib.rs` re-exports for ALL 9 new public symbols** ✅ (`grep -nE 'G1Source|G1Query|G1ToolKind|G1ToolStatus|G1ToolOutput|G1Outcome|execute_g1|G1_MASTER_DEADLINE|G1_PER_SOURCE_DEADLINE' crates/ucil-daemon/src/lib.rs` lists all 9). Cumulative-debt avoidance now confirmed across **6 consecutive WOs** (WO-0042 deferred → WO-0043 + 0044 + 0045 + 0046 + 0047 cleared).
- **AC22 — `tests/fixtures/**` NOT modified** ✅.
- **AC23 — `feature-list.json` and schema NOT modified by executor** ✅.
- **AC24 — `ucil-master-plan-v2.1-final.md` NOT modified** ✅.
- **AC25 — Commit cadence: 4 commits on the feature branch** ✅ (planner's `9075f24` + 4 executor commits — split per the WO's recommended cadence: 73535f1 types+trait, 5442276 orchestrator+helpers, 1e5febb test, 5c7dfd1 lib.rs re-exports + verify script).
- **AC26 — Branch is up to date with `origin`, working tree clean** ✅.

## Mutation patches (AC20 / AC21) — instructions for verifier

Both pre-baked mutations follow the WO-0046 lesson on the
`#![deny(warnings)]` cascade — the literal sed produces a
COMPILE-failure cascade (the orchestrator function body becomes flagged
as `unused_variable` / `unreachable_code` / etc. before the test runs),
so the **runtime-only variant** is the canonical mode.

### AC20 — Master-deadline guard neutered (runtime-only variant)

Replace the `execute_g1` body with:

```rust
pub async fn execute_g1<S>(query: G1Query, sources: Vec<Box<S>>, deadline: Duration) -> G1Outcome
where
    S: G1Source + ?Sized,
{
    let _ = query;
    let _ = sources;
    let _ = deadline;
    tokio::task::yield_now().await;
    G1Outcome::default()
}
```

(Drop the `#[tracing::instrument(...)]` attribute too — its
`fields(symbol = %query.symbol, source_count = sources.len())` will
warn `unused` once the args are dropped through `let _ = ...`. The
`#[allow(dead_code)]` on `run_g1_source` / `join_all_g1` may need to
be added if the linker complains about dead helpers.)

**Expected runtime failure**: `outcome.wall_elapsed_ms` is 0 (the
`yield_now().await` doesn't sleep), so the parallel-timing dual-bound
`>= 180 && < 600` panics at the lower bound `wall_elapsed_ms must be
>= 180 (proves at least one 200 ms sleep elapsed)`. The empty `Vec`
also fails the `outcome_a.results.len() == 4` check.

**Restore**: `git checkout -- crates/ucil-daemon/src/executor.rs`.

### AC21 — Per-source timeout neutered (runtime-only variant)

Replace the `tokio::time::timeout(per_source_deadline, source.execute(query)).await` line in `run_g1_source` with:

```rust
let _ = per_source_deadline;  // drop unused arg
Ok::<_, tokio::time::error::Elapsed>(source.execute(query).await)
```

**Expected runtime failure**: scenario (c)'s slow source (6 s sleep)
completes after the full 6 s without the timeout wrapper firing, so
the assertion `(c) outcome must contain exactly 1 TimedOut entry`
panics with `got 0`. Whole test wall-time exceeds the 5 500 ms ceiling
asserted at `executor.rs:2010` and trips the wall-time budget assertion.

**Restore**: `git checkout -- crates/ucil-daemon/src/executor.rs`.

## Things the verifier should know

1. **`tokio::join!` vs custom `join_all_g1`**: per the WO `scope_in`
   alternation, either is acceptable; I chose a tokio-only
   `std::future::poll_fn`-backed helper because the `Vec<Box<S>>`
   is dynamic-sized (the WO scope_in flags `join_all` is preferable
   for dynamic source counts) AND adding `futures` would violate
   AC18 (no new deps). Behaviourally equivalent to
   `futures::future::join_all` — see the helper's rustdoc at
   `executor.rs:1015-1024`.

2. **Tracing instrumentation**: I added a single `#[tracing::instrument(name = "ucil.group.structural", level = "debug", ...)]` parent span on `execute_g1` per master-plan §15.2 (`ucil.<layer>.<op>` naming). The per-source CHILD spans (`ucil.group.structural.<kind>`) are deferred to F02 per the WO's permission to "defer the actual instrumentation to F02 if it complicates this WO; document the deferral in the ready-for-review note" — adding the per-source instrument macros would have meant threading a `Span` argument through `run_g1_source`, which is best landed alongside F02's fusion logic where the per-source payload typing changes anyway.

3. **`G1Source: Send + Sync` supertraits + `Box<dyn G1Source>` on multi-thread tokio**: the test uses `Box<dyn G1Source + Send + Sync>` explicitly — the supertrait bounds on `G1Source` do **not** automatically propagate to the trait object, so the explicit `+ Send + Sync` is required for the `#[tokio::test(flavor = "multi_thread")]` flavor. The WO `scope_in` already specifies this (`Vec<Box<dyn G1Source + Send + Sync + 'static>>`).

4. **`enrich_find_definition` (WO-0037) was NOT modified**: F01 adds capability without removing the G1-fusion-lite hover-only helper. The non-removal is cited at `executor.rs:498-502` in the new G1 section's preamble comment. F02 (G1 fusion) will compose `execute_g1`'s outputs into a richer fused payload via the orchestrator; F01 ships the orchestrator + the trait only.

5. **Commit cadence**: 4 commits split per the WO recommendation
   (`acceptance` AC25 — "at least 3 commits"). Sizes: 203 LOC,
   183 LOC, 274 LOC, 81 LOC. The 274-LOC test commit exceeds the
   ~200 LOC soft target; covered by DEC-0005 module-coherence
   (single test fn + one `TestG1Source` impl + one `TestBehaviour`
   enum, all coherent for the single frozen selector).

6. **`Cargo.lock` not in diff**: confirmed via `git diff --name-only main...HEAD -- Cargo.lock` empty. No new dep introduction.

7. **Workspace-test pre-flight**: `cargo test --workspace --no-fail-fast` ran end-to-end with every result line `ok. 0 failed`. No tests were `#[ignore]`d, skipped, or quarantined.
