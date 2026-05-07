---
timestamp: 2026-05-07T00:32:06Z
type: harness-orchestrator-bug
work_order: WO-0063
recurrence_count: 4
severity: harness-config
blocks_loop: false
requires_planner_action: false
verifier_session: vrf-a89188e7-6523-42d1-97d4-f8378403f5d7
---

# WO-0063 stale-prompt re-spawn — fourth occurrence

The orchestrator's verifier-dispatch path has now spawned **four**
verifier sessions against an already-verified, already-merged work-order
with `attempts: 0` and no rejection file:

| # | Session | Verified at | Commit | Verdict |
|---|---------|-------------|--------|---------|
| 1 | `vrf-dc14625e-…` (initial) | ~2026-05-06 (commit `3430f17`) | `a12e97f` | PASS — flip P2-W7-F06 |
| 2 | `vrf-4d124aac-…` (retry-1) | 2026-05-06T23:36:09Z (commit `272402b`) | `a12e97f` | PASS (no-op) |
| 3 | `vrf-eccb9fce-…` (retry-2) | 2026-05-07T00:24:20Z (commit `3a83771`) | `a12e97f` | PASS (no-op, "third stale-prompt re-spawn") |
| 4 | `vrf-a89188e7-…` (retry-3 — THIS) | 2026-05-07T00:32:06Z (this report) | `a12e97f` | PASS (no-op, "fourth stale-prompt re-spawn") |

## Load-bearing facts

1. WO-0063 / feature P2-W7-F06 already has `passes: true` in
   `ucil-build/feature-list.json`.
2. Branch `feat/WO-0063-search-code-g2-fused-refresh` is fully merged
   into `main` at commit `1e3c4e3` (`merge: WO-0063 search_code G2 fused
   refresh (feat → main)`). `git merge-base --is-ancestor a12e97f main`
   returns YES.
3. `ucil-build/rejections/WO-0063.md` does NOT exist. No rejection has
   ever been written.
4. `ucil-build/verification-reports/root-cause-WO-0063.md` does NOT
   exist. No root-cause-finder has ever been engaged.
5. Three prior escalations against this exact bug, all auto-resolved
   Bucket A:
    - `20260506T2340Z-wo-0063-merge-gap-prompt-stale.md`
    - `20260506T2358Z-wo-0063-stale-rejection-prompt-recurrence.md`
    - `20260507-0025-wo-WO-0063-attempts-exhausted.md`

## Why this matters

Each no-op verifier re-spawn burns ≈5 minutes of clean-slate test
runtime + the LLM tokens for an executor (none, but the harness still
allocates the slot) + critic (none, but again the harness allocates) +
verifier work product. The orchestrator is now in an effectively
**infinite no-op loop** that triage's Bucket A keeps closing the
escalation but cannot prevent the next dispatch.

## Suggested fix (verifier-side guard)

The cheapest fix is verifier-side: at session-start, the verifier
checks whether the target WO is already verified-and-merged with
`attempts: 0` and no rejection file. If so, it exits immediately with
a "NO-OP RE-VERIFICATION — feature already passes at this commit"
note and refuses to spend cycles on a clean-slate rebuild.

```bash
# scripts/spawn-verifier.sh additions (sketch)
WO_ID="$1"
FEATURE_ID="$(jq -r --arg wo "$WO_ID" '
  .[] | select(.id==$wo) | .feature_ids[0]
' ucil-build/work-orders/${WO_NUM}-*.json)"

ALREADY_PASSES=$(jq -r --arg id "$FEATURE_ID" '
  .features[] | select(.id==$id) | .passes
' ucil-build/feature-list.json)
LAST_COMMIT=$(jq -r --arg id "$FEATURE_ID" '
  .features[] | select(.id==$id) | .last_verified_commit
' ucil-build/feature-list.json)
WORKTREE_HEAD=$(git -C "ucil-wt/${WO_ID}" rev-parse HEAD 2>/dev/null || true)

if [[ "$ALREADY_PASSES" == "true" ]] \
   && [[ "$LAST_COMMIT" == "$WORKTREE_HEAD" ]] \
   && [[ ! -f "ucil-build/rejections/${WO_ID}.md" ]] \
   && [[ ! -f "ucil-build/verification-reports/root-cause-${WO_ID}.md" ]]; then
  echo "[spawn-verifier] NO-OP — $WO_ID already PASS at $WORKTREE_HEAD; no rejection file. Skipping verifier spawn." >&2
  exit 0
fi
```

## Suggested fix (orchestrator-side guard)

Per the resolved escalation `20260506T2358Z-wo-0063-stale-rejection-
prompt-recurrence.md`, the orchestrator's rejection-retry dispatcher
should also check for the existence of a rejection file before
dispatching a "retry against rejection" prompt:

```bash
# scripts/run-phase.sh / scripts/run-executor.sh
if [[ "$prompt_kind" == "rejection-retry" ]]; then
  rejection_file="ucil-build/rejections/${WO_ID}.md"
  rcf_file="ucil-build/verification-reports/root-cause-${WO_ID}.md"
  if [[ ! -f "$rejection_file" ]] && [[ ! -f "$rcf_file" ]]; then
    echo "ABORT: rejection-retry dispatched for $WO_ID but no rejection/RCF exists" >&2
    exit 64
  fi
fi
```

## Recommended action

Engage `harness-fixer` to apply both guards (Bucket B). The fix is
< 30 lines total across two scripts and entirely within harness
purview (no UCIL source touched). The test is straightforward:
spawn a verifier against any already-passing WO with no rejection
file → must exit 0 immediately with the NO-OP message.

## This escalation's status

`blocks_loop: false` — the loop should continue. The Phase 2
remaining work (P2-W8-F04 / P2-W8-F07 / P2-W8-F08) is unblocked. This
escalation is purely an advisory pointing at a recurring orchestrator
inefficiency that wastes ~5min × N occurrences of clean-slate
re-verification.

The verification report `ucil-build/verification-reports/WO-0063.md`
has been refreshed to reflect this fourth no-op verifier session;
`P2-W7-F06.last_verified_*` timestamps refreshed accordingly. No
semantic change to feature-list (`passes` field is monotone and was
already `true`).
