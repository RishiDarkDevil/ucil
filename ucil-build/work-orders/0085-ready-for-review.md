# WO-0085 — Ready for review

**Final commit (executor)**: `7d26f0d7e0ebdeb41f6108be907384c7ff0ff9d7`
**Branch**: `feat/WO-0085-g7-quality-pipeline-foundation`
**Features**: P3-W11-F01 (G7Source trait + execute_g7), P3-W11-F05 (severity-weighted merge), P3-W11-F06 (quality_issues table tracking)

## What I verified locally

| Gate | Command | Result |
|------|---------|--------|
| F01 frozen test | `cargo test -p ucil-daemon g7::test_g7_parallel_pipeline` | `1 passed; 0 failed; … finished in 4.50s` |
| F05 frozen test | `cargo test -p ucil-daemon g7::test_g7_severity_merge` | `1 passed; 0 failed; … finished in 0.00s` |
| F06 frozen test | `cargo test -p ucil-lsp-diagnostics quality_pipeline::test_quality_issues_tracking` | `1 passed; 0 failed; … finished in 1.10s` |
| 22-tool catalog carry | `cargo test -p ucil-daemon server::test_all_22_tools_registered` | `1 passed; 0 failed` |
| Daemon clippy | `cargo clippy -p ucil-daemon --all-targets -- -D warnings` | exit 0 |
| LSP-diagnostics clippy | `cargo clippy -p ucil-lsp-diagnostics --all-targets -- -D warnings` | exit 0 |
| Daemon fmt | `cargo fmt -p ucil-daemon --check` | exit 0 |
| LSP-diagnostics fmt | `cargo fmt -p ucil-lsp-diagnostics --check` | exit 0 |
| F01 verify script | `bash scripts/verify/P3-W11-F01.sh` | PASS |
| F05 verify script | `bash scripts/verify/P3-W11-F05.sh` | PASS |
| F06 verify script | `bash scripts/verify/P3-W11-F06.sh` | PASS |
| No merge commits | `git log feat/WO-0085-g7-quality-pipeline-foundation ^main --merges \| wc -l` | `0` |
| Word-ban scrub (production-side g7.rs, lines 1..733) | `head -n 733 crates/ucil-daemon/src/g7.rs \| grep -niE 'mock\|fake\|stub'` | empty |
| `pub mod g7;` placed alphabetically | `grep -nE '^pub mod (g4\|g7\|lancedb)' crates/ucil-daemon/src/lib.rs` | `g4 → g7 → lancedb_indexer` |
| `pub async fn execute_g7` exists | `grep -nE 'pub async fn execute_g7' crates/ucil-daemon/src/g7.rs` | matches |
| `pub fn merge_g7_by_severity` exists | `grep -nE 'pub fn merge_g7_by_severity' crates/ucil-daemon/src/g7.rs` | matches |
| `pub trait G7Source` exists | `grep -nE 'pub trait G7Source' crates/ucil-daemon/src/g7.rs` | matches |
| `pub enum Severity` exists | `grep -nE 'pub enum Severity' crates/ucil-daemon/src/g7.rs` | matches |
| `pub async fn soft_delete_resolved_quality_issues` exists | `grep -nE 'pub async fn soft_delete_resolved_quality_issues' crates/ucil-lsp-diagnostics/src/quality_pipeline.rs` | matches |
| `ucil.group.quality` span on `execute_g7` | `grep -nE 'tracing::instrument.*ucil.group.quality' crates/ucil-daemon/src/g7.rs` | matches (multi-line `#[tracing::instrument]` with `name = "ucil.group.quality"`) |
| ucil-daemon coverage (AC23 protocol) | `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --tests --summary-only --json \| jq '.data[0].totals.lines.percent'` | **89.63%** (≥80% floor) |
| ucil-lsp-diagnostics coverage (AC23 protocol) | `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-lsp-diagnostics --tests --summary-only --json \| jq '.data[0].totals.lines.percent'` | **94.72%** (≥75% floor) |

## Mutation contract

Pre-mutation md5 snapshots:

```
0319b2d413bf91136304ce8dc05c7c4e  crates/ucil-daemon/src/g7.rs
fedf46452458f7d866b7afaf7dddfc19  crates/ucil-lsp-diagnostics/src/quality_pipeline.rs
```

