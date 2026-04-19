---
timestamp: 2026-04-19T16:27:40Z
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
2026-04-19T15:46:26Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T15:51:27Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T15:51:27Z [watchdog] spawned resume.sh (pid 58953)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 32
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              96d2303

Starting ./scripts/run-phase.sh 2
2026-04-19T15:52:27Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T15:57:27Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T15:58:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T16:03:30Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T16:04:32Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T16:09:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T16:10:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T16:15:35Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T16:16:37Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T16:21:37Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T16:22:39Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T16:27:40Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
