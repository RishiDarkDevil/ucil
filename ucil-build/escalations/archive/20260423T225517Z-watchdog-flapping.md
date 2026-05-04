---
timestamp: 2026-04-23T22:55:17Z
type: watchdog-flapping
severity: high
blocks_loop: true
requires_planner_action: true
---

# UCIL watchdog restart loop detected

The autonomous loop died and was restarted 3 times inside
3600s. Probable cause: a consistent crash (not a transient
kill). Watchdog has exited; fix the root cause and re-invoke via
`scripts/install-watchdog.sh` or `scripts/_watchdog.sh &` once the
loop runs clean for >1h.

Tail of `ucil-build/telemetry/watchdog.log`:
```
Work-orders on disk:    41
Unresolved escalations: 283
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              c224584

Starting ./scripts/run-phase.sh 2
2026-04-23T22:38:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T22:43:13Z [watchdog] invoking scripts/resume.sh --yes
2026-04-23T22:43:13Z [watchdog] spawned resume.sh (pid 71904)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 283
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              c224584

Starting ./scripts/run-phase.sh 2
2026-04-23T22:44:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T22:49:14Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T22:50:16Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T22:55:17Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
