# WO-0073 — ready for review

* **Branch**: `feat/WO-0073-g4-architecture-parallel-query`
* **Final commit sha**: `5040685` (`feat(verify): add scripts/verify/P3-W9-F09.sh ...`)
* **Feature**: `P3-W9-F09` — G4 (Architecture) parallel-query
* **Phase / Week**: 3 / 9

## What I verified locally

* `cargo build -p ucil-daemon` — clean compile (debug).
* `cargo build -p ucil-daemon --release` — clean compile (release).
* `cargo test -p ucil-daemon executor::test_g4_architecture_query --no-fail-fast` —
  `test executor::test_g4_architecture_query ... ok` (4.80 s).
* `cargo test -p ucil-daemon --no-fail-fast` — full crate green
  (164 unit + 25 doctest + integration green; no regression on existing tests).
* `cargo test -p ucil-core --no-fail-fast` — full crate green (44 + 7 + 9 + 2 …; no regression).
* `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings.
* `cargo fmt --all --check` — zero diff.
* `bash scripts/verify/P3-W9-F09.sh` — `[PASS] P3-W9-F09: G4 architecture parallel-query frozen test green`.
* `grep -niE 'mock|fake|stub' crates/ucil-daemon/src/g4.rs scripts/verify/P3-W9-F09.sh`
  — empty (AC17 word-ban).
* `git log feat/WO-0073-g4-architecture-parallel-query ^main --merges | wc -l` — 0 (AC26).
* `git diff main -- <forbidden_paths>` — 0 lines (AC20 sibling-stable).
* `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --summary-only --json |
   jq '.data[0].totals.lines.percent'` — `89.66` (≥80 % AC24 floor); g4.rs
  alone reports 94.39 % line coverage.
* AC18: 66 `(SA*)` matches in `crates/ucil-daemon/src/executor.rs`
  (G1 + G3 + G4 combined; ≥7 required).
* AC19: 0 anti-laziness markers (`todo!()`, `unimplemented!()`, `#[ignore]`)
  in g4.rs / executor.rs changed regions.
* AC30: `TODO(P3-W10-F14 / future)` ground-truth-on-conflict sentinel
  comment present in `merge_g4_dependency_union` per scope_in #22.

## M1/M2/M3 mutation contract (verifier-applicable)

The verifier applies each mutation in isolation via `Edit`, runs the
targeted selector `cargo test -p ucil-daemon
executor::test_g4_architecture_query`, observes the SA-tagged panic,
restores via `git checkout -- crates/ucil-daemon/src/g4.rs`, then
proceeds to the next mutation.  Pre-mutation md5sum:
`06d821601b81c12e162aa602d7815085  crates/ucil-daemon/src/g4.rs`.

### M1 — Sequential execution

* **File**: `crates/ucil-daemon/src/g4.rs`
* **Site (line 573)**:
  ```rust
  let outer = tokio::time::timeout(master_deadline, join_all_g4(futures)).await;
  ```
* **Replace with**:
  ```rust
  let outer = tokio::time::timeout(master_deadline, async {
      let mut outputs = Vec::with_capacity(futures.len());
      for fut in futures {
          outputs.push(fut.await);
      }
      outputs
  })
  .await;
  ```
* **Selector**: `cargo test -p ucil-daemon executor::test_g4_architecture_query --no-fail-fast`
* **Expected panic** (verified locally):
  `(SA1) parallel wall < 500 ms; left: 603, right: 500 (proves serial 3x200=600 ms did NOT happen → parallelism confirmed)`
* **Restore**: `git checkout -- crates/ucil-daemon/src/g4.rs`
* **Notes**:
  * The `join_all_g4` helper carries `#[allow(dead_code)]` so the
    sequential-await mutation compiles cleanly under `#![deny(warnings)]`
    (per WO-0070 lessons).

### M2 — Inverted edge dedup

* **File**: `crates/ucil-daemon/src/g4.rs`
* **Site (line 737)**:
  ```rust
  ue.edge.source == edge.source
  ```
* **Replace with**:
  ```rust
  ue.edge.source != edge.source
  ```
* **Selector**: `cargo test -p ucil-daemon executor::test_g4_architecture_query --no-fail-fast`
* **Expected panic** (verified locally):
  `(SA2) unified_edges.len() == 2; left: 3, right: 2 (foo→bar deduped, bar→baz unique)`
