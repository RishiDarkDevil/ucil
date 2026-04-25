---
timestamp: 2026-04-25T04:13:45Z
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
2026-04-25T03:31:28Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T03:31:28Z [watchdog] spawned resume.sh (pid 102827)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 379
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              0fe5cd4

Starting ./scripts/run-phase.sh 2
2026-04-25T03:32:28Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T03:37:29Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T03:38:31Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T03:43:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T03:44:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T03:49:34Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T03:50:36Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T03:55:37Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T03:56:39Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T04:01:40Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T04:02:42Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T04:07:42Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T04:08:44Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T04:13:45Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
