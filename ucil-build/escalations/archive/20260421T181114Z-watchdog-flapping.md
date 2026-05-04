---
timestamp: 2026-04-21T18:11:14Z
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
Main HEAD:              0176593

Starting ./scripts/run-phase.sh 2
2026-04-21T17:42:04Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T17:47:05Z [watchdog] invoking scripts/resume.sh --yes
2026-04-21T17:47:05Z [watchdog] spawned resume.sh (pid 28702)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 159
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              0176593

Starting ./scripts/run-phase.sh 2
2026-04-21T17:48:05Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T17:53:06Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T17:54:08Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T17:59:08Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T18:00:10Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T18:05:11Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T18:06:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T18:11:14Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
