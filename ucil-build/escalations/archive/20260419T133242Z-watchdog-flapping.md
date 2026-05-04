---
timestamp: 2026-04-19T13:32:42Z
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
2026-04-19T12:50:26Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T12:50:26Z [watchdog] spawned resume.sh (pid 24641)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 11
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              6fa2015

Starting ./scripts/run-phase.sh 2
2026-04-19T12:51:26Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T12:56:27Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T12:57:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T13:02:29Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T13:03:31Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T13:08:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T13:09:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T13:14:34Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T13:15:36Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T13:20:37Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T13:21:39Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T13:26:39Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T13:27:41Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T13:32:42Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
