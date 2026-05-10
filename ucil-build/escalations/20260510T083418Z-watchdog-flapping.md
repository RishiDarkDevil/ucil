---
timestamp: 2026-05-10T08:34:18Z
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
2026-05-10T07:51:52Z [watchdog] invoking scripts/resume.sh --yes
2026-05-10T07:51:52Z [watchdog] spawned resume.sh (pid 31944)
Already up to date.

=== Resume summary ===
Phase:                  3
Features passing:       118 / 234
Work-orders on disk:    94
Unresolved escalations: 17
Open rejections:        15
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              232dc7e

Starting ./scripts/run-phase.sh 3
2026-05-10T07:52:52Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T07:57:53Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T07:58:56Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T08:03:56Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T08:04:58Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T08:09:59Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T08:11:05Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T08:16:05Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T08:17:08Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T08:22:09Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T08:23:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T08:28:14Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T08:29:18Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T08:34:18Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
