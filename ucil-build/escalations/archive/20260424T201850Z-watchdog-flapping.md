---
timestamp: 2026-04-24T20:18:50Z
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
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              d2d306c

Starting ./scripts/run-phase.sh 2
2026-04-24T19:55:43Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T20:00:44Z [watchdog] invoking scripts/resume.sh --yes
2026-04-24T20:00:44Z [watchdog] spawned resume.sh (pid 34195)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 330
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              d2d306c

Starting ./scripts/run-phase.sh 2
2026-04-24T20:01:44Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T20:06:45Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T20:07:47Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T20:12:47Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T20:13:49Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T20:18:50Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
