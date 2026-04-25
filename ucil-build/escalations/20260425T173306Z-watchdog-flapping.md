---
timestamp: 2026-04-25T17:33:06Z
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
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              b1c9398

Starting ./scripts/run-phase.sh 2
2026-04-25T17:09:59Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T17:15:00Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T17:15:00Z [watchdog] spawned resume.sh (pid 123441)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 457
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              b1c9398

Starting ./scripts/run-phase.sh 2
2026-04-25T17:16:00Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T17:21:01Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T17:22:03Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T17:27:03Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T17:28:05Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T17:33:06Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
