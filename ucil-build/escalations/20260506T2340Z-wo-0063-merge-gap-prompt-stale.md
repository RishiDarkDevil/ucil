---
severity: harness-config
blocks_loop: false
requires_planner_action: false
agent: executor
work_order: WO-0063
feature_ids: [P2-W7-F06]
---

# WO-0063 — executor invoked against stale "rejection" prompt; work already PASSED + feature flipped, but feature-branch never merged into `main`

## Summary

The orchestrator fired an executor turn for WO-0063 with a prompt that
claimed "A PRIOR verifier attempt rejected your work" and instructed
remediation against:

- `ucil-build/rejections/WO-0063.md` — **does not exist**
- `ucil-build/verification-reports/root-cause-WO-0063.md` — **does not exist**

The premise is false. The actual ground-truth state on `main` HEAD
`3430f1755266d167c8f3775ae0c92009334f2784`:

| Fact | Evidence |
|------|----------|
| Verifier already PASSED WO-0063 | `ucil-build/verification-reports/WO-0063.md` line 7: `**Verdict**: **PASS**` |
| P2-W7-F06 already `passes=true` on `main` | `jq '.features[] \| select(.id == "P2-W7-F06")' ucil-build/feature-list.json` returns `passes: true, last_verified_ts: 2026-05-06T23:36:09Z, last_verified_by: verifier-dc14625e-..., last_verified_commit: a12e97ff3...` |
| Critic verdict was CLEAN | `ucil-build/critic-reports/WO-0063.md` line 15: `**Verdict**: **CLEAN with warnings**` |
| Recent commits confirm | `3430f17 chore(verifier): WO-0063 PASS — flip P2-W7-F06`, `39f0927 chore(critic): WO-0063 report — CLEAN`, `1daf60b chore(planner): emit WO-0063` |
| All 6 implementation commits exist on the feature branch | `git log --oneline main..feat/WO-0063-search-code-g2-fused-refresh` returns 6 commits (`7dda6c4`, `48f75df`, `aef7ba4`, `db9882c`, `9b0368c`, `a12e97f`) |
| No rejection file | `ls ucil-build/rejections/ \| grep -i 0063` returns empty |
| No RCF file | `ls ucil-build/verification-reports/root-cause-WO-0063.md` → `No such file or directory` |

## The actual gap: feat-branch never merged into `main`

This is the same pattern as the open
`ucil-build/escalations/20260505-1801-merge-failure-WO-0052.md` and the
WO-0049 / WO-0053 merge-failure series.

- `crates/ucil-daemon/src/g2_search.rs` exists on
  `feat/WO-0063-search-code-g2-fused-refresh` but **not on `main`**:

  ```
  $ ls crates/ucil-daemon/src/g2_search.rs   # in main checkout
  ls: cannot access ...: No such file or directory

  $ git log --oneline -5 main -- crates/ucil-daemon/src/g2_search.rs
  (empty)

  $ git branch --merged main | grep WO-0063
  (empty)

  $ git log --oneline main..feat/WO-0063-search-code-g2-fused-refresh | wc -l
  6
  ```

- The feature was flipped against
  `last_verified_commit: a12e97ff3e96f9641cad8ce08e1f9b4a9827a748`, which
  exists only on `refs/heads/feat/WO-0063-search-code-g2-fused-refresh`.
- The verification report itself is committed on `main` (commit
  `3430f17`) and references the feature-branch commit as the verified
  HEAD; that admin commit landed without the corresponding
  fast-forward / merge of the implementation commits.

So the harness believes P2-W7-F06 is "done" but the daemon binary built
from `main` does **not** contain the `g2_search.rs` module, the
`G2SourceProvider` trait, the `with_g2_sources` builder, the
`PluginManager::run_tools_call` extension, the new acceptance tests, or
the `scripts/verify/P2-W7-F06.sh` script.

## Why I did not re-implement

Per root `CLAUDE.md` anti-laziness contract and the work-order discipline:

1. **The work is already implemented and verified** — re-running the
   executor would either (a) duplicate already-pushed commits on the
   feature branch (would fail because the branch is up-to-date with
   origin and the changes already exist), or (b) silently produce a
   no-op. Either outcome wastes a verifier cycle.
2. **The marker-file path the prompt expects
   (`ucil-build/work-orders/0063-search-code-g2-fused-refresh-ready-for-review.md`)
   does not match the path the original execution used
   (`ucil-build/work-orders/0063-ready-for-review.md`)** — the original
   marker is committed on the feature branch at `a12e97f`. Writing a
   second marker under a new name would not constitute remediation; it
   would just confuse the next reader.
3. **No rejection or RCF artefact exists to "apply remediation from"** —
   the prompt instructs me to read two files that do not exist.
   Pretending to remediate against a non-existent diagnosis is a
   fabrication risk.
4. **`feature-list.json` is already `passes=true` for P2-W7-F06** — even
   if the executor produced new code, the verifier would have nothing
   to flip (whitelist-protected field already at terminal value).

## Worktree cleanup performed

