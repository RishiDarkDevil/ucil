---
timestamp: 2026-05-06T00:28:30Z
type: watchdog-flapping
severity: high
blocks_loop: true
requires_planner_action: true
resolved: true
---

# UCIL watchdog restart loop detected

The autonomous loop died and was restarted 3 times inside
3600s. Probable cause: a consistent crash (not a transient
kill). Watchdog has exited; fix the root cause and re-invoke via
`scripts/install-watchdog.sh` or `scripts/_watchdog.sh &` once the
loop runs clean for >1h.

Tail of `ucil-build/telemetry/watchdog.log`:
```
2026-05-05T23:49:24Z [watchdog] loop came back on its own; no restart needed
2026-05-06T00:05:27Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-06T00:10:28Z [watchdog] invoking scripts/resume.sh --yes
2026-05-06T00:10:28Z [watchdog] spawned resume.sh (pid 899619)

[resume] Main tree has uncommitted changes:
?? ucil-build/decisions/DEC-0015-search-code-g2-fan-out-and-fused-meta-field.md

  Resolve before resuming (commit or reset).
  Refusing --yes with dirty main tree.
2026-05-06T00:11:28Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-06T00:16:28Z [watchdog] invoking scripts/resume.sh --yes
2026-05-06T00:16:28Z [watchdog] spawned resume.sh (pid 901126)

[resume] Main tree has uncommitted changes:
?? ucil-build/decisions/DEC-0015-search-code-g2-fan-out-and-fused-meta-field.md

  Resolve before resuming (commit or reset).
  Refusing --yes with dirty main tree.
2026-05-06T00:17:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-06T00:22:29Z [watchdog] invoking scripts/resume.sh --yes
2026-05-06T00:22:29Z [watchdog] spawned resume.sh (pid 902489)

[resume] Main tree has uncommitted changes:
?? ucil-build/decisions/DEC-0015-search-code-g2-fan-out-and-fused-meta-field.md

  Resolve before resuming (commit or reset).
  Refusing --yes with dirty main tree.
2026-05-06T00:23:30Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-06T00:28:30Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```

## Resolution

Resolved 2026-05-06 by user-authorised monitor session.

Root cause: an orphaned ADR (`ucil-build/decisions/DEC-0015-search-code-g2-fan-out-and-fused-meta-field.md`) was left untracked in the working tree by an earlier killed planner. `scripts/resume.sh --yes` (called by the watchdog) refuses to run with a dirty tree, so each watchdog restart attempt failed at the same check. After 3 restarts/3600s the watchdog hit MAX_RESTARTS and exited.

Recovery: a fresh watchdog (PID 9517) was launched in a later resume cycle and has been running healthily since 2026-05-06 ~16:00Z UTC. The orphaned DEC-0015 ADR is being committed alongside this resolution so the watchdog's next restart attempt (if it ever happens) will succeed.

Net status now: watchdog 9517 alive, run-phase.sh 890517 alive, P2 16/25, branch synced. The escalation's blocks_loop:true halt condition no longer holds.

resolved: true
