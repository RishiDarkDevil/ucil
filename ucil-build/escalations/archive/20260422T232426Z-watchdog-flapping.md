---
timestamp: 2026-04-22T23:24:26Z
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
2026-04-22T22:43:12Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T22:48:12Z [watchdog] invoking scripts/resume.sh --yes
2026-04-22T22:48:12Z [watchdog] spawned resume.sh (pid 59927)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 234
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              0f2a18b

Starting ./scripts/run-phase.sh 2
2026-04-22T22:49:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T22:54:13Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T22:55:15Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T23:00:16Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T23:01:18Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T23:06:18Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T23:07:20Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T23:12:21Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T23:13:23Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T23:18:23Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-22T23:19:25Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-22T23:24:26Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
