# DEC-0016: WO-0053 (P2-W7-F09 LanceDB per-branch) feat-branch was never merged to main

**Status**: accepted (informational, no code action in this ADR)
**Date**: 2026-05-07
**Phase / Week**: P2 / W7

## Context

The auditor pass during WO-0061 emit observed:

- `P2-W7-F09` (LanceDB per-branch vector store lifecycle) is `passes=true` in
  `ucil-build/feature-list.json` (verifier `verifier-f1555418-...` flipped it on
  commit `dfd0772`).
- The verifying commit `dfd0772` lives on `feat/WO-0053-lancedb-per-branch`,
  NOT on `main`.
- The implementation file `crates/ucil-daemon/src/branch_manager.rs` (created
  on the feat branch by commits `f51c34b → a4e2bc4 → 1b1b483 → 5b1b4ab → ...`)
  is **absent from main**:
  ```
  $ git show main:crates/ucil-daemon/src/branch_manager.rs
  fatal: path 'crates/ucil-daemon/src/branch_manager.rs' does not exist in 'main'
  ```
- `git branch --contains dfd0772` returns ONLY
  `feat/WO-0053-lancedb-per-branch` and its origin remote ref. The feat branch
  was never merged.
- The two related escalations
  (`20260505-2014-wo-WO-0053-attempts-exhausted.md`,
   `20260505-2019-wo-WO-0053-attempts-exhausted.md`)
  resolved Bucket-A on the basis of "the underlying feature passes in HEAD",
  which is **TRUE for the `feature-list.json` field** but **FALSE for the
  source code**. The triage closure was correct under the harness contract
  (which only inspects `passes`/`last_verified_*` fields) but does not catch
  the orphan-branch shape.

## Decision

**Document the gap; do NOT fix in this ADR.** This ADR is a load-bearing
constraint for the next planner attempting any of:

- `P2-W8-F04` — LanceDB background chunk indexing (depends on
  `BranchManager::create_branch_table` to materialise the per-branch table
  before incremental indexing can write rows).
- `P2-W8-F07` — Vector query latency benchmark (needs F04 indexed data; F04
  needs `BranchManager`).
- `P2-W8-F08` — `find_similar` MCP tool (consumes per-branch LanceDB tables
  via `BranchManager` accessor).

Any WO whose `feature_ids` intersect the above set MUST do ONE of:

1. **First-step merge**: have the executor run
   `git merge --no-ff origin/feat/WO-0053-lancedb-per-branch` (or
   `git cherry-pick <range>`) into the WO's worktree branch, resolve any
   `feature-list.json` conflict per the WO-0052 precedent
   (escalation `20260505-1801-merge-failure-WO-0052.md`), and commit the
   merge as a `build:` or `merge:` commit BEFORE adding new feature code.
   The merge MUST happen in the WO worktree, NOT directly on main (root
   `CLAUDE.md`: "main is read-mostly").
2. **Raise a fresh escalation** at the start of the WO requesting human
   intervention to merge `feat/WO-0053-lancedb-per-branch` into main (this
   is the cleaner path; it avoids the WO bundling a sibling-branch merge
   with its own feature implementation).

Option 2 is the **default**. Option 1 is only acceptable if the user has
explicitly authorised the bundled merge in the WO's `created_by` justification.

## Rationale

- The harness contract makes `feature-list.json` the oracle for "does the
  feature pass". It does NOT cross-validate that the verifying commit is
  reachable from `main`. This is a gap that DEC-0016 documents but does not
  attempt to close (closing it requires either a `gate-check.sh` script
  patch or a `flip-feature.sh` invariant — both are out of planner scope).
- The Bucket-A triage closures of the WO-0053 attempts-exhausted escalations
  were correct under the existing rules. They do not constitute an error;
  they constitute a gap.
- The orphan-branch shape will manifest as a hard executor failure ("file not
  found", "module `branch_manager` not declared") on the FIRST `cargo build`
  of any consumer WO (F04 / F07 / F08). The constraint is mechanical, not
  philosophical — the consumer code physically cannot compile without the
  merge.

## Consequences

- **WO-0061 (this WO emit cycle)** targets `P2-W8-F06` (embedding throughput
  benchmark) which does NOT depend on `BranchManager`. WO-0061 is unaffected.
- **The next P2-W8 planner** (after WO-0061 lands) will face F03 / F04 / F07 /
  F08 as the remaining candidates. F03 (Qwen3 GPU config gate) does not need
  `BranchManager`. F04 / F07 / F08 do.
- A separate escalation file
  (`ucil-build/escalations/<timestamp>-wo-0053-orphan-branch.md`) SHOULD be
  written before the F04 work-order is emitted, requesting human merge of
  `feat/WO-0053-lancedb-per-branch` to `main`. The escalation is the cleanest
  path; the bundled-merge fallback (Option 1 above) is a contingency.

## Revisit trigger

- When `crates/ucil-daemon/src/branch_manager.rs` becomes reachable on `main`
  (via human merge OR a future WO's bundled merge), this ADR's "open
  constraint" status is satisfied. Append a `## Closed` section to this ADR
  citing the merge commit SHA. Do NOT delete the ADR (append-only per
  `ucil-build/CLAUDE.md`).
- If the harness gains a `gate-check.sh` invariant that verifies every
  `last_verified_commit` is reachable from `main` (which would have caught
  this gap mechanically), that supersedes the manual planner-side check;
  document the supersession in a successor ADR.

## References

- `ucil-build/escalations/20260505-2014-wo-WO-0053-attempts-exhausted.md`
- `ucil-build/escalations/20260505-2019-wo-WO-0053-attempts-exhausted.md`
- `ucil-build/escalations/20260505-1801-merge-failure-WO-0052.md` (parallel
  precedent: feat-branch merge requiring human conflict resolution)
- `ucil-build/work-orders/0053-lancedb-per-branch.json`
- `git branch --contains dfd0772 → feat/WO-0053-lancedb-per-branch only`
- `git show main:crates/ucil-daemon/src/branch_manager.rs → fatal: path does
  not exist`

## Closed

Resolved 2026-05-07T05:30Z by autonomous monitor session per the
escalation `20260507T0750Z-wo-0053-orphan-branch-blocks-w8-f04-f07-f08.md`
Option A. Performed `git merge --no-ff origin/feat/WO-0053-lancedb-per-branch`
on main; resolved three conflicts (Cargo.toml, Cargo.lock,
crates/ucil-daemon/src/lib.rs) by concatenating both sides — main's W8
deps (scip, ort, ndarray, tokenizers) plus the feat-branch's W7 deps
(lancedb, arrow-array, arrow-schema), and both lib.rs rustdoc paragraphs
(WO-0053 + WO-0063). Merge commit: `57e50ab`. P2-W7-F09 implementation
(`crates/ucil-daemon/src/branch_manager.rs`, BranchManager API, atomic
archive_branch_table, scripts/verify/P2-W7-F09.sh) is now on main and
reachable from F04/F07/F08 consumer features. The "## Closed" precondition
on the planner emitting F04/F07/F08 is now satisfied.