* **Restore**: `git checkout -- crates/ucil-daemon/src/g4.rs`
* **Notes**: Under the mutation, `position()`'s predicate never finds
  a matching unified edge → every input edge becomes a fresh
  `unified_edges` entry → dedup is broken.  The `==` → `!=` is the
  cleanest 2-character mutation per scope_in #11.

### M3 — BFS depth off-by-two

* **File**: `crates/ucil-daemon/src/g4.rs`
* **Site (line 860)**:
  ```rust
  let next_depth = current_depth + 1;
  ```
* **Replace with**:
  ```rust
  let next_depth = current_depth + 2;
  ```
* **Selector**: `cargo test -p ucil-daemon executor::test_g4_architecture_query --no-fail-fast`
* **Expected panic** (verified locally):
  `(SA4) BFS depth child1 == 1; left: 2, right: 1`
* **Restore**: `git checkout -- crates/ucil-daemon/src/g4.rs`
* **Notes**: Under the mutation, child1 lands at depth 2 (instead of
  1) at `max_blast_depth=3`; the SA4 depth-check assertion fires
  first since the lookup `by_node_sa4.get("child1")` succeeds (child1
  is still ≤3-reachable).

## AC reconciliation

Every `acceptance_criteria` row in WO-0073 was exercised locally:

| AC | Result | Evidence |
| --- | --- | --- |
| AC01 | ✅ | `test executor::test_g4_architecture_query ... ok` |
| AC02 | ✅ | `test result: ok. 164 passed` (lib unit) + integration green |
| AC03 | ✅ | `test result: ok. 44 passed` (ucil-core lib) |
| AC04 | ✅ | `cargo clippy --workspace --all-targets -- -D warnings` zero warnings |
| AC05 | ✅ | `cargo fmt --all --check` zero diff |
| AC06 | ✅ | `cargo build -p ucil-daemon --release` exit 0 |
| AC07 | ✅ | `bash scripts/verify/P3-W9-F09.sh` `[PASS]` |
| AC08 | ✅ | `rg '^pub async fn test_g4_architecture_query' crates/ucil-daemon/src/executor.rs` matches |
| AC09 | ✅ | `rg '^pub mod g4;' crates/ucil-daemon/src/lib.rs` matches |
| AC10 | ✅ | `rg 'pub trait G4Source' crates/ucil-daemon/src/g4.rs` matches |
| AC11 | ✅ | `rg 'pub async fn execute_g4' crates/ucil-daemon/src/g4.rs` matches |
| AC12 | ✅ | `rg 'pub fn merge_g4_dependency_union' crates/ucil-daemon/src/g4.rs` matches |
| AC13 | ✅ | `rg 'pub const G4_MASTER_DEADLINE' crates/ucil-daemon/src/g4.rs` matches |
| AC14 | ✅ | `rg 'pub const G4_PER_SOURCE_DEADLINE' crates/ucil-daemon/src/g4.rs` matches |
| AC15 | ✅ | `rg '#\[tracing::instrument' crates/ucil-daemon/src/g4.rs` matches |
| AC16 | ✅ | `rg '#\[allow\(dead_code\)\]' crates/ucil-daemon/src/g4.rs` matches |
| AC17 | ✅ | `rg -i 'mock\|fake\|stub' crates/ucil-daemon/src/g4.rs scripts/verify/P3-W9-F09.sh` exit 1 |
| AC18 | ✅ | 66 `(SA*)` matches in executor.rs (≥7 required) |
| AC19 | ✅ | No `todo!()`, `unimplemented!()`, `#[ignore]` in changed regions |
| AC20 | ✅ | `git diff main -- <forbidden sibling paths>` 0 lines |
| AC21 | ✅ | M1 SA1 panic verified locally (see "M1 — Sequential" above) |
| AC22 | ✅ | M2 SA2 panic verified locally (see "M2 — Inverted dedup" above) |
| AC23 | ✅ | M3 SA4 panic verified locally (see "M3 — BFS depth +2" above) |
| AC24 | ✅ | `env -u RUSTC_WRAPPER cargo llvm-cov ucil-daemon` line coverage 89.66 % (≥80 % floor) |
| AC25 | ⚠️ disclosed | scope_in #17 standing protocol (sccache `coverage-gate.sh` interaction) + scope_in #18 effectiveness-gate flake carve-out — verifier may re-run from a clean session. AC24 substantive measurement above is the protocol substitute. |
| AC26 | ✅ | `git log feat/WO-0073-g4-architecture-parallel-query ^main --merges \| wc -l` = 0 |
| AC27 | ✅ | Touched files: `crates/ucil-daemon/src/{g4.rs,lib.rs,executor.rs}`, `scripts/verify/P3-W9-F09.sh`. Zero forbidden_paths touched. |
| AC28 | ✅ | Every feat/test commit body carries `Phase: 3` + `Feature: P3-W9-F09` + `Work-order: WO-0073` trailers |
| AC29 | ✅ | `git rev-parse origin/feat/WO-0073-g4-architecture-parallel-query` resolves to local HEAD (5040685) |
| AC30 | ✅ | `rg 'TODO\(P3-W10-F14\|TODO.*GroundTruth' crates/ucil-daemon/src/g4.rs` matches (sentinel comment in `merge_g4_dependency_union` per scope_in #22) |

## Standing-protocol carve-outs

* AC25 — phase-3 gate-script standing-protocol substantive measurement
  per scope_in #17 (28 WOs deep) + scope_in #18 (effectiveness-gate
  phase-1/phase-2 flake escalations).  Continue scope_out wording
  pending Bucket-D/-B harness improvement.

