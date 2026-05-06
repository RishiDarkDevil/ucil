---
title: WO-0053 (P2-W7-F09) feat-branch never merged to main — blocks P2-W8-F04 / F07 / F08
created: 2026-05-07T07:50:00Z
created_by: planner
phase: 2
week: 8
blocks_loop: false
severity: harness-config
requires_planner_action: false
requires_user_action: true
related_decisions:
  - DEC-0016-wo-0053-feat-branch-not-merged.md
related_escalations:
  - 20260505-2014-wo-WO-0053-attempts-exhausted.md
  - 20260505-2019-wo-WO-0053-attempts-exhausted.md
  - 20260505-1801-merge-failure-WO-0052.md
related_features:
  - P2-W8-F04
  - P2-W8-F07
  - P2-W8-F08
  - P2-W7-F09  (already passes=true but verifying commit lives only on the orphan branch)
related_workorders:
  - WO-0053  (orphan)
  - WO-0063  (this planner cycle — P2-W7-F06; UNBLOCKED, NOT affected)
---

## Summary

Per **DEC-0016 §Consequences**, before any planner emits a work-order for
`P2-W8-F04`, `P2-W8-F07`, or `P2-W8-F08`, a fresh escalation MUST be filed
requesting human merge of `feat/WO-0053-lancedb-per-branch` into `main`.
This is that escalation.

WO-0053 verified `P2-W7-F09` (LanceDB per-branch vector store lifecycle) on
commit `dfd0772` — but that commit lives only on
`feat/WO-0053-lancedb-per-branch`, not on `main`. The implementation file
`crates/ucil-daemon/src/branch_manager.rs` (with `BranchManager`,
`create_branch_table`, `archive_branch_table`, etc.) is therefore **absent
from main** and consumer features cannot compile.

```
$ git show main:crates/ucil-daemon/src/branch_manager.rs
fatal: path 'crates/ucil-daemon/src/branch_manager.rs' does not exist in 'main'
$ git branch --contains dfd0772
* feat/WO-0053-lancedb-per-branch
  remotes/origin/feat/WO-0053-lancedb-per-branch
```

The harness-contract triage that closed
`20260505-2014-wo-WO-0053-attempts-exhausted.md` and `-2019-...` was
correct under the rules-as-written (oracle = `feature-list.json`, which
shows `P2-W7-F09: passes=true`), but did not catch the orphan-branch shape.

## Impact on remaining Phase 2 features

| Feature | Description | Depends on `BranchManager`? | Blocked? |
|---------|-------------|------------------------------|----------|
| **P2-W7-F06** | `search_code` G2 fused (Probe + ripgrep + LanceDB) | NO — `LancedbProvider` does a filesystem-existence check on `StorageLayout::branch_vectors_dir(...)/code_chunks.lance/` (per DEC-0015 D3) and returns empty hits. `StorageLayout::branch_vectors_dir` lives in `crates/ucil-daemon/src/storage.rs:230` ON MAIN. | **NO — WO-0063 (this cycle) proceeds.** |
| **P2-W8-F04** | LanceDB background chunk indexing | YES — needs `BranchManager::create_branch_table` to materialise the per-branch table before incremental indexing can write rows. | **YES.** |
| **P2-W8-F07** | Vector query latency benchmark p95 < 100 ms | YES — transitive via F04 (the bench runs against pre-indexed data). | **YES.** |
| **P2-W8-F08** | `find_similar` MCP tool | YES — consumes per-branch LanceDB tables via `BranchManager` accessor. | **YES.** |

## Requested action (user / human reviewer)

**Option A — clean merge (recommended):**

```bash
cd /home/rishidarkdevil/Desktop/ucil
git checkout main
git fetch origin feat/WO-0053-lancedb-per-branch
git merge --no-ff origin/feat/WO-0053-lancedb-per-branch -m "merge: WO-0053 lancedb-per-branch (orphan → main) — DEC-0016 closure"
# expected conflict: ucil-build/feature-list.json (per WO-0052 precedent)
#   resolution: keep main's `passes` values; the WO-0053 branch's flip for
#               P2-W7-F09 already happened on the trailing commit `dfd0772`,
#               which is what the merge brings in.
# verify:
#   cargo build -p ucil-daemon                # should compile branch_manager.rs
#   cargo test -p ucil-daemon branch_manager::test_lancedb_per_branch
git push origin main
```

After the merge:
1. Append a `## Closed` section to `ucil-build/decisions/DEC-0016-wo-0053-feat-branch-not-merged.md` citing the merge commit SHA (per DEC-0016 §Revisit trigger).
2. Mark this escalation `resolved: true` with the merge commit SHA.
3. The autonomous loop's next planner cycle can then emit the F04 work-order without the orphan-branch concern.

**Option B — bundled merge in a follow-on WO:**

Per DEC-0016 §Decision Option 1, the F04 work-order can `git merge --no-ff
origin/feat/WO-0053-lancedb-per-branch` inside its OWN worktree branch
before adding new feature code. This requires explicit user authorisation
in the WO's `created_by` justification (DEC-0016 §Decision: "Option 1 is
only acceptable if the user has explicitly authorised the bundled merge").

Default per DEC-0016: **Option A.**

## Why this does NOT block WO-0063 / P2-W7-F06

WO-0063 (this planner cycle, refresh of stale WO-0057) implements the
G2-fused `search_code` MCP tool. Its `LancedbProvider` is filesystem-only
per DEC-0015 D3 — it calls `tokio::fs::try_exists(...)` on the per-branch
LanceDB table directory and returns empty `G2SourceResults` if the
directory is absent or empty. No `BranchManager` import. No `lancedb`
crate dependency added. The forward-compat path (when F04 lands) is
documented inline in the impl rustdoc.

`StorageLayout::branch_vectors_dir(&self) -> PathBuf` is defined in
`crates/ucil-daemon/src/storage.rs:230` on `main` and is fully reachable
from WO-0063's worktree. Verified pre-emit by reading `git show
main:crates/ucil-daemon/src/storage.rs`.

## What the autonomous loop should do until human merge happens

1. The executor picks up WO-0063 (P2-W7-F06) next iteration.
2. After WO-0063 merges, Phase 2 stands at 22/25 features passing.
3. Phase 2 cannot reach `gate(2) = green` until F04 / F07 / F08 also pass,
   which requires the orphan-branch merge above.
4. Until the merge, `scripts/run-phase.sh` will halt rather than emit
   F04/F07/F08 WOs (DEC-0016 §Consequences makes this prerequisite
   mechanical: any planner attempting F04/F07/F08 must first observe a
   `## Closed` section on DEC-0016, which is absent until the human merge).

## References

- `ucil-build/decisions/DEC-0016-wo-0053-feat-branch-not-merged.md`
- `ucil-build/work-orders/0053-lancedb-per-branch.json`
- `ucil-build/escalations/20260505-2014-wo-WO-0053-attempts-exhausted.md`
- `ucil-build/escalations/20260505-2019-wo-WO-0053-attempts-exhausted.md`
- `ucil-build/escalations/20260505-1801-merge-failure-WO-0052.md` (parallel
  WO-0052 feat-branch merge; shows the `feature-list.json` conflict shape
  to expect when merging the orphan branch).
- Phase-log lessons WO-0062 line 744 (orphan-branch carryover note).

resolved: false
