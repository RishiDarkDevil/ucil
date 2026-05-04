---
timestamp: 2026-04-24T19:36:37Z
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
2026-04-24T18:55:22Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T19:00:22Z [watchdog] invoking scripts/resume.sh --yes
2026-04-24T19:00:22Z [watchdog] spawned resume.sh (pid 19274)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 323
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              30ee9d5

Starting ./scripts/run-phase.sh 2
2026-04-24T19:01:22Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T19:06:23Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T19:07:26Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T19:12:26Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T19:13:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T19:18:29Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T19:19:31Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T19:24:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T19:25:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T19:30:34Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T19:31:36Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T19:36:37Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
