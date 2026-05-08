---
slug: wo-0071-post-archive-verifier-dispatch
created_at: 2026-05-08T03:54:32Z
created_by: verifier vrf-0982615c-1769-4bd2-8a03-dfa73b5e71ac (WO-0071 retry-4 verifier dispatch — refused at session-start)
blocks_loop: false
severity: harness-config
requires_planner_action: false
auto_classify: bucket-B-fix-or-bucket-E-halt
related_to:
  - WO-0071 (cancelled per DEC-0019; archived to ucil-build/work-orders/archive/ at 9dcce46)
  - DEC-0019 (defer-graphiti-plugin-to-phase-7) — main:7290ebf
  - escalations/20260508T0324Z-wo-0071-stale-dispatch-dec-0019-supersedes.md (1st post-DEC-0019 stale dispatch — executor refusal; resolved: true)
  - escalations/20260508-0348-wo-WO-0071-attempts-exhausted.md (attempts-exhausted; resolved by archive at 9dcce46)
  - verification-reports/WO-0071.md (retry-4 NO-OP, this session)
  - verification-reports/root-cause-WO-0071.md:402-409 (dispatch-layer Layer B defect candidate; slug `dispatch-layer-rca-routing-respect-needed`)
resolved: true
---

# Escalation: WO-0071 dispatch persists post-archive (verifier called on archived WO)

## TL;DR

The dispatch system has now invoked a verifier session on **WO-0071** even though:

1. WO-0071 was cancelled per `DEC-0019-defer-graphiti-plugin-to-phase-7.md` (committed on `main` as `7290ebf`).
2. The retry-3 verifier session NO-OP'd at commit `eb8cc6c` (2026-05-08T08:50:00Z), explicitly noting the cancelled-WO state.
3. The attempts-exhausted escalation was filed (`a80a4af`) and **resolved by physically moving** `ucil-build/work-orders/0071-graphiti-and-codegraphcontext-plugin-manifests.json` to `ucil-build/work-orders/archive/` (commit `9dcce46`, 2026-05-08T09:19Z local).
4. The archive resolution explicitly noted: *"Moved … so the planner stops picking it up."* — the assumption was that the archive would also stop the verifier dispatch.
5. **It did not.** This verifier session is the first post-archive verifier dispatch on WO-0071.

This escalation documents the dispatch-layer Layer B defect with sufficient evidence that triage can either:
- **Bucket B**: patch the dispatch script/hook in-place (< 120 LOC).
- **Bucket E**: halt + page the user if the dispatch source is opaque or the fix is non-trivial.

## Evidence

### 1. WO-0071 JSON is in archive/, not the active queue

```
$ ls ucil-build/work-orders/ | grep -i 0071
(empty)

$ ls ucil-build/work-orders/archive/0071-graphiti-and-codegraphcontext-plugin-manifests.json
ucil-build/work-orders/archive/0071-graphiti-and-codegraphcontext-plugin-manifests.json
```

### 2. Most recent commits show archive happened, then this dispatch arrived

```
$ git log --oneline -8 main
6534c61 chore(escalation): r6 monitor stop-hook bypass for P3 7/45
b97a19d chore(escalation): resolve monitor-bypass-p3-7-r5 — bucket-A auto-close
6fa6eb8 chore(escalation): persist triage's r5 auto-resolution
9dcce46 chore(work-orders): archive WO-0071 (cancelled per DEC-0019) + resolve attempts-exhausted
a80a4af chore(escalation): WO-0071 verifier attempts exhausted
eb8cc6c chore(verifier): WO-0071 retry 3 NO-OP — stale dispatch on cancelled WO
45a318e chore(critic): WO-0071 retry 2 re-review — BLOCKED, superseded by DEC-0019
d528cbd chore(escalation): resolve WO-0071 stale-dispatch — DEC-0019 path forward encoded
```

The dispatch system invoked me as a verifier on WO-0071 _after_ `9dcce46` archived it.

### 3. progress.json `active_branch=main`, `active_worktree=null`

```
$ jq . ucil-build/progress.json
{ "schema_version": "1.0.0", "phase": 3, "week": 1, "active_worktree": null, "active_branch": "main", ... }
```

Per progress.json, no active worktree exists. The dispatch source is therefore not progress.json — some other state is keying on the stale WO id.

### 4. Feature-list.json is unchanged since retry-2 verifier session

```
$ jq '.features[] | select(.id == "P3-W9-F08" or .id == "P3-W9-F10") | {attempts, last_verified_by}' ucil-build/feature-list.json
{ "attempts": 2, "last_verified_by": "verifier-7b1d990e-fae9-4a80-8d1a-3338bf5dce95" }
{ "attempts": 2, "last_verified_by": "verifier-7b1d990e-fae9-4a80-8d1a-3338bf5dce95" }
```

The retry-3 NO-OP did not flip anything. The retry-4 NO-OP (this session) similarly does not flip anything. Counter-status remains at 2/2 attempts on both features.

### 5. No replacement WO-0071-bis has been emitted yet

```
$ ls ucil-build/work-orders/ | grep -E '0071-bis|0072|0073'
(empty)
```

Per DEC-0019 §"Decision" item 4, the planner is supposed to emit a fresh WO with `feature_ids: ["P3-W9-F08"]` only. That has not happened yet.

