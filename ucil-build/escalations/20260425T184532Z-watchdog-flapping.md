---
timestamp: 2026-04-25T18:45:32Z
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
Main HEAD:              7b5a843

Starting ./scripts/run-phase.sh 2
2026-04-25T18:10:20Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T18:15:21Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T18:15:21Z [watchdog] spawned resume.sh (pid 136908)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 464
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              7b5a843

Starting ./scripts/run-phase.sh 2
2026-04-25T18:16:21Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T18:21:22Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T18:22:23Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T18:27:24Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T18:28:26Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T18:33:27Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T18:34:28Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T18:39:29Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T18:40:31Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T18:45:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
