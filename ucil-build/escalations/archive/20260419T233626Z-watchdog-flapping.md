---
timestamp: 2026-04-19T23:36:26Z
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
2026-04-19T22:54:10Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T22:54:10Z [watchdog] spawned resume.sh (pid 148004)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 81
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              65ac9fa

Starting ./scripts/run-phase.sh 2
2026-04-19T22:55:10Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T23:00:11Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T23:01:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T23:06:13Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T23:07:15Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T23:12:16Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T23:13:18Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T23:18:18Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T23:19:20Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T23:24:21Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T23:25:23Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T23:30:23Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T23:31:25Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T23:36:26Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