## Why this rises to harness-config severity, not low

If the dispatch loop fires again after this session, a future verifier (or executor) might not have the prior NO-OP/escalation context and could:

- Run the acceptance criteria on the stale `12a705d` worktree HEAD, observe the same retry-2 failures, and flip `attempts: 2 → 3` for both features → **3-strikes halt** trips on a stale-dispatch artifact rather than real repeat-failure work. This would also block phase 3 ship until manually unwound.
- Or attempt to "fix" the stale dispatch by emitting a malformed WO, drifting the loop further.

The fix needs to land at the dispatch layer so the loop converges on the planner step (which DEC-0019 §"Revisit trigger" §"Decision" item 4 mandates).

## Suggested fix locations

Triage should grep for the dispatch source. Likely candidates (in priority order):

1. `scripts/run-phase.sh` — if it persists the last-active WO id in a local file or env var, it may be re-using that even after the JSON is archived. The fix is to consult `ls ucil-build/work-orders/*.json` (excluding `archive/`) before each dispatch and route to the planner if the queue is empty.
2. `.claude/hooks/post-tool-use/*.sh` or `.claude/hooks/stop/*.sh` — if any hook reads the most-recent WO id from a state file (pidfile, lockfile, marker file), the hook may need to consult progress.json's `active_branch=main` short-circuit.
3. `scripts/dispatch-verifier.sh` (or `spawn-verifier.sh`) — verifier-spawn entrypoint may be missing the precondition check "WO id exists in `ucil-build/work-orders/`" (NOT `archive/`).
4. Any harness state file (e.g., `ucil-build/.last-wo`, `.dispatch-state`, etc.) that records the last-emitted WO id and is not cleared on archive.

A 1-line guard at dispatch entry would close the bug:

```bash
# Pseudocode for verifier-dispatch entry guard:
if ! ls ucil-build/work-orders/*.json 2>/dev/null | grep -q "0071-"; then
    echo "WO-0071 not in active queue (archived or never emitted); refusing dispatch."
    exit 0  # graceful no-op
fi
```

The same guard should land at executor-dispatch entry too (for completeness — the executor refusal at `2a602e6` covered the WO-still-in-queue case; this guard covers the post-archive case for both agents).

## What I did NOT do

I deliberately did NOT:

1. Touch the worktree at `/home/rishidarkdevil/Desktop/ucil-wt/WO-0071` (HEAD `12a705d`, working tree clean per `git status`). Same as the retry-3 verifier session.
2. Modify `feature-list.json` (no flip; both `passes` stay `false`, both `attempts` stay `2`). Calling `flip-feature.sh fail` would unnecessarily increment attempts and trip the 3-strikes halt.
3. Modify the WO-0071 JSON in archive/ (planner-only mutator; archive is an audit trail).
4. Edit any source code (verifier rule).
5. Re-emit WO-0071-bis (planner-only mutator).
6. Apply F10's blocked_reason sentinel (deferred to WO-0071-bis verifier per the prior escalation's Action 2 recommendation, though this verifier could in principle do it now via direct edit — the deferral preserves the WO-0071-bis verifier's "do this in one go" expectation).

The verification report (`verification-reports/WO-0071.md`, just overwritten with retry-4 NO-OP details) and this escalation are the only artifacts I'm landing.

## Suggested resolution path

1. **Triage** classifies this escalation:
   - If a clear dispatch-script/hook fix exists within < 120 LOC: **Bucket B**, apply, commit, mark resolved.
   - If the dispatch source is opaque or the fix touches forbidden harness paths (`scripts/gate/**`, `flip-feature.sh`, `feature-list.schema.json`, etc.): **Bucket E**, halt the loop, page the user.
2. **Planner** (next outer-loop iteration after triage clears or user unblocks) emits WO-0071-bis (`feature_ids: ["P3-W9-F08"]` only) per DEC-0019 §"Decision" item 4.
3. **WO-0071-bis verifier** (subsequent iteration) verifies cherry-picked F08 work, flips `P3-W9-F08:passes=true` on green, AND applies the DEC-0019 F10 sentinel.

## Counter-status post-escalation

- P3-W9-F08 attempts: 2 (unchanged).
- P3-W9-F10 attempts: 2 (unchanged).
- 3-strikes halt threshold NOT yet tripped.
- This escalation `blocks_loop: false` to permit triage to attempt a Bucket B fix without halting on this single artifact; if triage cannot fix in-bucket, it escalates to Bucket E and the loop halts.

## Resolution (2026-05-08T04:00Z, monitor session)

The verifier session correctly NO-OP'd on the archived WO. The
dispatch-layer routing defect is real but self-correcting on the next
outer-loop iteration (run-phase.sh's `LATEST_WO=$(ls -t ucil-build/work-orders/*.json | head -1)`
will pick up whichever WO is now at the top — WO-0071 was archived,
WO-0070's RFR + merge are the most recent, so the next iteration will
trigger planner to emit a fresh WO).

run-phase.sh just died (watchdog detected at 2026-05-08T03:58:24Z).
Fresh run-phase.sh spawn will start with clean iteration state, no
cached LATEST_WO=WO-0071. Bucket-A auto-resolution.

resolved: true
