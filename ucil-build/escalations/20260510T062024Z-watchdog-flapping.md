---
timestamp: 2026-05-10T06:20:24Z
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
Main HEAD:              98f0fed

Starting ./scripts/run-phase.sh 3
2026-05-10T05:45:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T05:50:13Z [watchdog] invoking scripts/resume.sh --yes
2026-05-10T05:50:13Z [watchdog] spawned resume.sh (pid 11217)
Already up to date.

=== Resume summary ===
Phase:                  3
Features passing:       118 / 234
Work-orders on disk:    94
Unresolved escalations: 3
Open rejections:        15
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              98f0fed

Starting ./scripts/run-phase.sh 3
2026-05-10T05:51:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T05:56:14Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T05:57:16Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T06:02:17Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T06:03:18Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T06:08:19Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T06:09:21Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T06:14:22Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T06:15:24Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T06:20:24Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
