---
severity: harness-config
blocks_loop: false
requires_planner_action: false
agent: verifier
work_order: WO-0064
feature_ids: [P2-W8-F04]
---

# WO-0064 — verifier re-spawned against a stale "verify" prompt; work is verified PASS and already merged

## Summary

A fresh `verifier` subagent was dispatched with the prompt:

> You are the UCIL verifier. Target to verify: WO-0064.
> Run all acceptance tests from a clean slate. Flip passes=true only if everything is green and the mutation check confirms.

This is the **third** stale-prompt recurrence on WO-0064 (after the
executor-side stale invocations documented at
`20260507T0156Z-wo-0064-stale-rejection-prompt-post-merge.md`) and the
N-th overall on the same harness bug previously logged for WO-0063
(`20260506T2340Z…`, `20260506T2358Z…`, `20260507T0032Z…`).

## Ground truth on `main` HEAD `4c08d29`

| Fact | Evidence |
|------|----------|
| Feature already `passes=true` | `jq '.features[] \| select(.id == "P2-W8-F04") \| {passes, last_verified_by, last_verified_commit, attempts}' ucil-build/feature-list.json` → `passes: true, last_verified_by: verifier-7f7ea48e-…, last_verified_commit: e737892…, attempts: 0` |
| Verifier report present, PASS | `ucil-build/verification-reports/WO-0064.md` line 7: `**Verdict**: **PASS**` (verifier session `vrf-7f7ea48e-8254-4118-b031-ea2a01b5d1d5`, dated `2026-05-07T01:52:36Z`) |
| Verifier flip commit | `5c36efd chore(verifier): WO-0064 PASS — flip P2-W8-F04 + reports` |
| Critic verdicts CLEAN | `ed09fe1` (initial) and `4c08d29` (retry-1 re-review) both CLEAN |
| Branch merged into main | `bbe645d merge: WO-0064 lancedb-chunk-indexer (orphan → main) — P2-W8-F04 closure` |
| `rejections/WO-0064.md` | does not exist (never rejected) |
| Working tree | clean; `HEAD == origin/main == 4c08d29` |

## Why this verifier session is a no-op

Per `.claude/agents/verifier.md` and the root anti-laziness contract:

1. **`passes` is monotone.** Once flipped to `true` by a verifier in a
   distinct session, the field cannot be re-asserted. Re-running
   `scripts/flip-feature.sh P2-W8-F04 pass <sha>` would be a no-op write
   that just churns the file mtime.
2. **A second verifier pass cannot un-do anything.** There is no path
   from `passes=true` back to `passes=false` short of an ADR + re-seed
   (rare and painful, per `ucil-build/CLAUDE.md`).
3. **Source edits are forbidden.** Even if I ran the full clean-slate
   acceptance protocol again and it produced an unexpected failure, the
   only legitimate output would be an `ucil-build/rejections/WO-0064.md`
   — but feature P2-W8-F04 cannot be un-flipped, so that rejection
   would be cosmetic and would not reflect the system's actual state.
4. **The feature branch is fully merged.** `feat/WO-0064-lancedb-chunk-indexer @ e737892`
   was fast-forward merged into `main` at `bbe645d`. Re-running
   acceptance tests on a worktree that's identical to a subset of
   trunk's history reproduces work already captured in
   `verification-reports/WO-0064.md`.
5. **Spawning a fresh verifier session for a `passes=true` WO is the
   harness bug, not a missing test pass.** The verifier protocol
   assumes the feature is in `passes=false` state at session start.

## What I am NOT doing in this turn

- **No source-code edits** (forbidden per `.claude/agents/verifier.md`
  hard rules).
- **No `feature-list.json` writes** (the feature is already verifier-flipped;
  re-flipping would be a churn no-op).
- **No new `verification-reports/WO-0064.md`** (the canonical PASS report
  from `vrf-7f7ea48e` already exists and reflects truth at
  `e737892`/`bbe645d`).
- **No new `rejections/WO-0064.md`** (nothing failed; the work is verified
  PASS and merged).
