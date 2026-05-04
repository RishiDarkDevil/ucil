---
timestamp: 2026-04-22T20:23:25Z
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
Starting ./scripts/run-phase.sh 2
2026-04-22T19:42:10Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T19:47:11Z [watchdog] invoking scripts/resume.sh --yes
2026-04-22T19:47:11Z [watchdog] spawned resume.sh (pid 24083)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 213
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              9309964

Starting ./scripts/run-phase.sh 2
2026-04-22T19:48:11Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T19:53:11Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T19:54:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T19:59:14Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T20:00:16Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T20:05:17Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T20:06:19Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T20:11:19Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T20:12:21Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T20:17:22Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T20:18:24Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T20:23:25Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
