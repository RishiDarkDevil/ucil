---
timestamp: 2026-04-25T11:48:50Z
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
2026-04-25T11:07:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T11:12:34Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T11:12:34Z [watchdog] spawned resume.sh (pid 52459)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 415
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              b6d5369

Starting ./scripts/run-phase.sh 2
2026-04-25T11:13:35Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T11:18:35Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T11:19:38Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T11:24:39Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T11:25:41Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T11:30:42Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T11:31:44Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T11:36:45Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T11:37:47Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T11:42:48Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T11:43:50Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T11:48:50Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
