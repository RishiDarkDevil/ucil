---
timestamp: 2026-04-23T18:17:36Z
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
2026-04-23T17:36:22Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T17:41:22Z [watchdog] invoking scripts/resume.sh --yes
2026-04-23T17:41:22Z [watchdog] spawned resume.sh (pid 15365)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 248
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              fcc7ca0

Starting ./scripts/run-phase.sh 2
2026-04-23T17:42:23Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T17:47:23Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T17:48:25Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T17:53:26Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T17:54:28Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T17:59:28Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T18:00:30Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T18:05:31Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T18:06:33Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T18:11:34Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T18:12:36Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T18:17:36Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
