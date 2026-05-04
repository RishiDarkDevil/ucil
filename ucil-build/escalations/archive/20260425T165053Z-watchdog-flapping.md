---
timestamp: 2026-04-25T16:50:53Z
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
2026-04-25T16:09:38Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T16:14:39Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T16:14:39Z [watchdog] spawned resume.sh (pid 111931)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 450
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              c94a13b

Starting ./scripts/run-phase.sh 2
2026-04-25T16:15:39Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T16:20:39Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T16:21:41Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T16:26:42Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T16:27:45Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T16:32:46Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T16:33:47Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T16:38:48Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T16:39:50Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T16:44:51Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T16:45:53Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T16:50:53Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
