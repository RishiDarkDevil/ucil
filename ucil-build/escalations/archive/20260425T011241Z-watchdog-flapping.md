---
timestamp: 2026-04-25T01:12:41Z
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
2026-04-25T00:30:24Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T00:30:24Z [watchdog] spawned resume.sh (pid 75440)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 358
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              5452cc8

Starting ./scripts/run-phase.sh 2
2026-04-25T00:31:24Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T00:36:25Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T00:37:26Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T00:42:27Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T00:43:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T00:48:30Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T00:49:32Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T00:54:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T00:55:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T01:00:35Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T01:01:37Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T01:06:38Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T01:07:40Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T01:12:41Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
