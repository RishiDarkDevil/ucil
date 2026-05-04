---
timestamp: 2026-04-25T21:46:32Z
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
Main HEAD:              abf8b97

Starting ./scripts/run-phase.sh 2
2026-04-25T21:11:20Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T21:16:21Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T21:16:21Z [watchdog] spawned resume.sh (pid 176426)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 485
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              abf8b97

Starting ./scripts/run-phase.sh 2
2026-04-25T21:17:21Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T21:22:22Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T21:23:24Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T21:28:25Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T21:29:26Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T21:34:27Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T21:35:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T21:40:30Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T21:41:32Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T21:46:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