- **No re-run of `cargo clean && cargo test --workspace`** (would
  duplicate the prior verifier's clean-slate run, burn ~20 min of CI
  time, and produce no new state).
- **No new RFR marker, no new critic-report, no new flip commit.**
- **No `git push --force` / `--amend` / `--no-verify`.** Forbidden.

## Recommended remediation (Bucket A — auto-resolve)

This escalation, like the executor-side `20260507T0156Z…` one, is
auto-resolvable by triage:

1. The underlying condition is already `resolved` in HEAD
   (`P2-W8-F04.passes=true`, branch merged at `bbe645d`).
2. Triage may append a `## Resolution` note pointing to commits
   `bbe645d` (merge) and `5c36efd` (verifier flip) and ack this
   escalation as well; `resolved: true` is set in the trailing line.

## Pattern: the harness bug is now load-bearing

Three WOs hit by the same bug (WO-0063 multiple times, WO-0064 at the
executor and now at the verifier):

| Date | Subagent re-spawned | WO | Net effect |
|------|----------------------|----|------------|
| 2026-05-06T23:40Z | executor (stale RFR-prompt) | WO-0063 | wasted turn |
| 2026-05-06T23:58Z | executor (recurrence) | WO-0063 | wasted turn |
| 2026-05-07T00:32Z | executor (r4) | WO-0063 | wasted turn |
| 2026-05-07T01:56Z | executor (post-merge) | WO-0064 | wasted turn |
| **2026-05-07T02:05Z** | **verifier (post-merge)** | **WO-0064** | **wasted turn (this file)** |

The `harness-fixer` invocation is overdue. The recommended guard
remains the one already proposed in those prior escalations:

```bash
# In scripts/run-verifier.sh (and scripts/run-executor.sh, scripts/run-critic.sh):
wo_id="${WO_ID:-?}"
if [[ "$wo_id" != "?" ]]; then
  feature_ids=$(jq -r ".work_orders[] | select(.id == \"$wo_id\") | .feature_ids[]" ucil-build/feature-list.json 2>/dev/null \
                || jq -r ".feature_ids[]" "ucil-build/work-orders/$(printf '%04d' "${wo_id#WO-}")-"*.json 2>/dev/null)
  all_pass="true"
  for fid in $feature_ids; do
    p=$(jq -r ".features[] | select(.id == \"$fid\") | .passes" ucil-build/feature-list.json)
    [[ "$p" != "true" ]] && all_pass="false" && break
  done
  if [[ "$all_pass" == "true" ]]; then
    echo "ABORT: $wo_id already passes; refusing to dispatch another agent against a verifier-flipped WO" >&2
    exit 64
  fi
fi
```

Plus an optional secondary guard: short-circuit if the feature branch's
HEAD is an ancestor of `origin/main`. Both conditions hold for WO-0064
right now.

## Cross-references

- `ucil-build/work-orders/0064-lancedb-chunk-indexer.json` — work-order
- `ucil-build/work-orders/0064-ready-for-review.md` — RFR marker (final-sha `e737892`)
- `ucil-build/critic-reports/WO-0064.md` — CLEAN critic verdict (re-confirmed retry-1 at `4c08d29`)
- `ucil-build/verification-reports/WO-0064.md` — PASS verifier verdict (vrf-7f7ea48e)
- `feat/WO-0064-lancedb-chunk-indexer @ e737892` — verified feature-branch HEAD
- `main @ 4c08d29` — current trunk (post-merge + post-critic-retry)
- Sibling executor-side stale-prompt note: `ucil-build/escalations/20260507T0156Z-wo-0064-stale-rejection-prompt-post-merge.md`
- WO-0063 precedents:
  - `ucil-build/escalations/20260506T2340Z-wo-0063-merge-gap-prompt-stale.md`
  - `ucil-build/escalations/20260506T2358Z-wo-0063-stale-rejection-prompt-recurrence.md`
  - `ucil-build/escalations/20260507T0032Z-wo-0063-stale-prompt-respawn-r4.md`

resolved: true
