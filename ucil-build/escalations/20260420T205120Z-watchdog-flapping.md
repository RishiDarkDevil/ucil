---
timestamp: 2026-04-20T20:51:20Z
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
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              875c1c4

Starting ./scripts/run-phase.sh 2
2026-04-20T20:22:11Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T20:27:12Z [watchdog] invoking scripts/resume.sh --yes
2026-04-20T20:27:12Z [watchdog] spawned resume.sh (pid 25600)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 127
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              875c1c4

Starting ./scripts/run-phase.sh 2
2026-04-20T20:28:12Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T20:33:13Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T20:34:15Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T20:39:15Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T20:40:17Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T20:45:18Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T20:46:20Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T20:51:20Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