## Lessons applied (citations in commit bodies)

1. WO-0067/0068/0069/0070/0072 pre-baked M1/M2/M3 mutation contract.
2. WO-0068/0070 frozen-test at module root (NOT under `mod tests {}`).
3. WO-0068/0070 per-source deadline as unconditional `const`.
4. WO-0067 `#[tracing::instrument]` on async/IO orchestration
   (`ucil.group.architecture` span per master-plan §15.2).
5. WO-0068/0070 alphabetical `pub mod` / `pub use` placement.
6. WO-0067/0068/0069/0070 DEC-0007 SA-numbered panic bodies.
7. WO-0069 word-ban grep production-side; test impls
   (`TestG4Source`) under `#[cfg(test)]` exempt.
8. WO-0068/0070 no mocks of MCP/JSON-RPC/subprocess; UCIL-owned
   `G4Source` DI seam per `DEC-0008` §4; production wiring deferred.
9. WO-0070 pre-emptive `#[allow(dead_code)]` on `join_all_g4` so M1
   compiles cleanly under `#![deny(warnings)]`.
10. WO-0070 SA1 ceiling 500 ms (NOT 700) — parallelism dual-bound
    `200 × 3/2 + 200 = 500` reliably traps serial 3×200 = 600 ms.
11. WO-0070 multi-thread tokio test flavor for parallelism
    observability.
12. WO-0070 AC25 reworded to `git log feat ^main --merges` = 0
    (workflow-timing tolerant).

## Source map (touched files)

* `crates/ucil-daemon/src/g4.rs` — NEW; 898 LOC including module-level
  rustdoc.  Public surface: `G4Source` trait, `G4Query`,
  `G4DependencyEdge`, `G4EdgeKind` (`{Import, Call, Inherits, Implements,
  Other(String)}`), `G4EdgeOrigin` (`{Inferred, GroundTruth}`),
  `G4SourceOutput`, `G4SourceStatus`, `G4Outcome`, `G4UnifiedEdge`,
  `G4BlastRadiusEntry`, `G4UnionOutcome`, `execute_g4`,
  `merge_g4_dependency_union`, `G4_MASTER_DEADLINE`,
  `G4_PER_SOURCE_DEADLINE`.
* `crates/ucil-daemon/src/lib.rs` — `pub mod g4;` and matching
  `#[rustfmt::skip] pub use g4::{...}` block (alphabetically between
  `g3` and `lancedb_indexer`).
* `crates/ucil-daemon/src/executor.rs` — appended frozen
  `test_g4_architecture_query` at module root (NOT under
  `mod tests {}`) per DEC-0007.  8 SA-tagged sub-assertions
  (SA1..SA8) covering parallelism, edge-union dedup, disjoint-edges,
  BFS-depth, multiplicative coupling, partial-error, partial-timeout,
  master-deadline trip.  Local `TestG4Source` impl per `DEC-0008` §4.
* `scripts/verify/P3-W9-F09.sh` — NEW.  Rename-drift guards on
  frozen public symbols + frozen-selector grep + cargo-test summary
  regex.  No subprocess, no env-gated short-circuits.
