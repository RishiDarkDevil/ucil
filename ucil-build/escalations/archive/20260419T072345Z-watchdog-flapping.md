---
timestamp: 2026-04-19T07:23:45Z
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
Main HEAD:              f4fe62e

Starting ./scripts/run-phase.sh 1
2026-04-19T07:00:38Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T07:05:39Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T07:05:39Z [watchdog] spawned resume.sh (pid 19688)
Already up to date.

=== Resume summary ===
Phase:                  1
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 1
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              f4fe62e

Starting ./scripts/run-phase.sh 1
2026-04-19T07:06:39Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T07:11:40Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T07:12:42Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T07:17:42Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T07:18:44Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T07:23:45Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
