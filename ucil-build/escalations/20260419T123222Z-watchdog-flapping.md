---
timestamp: 2026-04-19T12:32:22Z
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
2026-04-19T11:50:06Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T11:50:06Z [watchdog] spawned resume.sh (pid 12391)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 4
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              a7ac631

Starting ./scripts/run-phase.sh 2
2026-04-19T11:51:06Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T11:56:07Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T11:57:09Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T12:02:09Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T12:03:11Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T12:08:12Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T12:09:14Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T12:14:14Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T12:15:16Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T12:20:17Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T12:21:19Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T12:26:19Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T12:27:21Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T12:32:22Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
