---
timestamp: 2026-04-23T00:41:13Z
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
Main HEAD:              0b71ae8

Starting ./scripts/run-phase.sh 2
2026-04-23T00:06:00Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T00:11:00Z [watchdog] invoking scripts/resume.sh --yes
2026-04-23T00:11:00Z [watchdog] spawned resume.sh (pid 12723)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 241
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              0b71ae8

Starting ./scripts/run-phase.sh 2
2026-04-23T00:12:00Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T00:17:01Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T00:18:04Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T00:23:04Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T00:24:06Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T00:29:07Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T00:30:09Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T00:35:10Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T00:36:12Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T00:41:13Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
