---
severity: harness-config
blocks_loop: false
requires_planner_action: false
agent: executor
work_order: WO-0063
feature_ids: [P2-W7-F06]
---

# WO-0063 — executor re-invoked AGAIN against a stale "rejection" prompt; work is complete, merged, and re-verified

## Summary

The orchestrator fired another executor turn for WO-0063 with the same
stale prompt previously documented in
`ucil-build/escalations/20260506T2340Z-wo-0063-merge-gap-prompt-stale.md`
(already resolved). The prompt instructs me to apply remediation from
two files that do not exist:

- `ucil-build/rejections/WO-0063.md` — **does not exist**
- `ucil-build/verification-reports/root-cause-WO-0063.md` — **does not exist**

## Ground truth on `main` HEAD `272402b`

| Fact | Evidence |
|------|----------|
| Branch already merged | commit `1e3c4e3 merge: WO-0063 search_code G2 fused refresh (feat → main)` |
| Implementation present on main | `crates/ucil-daemon/src/g2_search.rs` exists on `main` |
| P2-W7-F06 already `passes=true` | `jq '.features[] \| select(.id == "P2-W7-F06")'` returns `passes: true, last_verified_by: verifier-4d124aac-..., last_verified_commit: a12e97ff3...` |
| Verifier already PASS (original) | `verification-reports/WO-0063.md` line 7: PASS |
| Verifier already PASS (retry-1 re-verify) | commit `272402b chore(verifier): WO-0063 retry-1 re-verify — PASS (no-op confirm)` |
| Critic CLEAN (retry-1) | commit `9d197b8 chore(critic): WO-0063 retry-1 re-review — CLEAN with warnings` |
| Prior merge-gap escalation resolved | commit `b6544bb chore(escalation): resolve WO-0063 merge gap — merged at 1e3c4e3` |
| Prior stale-prompt escalation resolved | `escalations/20260506T2340Z-wo-0063-merge-gap-prompt-stale.md` ends with `resolved: true` |

The work-order is **doubly verified** (once at flip, once on the
post-merge no-op re-verify) and merged into trunk. There is nothing
remaining to implement.

## Why I did not write code

Per root `CLAUDE.md` anti-laziness contract and the executor protocol:

1. **No rejection or RCF artefact exists to remediate from.** Pretending
   to apply a non-existent diagnosis is a fabrication risk.
2. **`feature-list.json:P2-W7-F06.passes` is already `true`** at the
   commit that the verifier verified; the field is whitelisted and
   monotone — even the verifier cannot un-flip it.
3. **No new diff is possible.** The feature branch HEAD `a12e97f` is
   already merged into `main`; any executor "remediation" would either
   duplicate an existing commit (no-op) or invent scope outside the
   work-order, which is forbidden.
4. **The marker file `0063-search-code-g2-fused-refresh-ready-for-review.md`
   that the prompt expects** does not match the marker the original
   execution wrote (`0063-ready-for-review.md`, committed at `a12e97f`).
   Writing a second marker under a new name would not constitute
   remediation; it would just confuse the next reader.

## Worktree state

The worktree at `/home/rishidarkdevil/Desktop/ucil-wt/WO-0063` had the
same stale uncommitted modification to
`ucil-build/verification-reports/coverage-ucil-daemon.md` that the prior
stale-prompt escalation noted. Restored to HEAD via
`git checkout -- ucil-build/verification-reports/coverage-ucil-daemon.md`.
Worktree is now clean and matches
`origin/feat/WO-0063-search-code-g2-fused-refresh`.

## Recommended remediation (Bucket A — admin/benign auto-resolve)

This escalation is auto-resolvable: the underlying condition is already
fixed in HEAD. Triage should:

1. Append a `## Resolution` note pointing to commit `1e3c4e3` (merge)
   and `272402b` (post-merge re-verify PASS).
2. Set this escalation's `resolved: true` (already done — see footer).
3. Commit + push.

## The actual root cause (recurring harness bug)

Something in the orchestrator continues to dispatch
`scripts/run-executor.sh` with a synthesised rejection prompt for
WO-0063 even though:
- `ucil-build/rejections/WO-0063.md` does not exist
- `ucil-build/verification-reports/root-cause-WO-0063.md` does not exist
- `attempts: 0` and `passes: true` on the feature

The recurrence (this is now the **second** stale-prompt invocation in
under 24h) suggests the run-executor entry-point is fabricating the
"prior verifier rejected" framing without checking ground truth. A
`harness-fixer` agent invocation on `scripts/run-executor.sh` (or
whichever orchestrator script generates the executor prompt) is
warranted to add a precondition guard:

```bash
# Pseudocode — refuse to dispatch a "retry against rejection" prompt
# if no rejection or RCF artefact exists.
if [[ "$prompt_kind" == "rejection-retry" ]]; then
  rejection_file="ucil-build/rejections/${WO_ID}.md"
  rcf_file="ucil-build/verification-reports/root-cause-${WO_ID}.md"
  if [[ ! -f "$rejection_file" ]] && [[ ! -f "$rcf_file" ]]; then
    echo "ABORT: rejection-retry dispatched for $WO_ID but no rejection/RCF exists" >&2
    exit 64
  fi
fi
```

Without this guard, every future no-op verifier "retry" cycle on a
verified+merged WO will burn another executor turn → critic turn →
verifier turn (≈3 LLM invocations) for zero work product.

## What I am NOT doing in this turn

- **No source-code edits.** The work is verified and merged.
- **No `feature-list.json` writes.** Whitelist-protected; already
  `passes=true`.
- **No new ready-for-review marker file.** Original is at `a12e97f` on
  the feature branch.
- **No `git push --force` / `--amend` / `--no-verify`.** Forbidden.

## Cross-references

- `ucil-build/escalations/20260506T2340Z-wo-0063-merge-gap-prompt-stale.md` — first stale-prompt occurrence (resolved)
- `ucil-build/escalations/20260506T2340Z-wo-0063-merge-gap-prompt-stale.md` (commit `b6544bb`) — merge-gap resolution
- `ucil-build/critic-reports/WO-0063.md` — CLEAN-with-warnings critic verdict (re-confirmed)
- `ucil-build/verification-reports/WO-0063.md` — PASS verifier verdict (re-confirmed)
- `ucil-build/work-orders/0063-search-code-g2-fused-refresh.json` — work-order
- `feat/WO-0063-search-code-g2-fused-refresh @ a12e97f` — feature-branch HEAD (merged into main at `1e3c4e3`)
- `main @ 272402b` — re-verify PASS commit

resolved: true
