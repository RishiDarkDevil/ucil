---
timestamp: 2026-04-25T13:43:31Z
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
Main HEAD:              8b318ac

Starting ./scripts/run-phase.sh 2
2026-04-25T13:08:19Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T13:13:20Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T13:13:20Z [watchdog] spawned resume.sh (pid 73473)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 429
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              8b318ac

Starting ./scripts/run-phase.sh 2
2026-04-25T13:14:20Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T13:19:21Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T13:20:23Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T13:25:23Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T13:26:25Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T13:31:26Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T13:32:28Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T13:37:28Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T13:38:30Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T13:43:31Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
