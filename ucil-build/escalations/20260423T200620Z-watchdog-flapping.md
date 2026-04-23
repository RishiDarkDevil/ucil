---
timestamp: 2026-04-23T20:06:20Z
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
Main HEAD:              6dca23b

Starting ./scripts/run-phase.sh 2
2026-04-23T19:37:03Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T19:42:04Z [watchdog] invoking scripts/resume.sh --yes
2026-04-23T19:42:04Z [watchdog] spawned resume.sh (pid 39483)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 262
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              6dca23b

Starting ./scripts/run-phase.sh 2
2026-04-23T19:43:04Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T19:48:05Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T19:49:07Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T19:54:07Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T19:55:09Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T20:00:10Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T20:01:19Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T20:06:20Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