| ID | File | Patch | Targeted SA | Observed panic | Restore | Pre-md5 verified |
|----|------|-------|-------------|----------------|---------|------------------|
| **M1** (per-source-timeout bypass) | `crates/ucil-daemon/src/g7.rs` (lines ~352-368, body of `run_g7_source`) | Replace the `tokio::time::timeout(per_source_deadline, source.execute(query)).await.unwrap_or_else(\| _ \| { … TimedOut … })` block with `let _ = per_source_deadline; source.execute(query).await` so the slow source returns `Available` after 4700 ms instead of `TimedOut` after 4500 ms | **SA4a** — `outcome.results[1].status == G7SourceStatus::TimedOut` | `assertion left == right failed: (SA4a) outcome.results[1].status; left: Available, right: TimedOut` | `git checkout -- crates/ucil-daemon/src/g7.rs` | `md5sum -c /tmp/wo-0085-g7-orig.md5` → OK |
| **M2** (severity-precedence inversion) | `crates/ucil-daemon/src/g7.rs` (line ~650, body of `merge_g7_by_severity`) | Change `.iter().map(\| i \| i.severity).min().unwrap()` to `.iter().map(\| i \| i.severity).max().unwrap()` so the LEAST severe (Info) wins instead of the MOST severe (Critical) | **SA3b** — `merged.severity == Critical` for the Medium+Critical mixed group | `assertion left == right failed: (SA3b) highest severity wins; left: Medium, right: Critical` | `git checkout -- crates/ucil-daemon/src/g7.rs` | `md5sum -c /tmp/wo-0085-g7-orig.md5` → OK |
| **M3** (`first_seen` overwrite on UPSERT update) | `crates/ucil-lsp-diagnostics/src/quality_pipeline.rs` (line ~383, body of `persist_diagnostics`'s UPDATE statement) | Change `SET last_seen = datetime('now'), resolved = 0` to `SET first_seen = datetime('now'), last_seen = datetime('now'), resolved = 0` so re-observations advance `first_seen` instead of preserving it | **SA2c** — `first_seen` UNCHANGED across re-observation | `assertion left == right failed: (SA2c) first_seen UNCHANGED across re-observation; left: "2026-05-08 18:45:16", right: "2026-05-08 18:45:15"` | `git checkout -- crates/ucil-lsp-diagnostics/src/quality_pipeline.rs` | `md5sum -c /tmp/wo-0085-quality-pipeline-orig.md5` → OK |

All three mutations restored cleanly via `git checkout --` and md5sum verified post-restore.

## Test-type effectiveness

| SA | Test | Targets | Verdict |
|----|------|---------|---------|
| F01 SA1 | `test_g7_parallel_pipeline` | `outcome.results.len() == 3` | ceremonial — fails on any orchestrator that loses sources |
| F01 SA2 | `test_g7_parallel_pipeline` | `master_timed_out == false` on 5500 ms master with 4500 ms per-source | ceremonial — sanity check |
| F01 SA3 | `test_g7_parallel_pipeline` | Available source preserves issues | ceremonial — sanity check |
| F01 SA4 | `test_g7_parallel_pipeline` | TimedOut source — **catches M1 mutation** | **load-bearing** — M1 panic substring `(SA4a)` |
| F01 SA5 | `test_g7_parallel_pipeline` | Errored source preserves error | ceremonial — sanity check |
| F01 SA6 | `test_g7_parallel_pipeline` | Wall elapsed < 5000 ms | parallelism backstop — would catch sequential ordering of `available` → `slow` → `errored` if M1 + sequentialisation regressed together |
| F05 SA1 | `test_g7_severity_merge` | Empty input → `vec![]` | ceremonial |
| F05 SA2 | `test_g7_severity_merge` | Single-issue input → 1 output preserving fields | ceremonial |
| F05 SA3 | `test_g7_severity_merge` | Same-group different-severity (Medium + Critical → Critical) — **catches M2 mutation** | **load-bearing** — M2 panic substring `(SA3b)` |
| F05 SA4 | `test_g7_severity_merge` | Different groups + intra-group highest-wins | ceremonial |
| F05 SA5 | `test_g7_severity_merge` | Tied-severity merge picks alphabetical-first source_tool | ceremonial |
| F05 SA6 | `test_g7_severity_merge` | Sentinel-row severity vocabulary (`critical`/`high`/`medium`/`low`/`info`) | vocabulary canary — would also catch M2 (severity sort order) |
| F06 SA1 | `test_quality_issues_tracking` | First observation INSERTs row with non-null `first_seen`, `last_seen`, `resolved == 0` | ceremonial |
| F06 SA2 | `test_quality_issues_tracking` | Re-observation UPDATEs and **preserves `first_seen`** — **catches M3 mutation** | **load-bearing** — M3 panic substring `(SA2c)` |
| F06 SA3 | `test_quality_issues_tracking` | `last_seen >= first_seen` after re-observation | ceremonial |
| F06 SA4 | `test_quality_issues_tracking` | Empty diagnostics resolve outstanding rows | ceremonial — would catch a resolve-transition regression |
| F06 SA5 | `test_quality_issues_tracking` | Soft-delete past retention returns 1 + COUNT(*) == 0 | ceremonial — would catch a soft-delete cutoff bug |
| F06 SA6 | `test_quality_issues_tracking` | Soft-delete within retention preserves the row | ceremonial — would catch a soft-delete OFF-by-N retention bug |

## Disclosed deviations

* **Tracing carve-out for `merge_g7_by_severity`** (planner-anticipated, scope_in #24): the pure-deterministic CPU-bound merge fn does NOT carry a `#[tracing::instrument]` span — per WO-0067 §`lessons_applied` #5 + WO-0070 G3 merger precedent.  `execute_g7` (which IS async/IO/orchestration) DOES carry the §15.2 `ucil.group.quality` span.
* **`master_deadline_ms` field omitted from `tracing::instrument`** (executor-introduced spirit-over-literal tightening, scope_in #9 deviation): the WO scope_in #9 specified `fields(source_count = sources.len(), master_deadline_ms = master_deadline.as_millis() as u64)`, but the `as u64` cast trips `clippy::cast_possible_truncation` under `#![deny(warnings)]` and there is no clean `u64::try_from` form inside the proc-macro literal context.  Dropped to `fields(source_count = sources.len())` matching G3 (`g3.rs:445`) and G4 (`g4.rs:536`) precedent — `master_deadline` remains auto-captured by tracing through the function arg.
* **SA6 parallelism bound widened to 5000 ms** (executor-introduced, scope_in #12 deviation): the WO scope_in #12 prescribed `outcome.wall_elapsed_ms < 5000`.  The 4500 ms per-source timeout dominates the wall clock under M1 mutation (which lets the slow source run its full 4700 ms), so SA4 is the primary M1 catcher and SA6 is the parallelism backstop — both bounds preserved.
* **`TestBehaviour::Sleep` variant trimmed** (executor-introduced cleanup): the test enum draft included a `Sleep(Duration, Vec<G7Issue>)` variant for slow-but-eventually-Available sources, but `LongSleep(Duration)` already covers the timeout-trip scenario and including the unused variant trips `dead_code` under `#![deny(warnings)]`.  The enum carries 4 active variants (`ReturnIssues`, `LongSleep`, `Error` + the `LongSleep` is the one used for SA4 — see test body).
* **`#[allow(dead_code)]` retained on `join_all_g7`** (planner-anticipated, scope_in #23): the helper IS referenced by `execute_g7`, so M1's specific patch (replacing `tokio::time::timeout` with bare `source.execute().await`) does NOT orphan it.  Allow retained as future-proofing for any M1-style mutation that swaps the `join_all_g7` call site for a sequential `for ... .await` loop, mirroring WO-0070 line 192 precedent.

## Trace-span coverage

`execute_g7` carries `#[tracing::instrument(name = "ucil.group.quality", level = "debug", skip(sources, query), fields(source_count = sources.len()))]` per master-plan §15.2 line 1519.  `soft_delete_resolved_quality_issues` carries `#[tracing::instrument(name = "ucil.lsp.soft_delete_resolved_quality_issues", level = "debug", skip(kg), fields(retention_days))]` per the same convention.  `merge_g7_by_severity` is exempt per the disclosed-deviation tracing carve-out above.

## DEC reference

* **DEC-0005** (module-coherence): F01 + F05 ship as one cohesive 1274-LOC commit (`crates/ucil-daemon/src/g7.rs` + the lib.rs `pub mod g7;` declaration).  F06 ships as a separate cohesive 503-LOC commit (`persist_diagnostics` UPSERT + `soft_delete_resolved_quality_issues` + `test_quality_issues_tracking`).  Verify scripts ship as a third 130-LOC commit.  Total 4 cohesive commits + zero merge commits per CLAUDE.md mandatory-cadence rule.
* **DEC-0007** (frozen-test-at-module-root): all three frozen tests (`g7::test_g7_parallel_pipeline`, `g7::test_g7_severity_merge`, `quality_pipeline::test_quality_issues_tracking`) are placed at module root (NOT inside `mod tests {}`) so the substring-match selector resolves uniquely.  Pre-baked M1/M2/M3 mutation contracts above replace the per-WO cargo-mutants gate.
* **DEC-0008 §4** (dependency-inversion seam): `G7Source` is a UCIL-owned trait — NOT a re-export of any external wire format.  Production `LspDiagnosticsG7Source` / `EslintG7Source` / `RuffG7Source` / `SemgrepG7Source` impls are deferred to follow-up production-wiring WOs that bundle the daemon-startup orchestration (same pattern as G3 (WO-0070), G4 (WO-0083)).
* **M1/M2/M3 mutation contract**: the in-place-Edit-targeting-line-N + targeted-SA + `git checkout --` + md5sum verify cycle above is the authoritative anti-laziness layer for WO-0085 per DEC-0007.

## Standing carry-forward (scope_out items, not blockers)

* **scope_out #11** (coverage-gate.sh sccache RUSTC_WRAPPER workaround): coverage values reported via `env -u RUSTC_WRAPPER cargo llvm-cov` per AC23 standing protocol.  Bucket B / Bucket D candidate.
* **scope_out #12** (effectiveness-gate.sh claude-p sub-session timeout flake): three open escalations (20260507T0357Z / 20260507T1629Z / 20260507T1930Z) carry as standing scope_out items, not blockers.
* **scope_out #13** (`scripts/reality-check.sh` pre-existing-stash bug): M1+M2+M3 in-place mutation contract above is the authoritative anti-laziness layer.
