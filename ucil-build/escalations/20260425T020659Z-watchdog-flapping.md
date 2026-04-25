---
timestamp: 2026-04-25T02:06:59Z
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
2026-04-25T01:25:45Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T01:30:45Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T01:30:45Z [watchdog] spawned resume.sh (pid 85028)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 365
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              7724713

Starting ./scripts/run-phase.sh 2
2026-04-25T01:31:46Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T01:36:46Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T01:37:48Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T01:42:49Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T01:43:51Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T01:48:52Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T01:49:53Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T01:54:54Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T01:55:56Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T02:00:57Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T02:01:59Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T02:06:59Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
