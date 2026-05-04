---
timestamp: 2026-04-19T16:57:47Z
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
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 39
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              3789a06

Starting ./scripts/run-phase.sh 2
2026-04-19T16:46:46Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T16:51:47Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T16:51:47Z [watchdog] spawned resume.sh (pid 69799)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 39
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              3789a06

Starting ./scripts/run-phase.sh 2
2026-04-19T16:52:47Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T16:57:47Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