The worktree at `/home/rishidarkdevil/Desktop/ucil-wt/WO-0063` had a
stale uncommitted modification to
`ucil-build/verification-reports/coverage-ucil-daemon.md` (a regenerated
coverage report from a prior verifier run; not part of any work-order
scope). Restored to HEAD via
`git -C /home/rishidarkdevil/Desktop/ucil-wt/WO-0063 checkout --
ucil-build/verification-reports/coverage-ucil-daemon.md`. Worktree is
now clean and matches `origin/feat/WO-0063-search-code-g2-fused-refresh`.

## Recommended remediation (Bucket B — harness-config)

Either of the following short fixes resolves the gap. **Both are
harness-side; none touches UCIL source.**

### Option A — fast-forward merge the feature branch into `main` (preferred)

```bash
git checkout main
git merge --ff-only feat/WO-0063-search-code-g2-fused-refresh
git push origin main
```

This requires `main` to be at-or-behind the feature branch's
merge-base, which it is (the feature branch is 6 commits ahead of
`main` and `main` has no diverging commits beyond the
verifier/critic/planner admin commits, which do not touch any of the
six paths in WO-0063's allow-list).

The fast-forward will introduce on `main`:
- `crates/ucil-daemon/src/g2_search.rs` (NEW, +707 LOC)
- `crates/ucil-daemon/src/lib.rs` (+16 LOC)
- `crates/ucil-daemon/src/plugin_manager.rs` (+220 LOC)
- `crates/ucil-daemon/src/server.rs` (+621 / -68 LOC)
- `scripts/verify/P2-W7-F06.sh` (NEW, +108 LOC, mode 100755)
- `ucil-build/work-orders/0063-ready-for-review.md` (NEW, +264 LOC)

After merge, gate-check.sh phase 2's
`scripts/verify/P2-W7-F06.sh` will pass against `main`, the daemon
binary will contain the G2-fused `search_code` handler, and the WO-0063
work is genuinely complete on the trunk.

### Option B — investigate why the verifier flipped without merging

Per the verifier protocol and `scripts/flip-feature.sh`, the verifier
should fast-forward merge the feature branch onto `main` BEFORE flipping
`passes=true`. The fact that this gap exists for WO-0049 / WO-0052 /
WO-0053 / and now WO-0063 suggests a structural bug in
`scripts/flip-feature.sh` or the verifier protocol. A
`harness-fixer` agent invocation on `scripts/flip-feature.sh` would be
appropriate. (The four-WO pattern argues this is harness-side, not
agent-side, since four independent verifier sessions all hit the same
behaviour.)

### Option C — re-emit a planner work-order to perform the merge

If neither of the above is in scope for triage, the planner can emit a
short merge-recovery work-order of the same shape as a hypothetical
`WO-XXXX-merge-recovery-WO-0063.json`, mirroring the pattern that would
unstick WO-0049 / WO-0052 / WO-0053. The merge-recovery WO would have
empty `feature_ids` (P2-W7-F06 is already flipped) and a single
`scope_in` step: "fast-forward merge feat/WO-0063-search-code-g2-fused-refresh into main; push".

## What I am NOT doing in this turn

- **No source-code edits.** The work is done, verified, and pushed to
  the feature branch. Touching source on `main` outside of a planner
  work-order is forbidden.
- **No `feature-list.json` writes.** P2-W7-F06 is already `passes=true`;
  the verifier already flipped it; touching the field is whitelist-
  protected and would fail the pre-commit hook anyway.
- **No `git merge` of the feature branch into `main`.** That requires
  triage / verifier / human authorisation per the established workflow;
  the executor's role is implementation on the feature branch only.
- **No new ready-for-review marker file.** The original marker is
  committed at `a12e97f` on the feature branch under the name
  `ucil-build/work-orders/0063-ready-for-review.md`. Writing a second
  marker file would not change the merge state.
- **No `git push --force` / `--amend` / `--no-verify`.** Forbidden by
  root `CLAUDE.md`.

## Status to triage

- **Bucket**: B (harness-config, ≤120 LOC fix in `scripts/`)
- **Suggested fix**: Option A (fast-forward merge — zero new code)
- **Severity**: `harness-config` — the gate is structurally correct on
  the feature branch but the trunk binary is missing the
  implementation; the daemon's `search_code` tool on `main` will only
  return the legacy `_meta` shape (no `g2_fused`) until the merge lands.
- **`blocks_loop: false`** — the harness considers P2-W7-F06 done; the
  outer loop will continue. But Phase 2 cannot be honestly shipped
  until `main` actually contains the implementation.

## Cross-references

- `ucil-build/escalations/20260505-1801-merge-failure-WO-0052.md` — same pattern, WO-0052
- `ucil-build/escalations/20260507T0750Z-wo-0053-orphan-branch-blocks-w8-f04-f07-f08.md` — adjacent merge-failure pattern, WO-0053
- `ucil-build/escalations/20260505T0009Z-critic-wo-0049-push-blocked-network-unreachable.md` — WO-0049 push/merge series origin
- `ucil-build/decisions/DEC-0016-wo-0053-feat-branch-not-merged.md` — orphan-branch ADR
- `ucil-build/critic-reports/WO-0063.md` — CLEAN-with-warnings critic verdict
- `ucil-build/verification-reports/WO-0063.md` — PASS verifier verdict
- `ucil-build/work-orders/0063-search-code-g2-fused-refresh.json` — work-order
- `feat/WO-0063-search-code-g2-fused-refresh @ a12e97f` — feature-branch HEAD with all required artefacts

resolved: true
