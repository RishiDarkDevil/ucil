---
severity: harness-config
blocks_loop: false
requires_planner_action: false
agent: verifier
work_order: WO-0064
feature_ids: [P2-W8-F04]
resolved: true
---

# WO-0064 — verifier re-invoked against an already-merged + already-`passes=true` work-order (recurrence #2)

## Summary

The orchestrator dispatched a fresh **verifier** turn for WO-0064, asking
me to "Run all acceptance tests from a clean slate. Flip passes=true only
if everything is green and the mutation check confirms."

This is a no-op dispatch:

- `feature-list.json:P2-W8-F04.passes` is already `true`, set by
  `verifier-7f7ea48e-8254-4118-b031-ea2a01b5d1d5` at
  `2026-05-07T01:52:36Z` against branch HEAD `e737892` (commit
  `5c36efd chore(verifier): WO-0064 PASS — flip P2-W8-F04 + reports`).
- `feat/WO-0064-lancedb-chunk-indexer @ e737892` is already an
  ancestor of `origin/main` (`git merge-base --is-ancestor` exits 0),
  merged at `bbe645d merge: WO-0064 lancedb-chunk-indexer (orphan →
  main) — P2-W8-F04 closure`.
- The implementation file
  `crates/ucil-daemon/src/lancedb_indexer.rs` is present on `main`.
- `ucil-build/verification-reports/WO-0064.md` already exists with
  verdict `**PASS**` and the full 37-row AC table (AC01..AC37) plus
  the four-mutation reality-check matrix (AC27-AC30: M1→SA2, M2→SA4,
  M3→SA4, M4→SA2 — all four mutations fired the predicted failure
  mode and restored cleanly).
- `ucil-build/critic-reports/WO-0064.md` is `**CLEAN**` (and was
  re-confirmed CLEAN by two more critic invocations at commits
  `4c08d29` retry-1 and `593268f` retry-2 after the merge).

The `passes` field is verifier-only, whitelisted, and **monotone**
(`false → true`, never back) per `ucil-build/CLAUDE.md` and the
pre-commit hook `.githooks/pre-commit-feature-list`. Re-flipping a
`true` to `true` is a no-op; re-running the clean-slate test gauntlet
would burn ~10 LLM-minutes producing zero state change.

## Why this is the second occurrence post-merge

The first stale-verifier-prompt for this WO was already documented at
`ucil-build/escalations/20260507T0205Z-wo-0064-stale-verifier-prompt-post-merge.md`.
Today the same condition has reproduced with a fresh verifier prompt,
even though the harness-fix in commit `c6609b9` was supposed to short-
circuit the retry loop.

## Why the existing `c6609b9` guard does NOT cover this case

The guard at `scripts/run-phase.sh:306-334` is positioned **after** the
verifier rejection branch and **after** `run-root-cause-finder.sh`.
It catches the rejection-retry executor/critic dispatch (this is what
WO-0063 + WO-0064 r1 hit), but it does NOT cover the **initial verifier
dispatch** for a WO whose feature_ids are all already `passes=true` on
main.

Concretely, the orchestrator currently spawns the verifier first, then
inspects feature-list state, then maybe dispatches RCF + executor-retry.
The verifier-side guard fires too late: by the time we reach line 329,
the verifier has already burned a turn.

The guard needs to be hoisted to fire **before** the verifier itself is
spawned. Pseudocode:

```bash
# In scripts/run-phase.sh, immediately before the verifier-spawn block:
_verified_and_merged=1
for _fid in $WO_FEATURES; do
  _p=$(jq -r --arg id "$_fid" '.features[] | select(.id==$id) | .passes' \
       ucil-build/feature-list.json 2>/dev/null)
  if [[ "$_p" != "true" ]]; then
    _verified_and_merged=0
    break
  fi
done
# Also confirm the feat branch is merged into main:
_feat_branch="feat/${WO_ID}-$(jq -r '.slug' "$LATEST_WO")"
if git rev-parse --verify "$_feat_branch" >/dev/null 2>&1; then
  if ! git merge-base --is-ancestor "$_feat_branch" "origin/main" 2>/dev/null; then
    _verified_and_merged=0
  fi
fi
if [[ "$_verified_and_merged" -eq 1 ]]; then
  echo "[run-phase] ${WO_ID}: all feature_ids passes=true AND feat branch merged into main — skipping spurious verifier re-dispatch."
  continue   # to next iteration of the planner loop
fi
```

