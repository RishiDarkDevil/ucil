---
timestamp: 2026-04-25T10:48:27Z
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
2026-04-25T10:07:08Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T10:12:09Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T10:12:09Z [watchdog] spawned resume.sh (pid 41804)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 408
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              06589f3

Starting ./scripts/run-phase.sh 2
2026-04-25T10:13:09Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T10:18:10Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T10:19:13Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T10:24:13Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T10:25:18Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T10:30:19Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T10:31:21Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T10:36:22Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T10:37:24Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T10:42:24Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T10:43:26Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T10:48:27Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