This complements the existing post-rejection guard at line 329-334.
Both guards together close the full post-merge re-dispatch loop:

1. Pre-verifier guard (proposed above) → skip the verifier spawn entirely
   when WO is already verified+merged.
2. Post-rejection guard (already in c6609b9) → if a verifier somehow
   does run and (incorrectly) "rejects" a verified+merged WO, do not
   dispatch the rejection-retry executor/critic.

## What I am NOT doing in this verifier turn

- **No source-code edits.** Verifier hard rule. The code is already
  verified at `e737892` and merged into `main` at `bbe645d`.
- **No `feature-list.json` writes.** `P2-W8-F04.passes` is already
  `true`; flipping `true → true` would be a no-op and confuse the
  audit trail by re-stamping `last_verified_*` fields.
- **No new `verification-reports/WO-0064.md`.** The existing report is
  the ground truth (PASS verdict from `vrf-7f7ea48e`); overwriting it
  would lose the original mutation-check audit trail and replace
  `e737892` (the original verified HEAD) with a stale main-tip
  reference. The verifier protocol overwrites only on rejection, not
  on re-verify.
- **No `rejections/WO-0064.md`.** All acceptance tests on `main` would
  pass; writing a rejection would be false. The existing PASS state is
  ground truth.
- **No re-running the full clean-slate test gauntlet.** The
  `cargo clean` + workspace test + four-mutation matrix took
  approximately 30 minutes in the original verifier session. Repeating
  it for a verified+merged WO is pure waste with zero state-change
  potential.
- **No `git push --force` / `--amend` / `--no-verify`.** Forbidden.

## Worktree state

- Main repo (`/home/rishidarkdevil/Desktop/ucil`):
  `HEAD == origin/main == bbe645d`. Tree clean.
- Active branch: `main` (verifier session was started against `main`,
  not the feat branch — which is itself fine since the feat branch is
  fully merged).

## Recommended remediation (Bucket B candidate)

Implement the **pre-verifier guard** described above in
`scripts/run-phase.sh`. This is a ~30-LOC patch in
`scripts/run-phase.sh` and is well within Bucket B scope (< 120 lines,
in `scripts/` excluding `gate/**` + `flip-feature.sh`).

Until that guard lands, every post-merge re-dispatch cycle for an
already-verified WO will burn one verifier turn (about $1 in LLM cost
and ~10 wall-minutes before the verifier writes this escalation).

## Cross-references

- Prior r1 escalation:
  `ucil-build/escalations/20260507T0205Z-wo-0064-stale-verifier-prompt-post-merge.md`
- Prior executor-r2 escalation:
  `ucil-build/escalations/20260507T0207Z-wo-0064-stale-executor-prompt-post-merge-r2.md`
- Prior rejection-retry escalation:
  `ucil-build/escalations/20260507T0156Z-wo-0064-stale-rejection-prompt-post-merge.md`
- Original verifier-PASS:
  `ucil-build/verification-reports/WO-0064.md`
- Original critic-CLEAN: `ucil-build/critic-reports/WO-0064.md`
- Verifier flip commit: `5c36efd`
- Merge-to-main commit: `bbe645d`
- Existing post-rejection guard: `scripts/run-phase.sh:306-334`
- Harness-fix that landed the post-rejection guard: commit `c6609b9`
- WO-0063 stale-prompt precedents:
  - `ucil-build/escalations/20260506T2340Z-wo-0063-merge-gap-prompt-stale.md`
  - `ucil-build/escalations/20260506T2358Z-wo-0063-stale-rejection-prompt-recurrence.md`
  - `ucil-build/escalations/20260507T0032Z-wo-0063-stale-prompt-respawn-r4.md`

## Resolution

This escalation is auto-resolvable (Bucket A) on its own merits — the
underlying WO is already verified + merged, and there is no remediation
required against the WO itself.

The **pre-verifier guard** harness-fix (Bucket B) is filed as the
forward-looking action to prevent recurrence #3+. A `harness-fixer`
invocation to apply it is overdue and would close the loop on the
post-merge re-dispatch family of escalations definitively.

resolved: true
