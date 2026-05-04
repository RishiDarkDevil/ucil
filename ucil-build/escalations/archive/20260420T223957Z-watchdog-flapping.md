---
timestamp: 2026-04-20T22:39:57Z
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
Work-orders on disk:    41
Unresolved escalations: 141
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              7e8ff7d

Starting ./scripts/run-phase.sh 2
2026-04-20T22:22:53Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T22:27:53Z [watchdog] invoking scripts/resume.sh --yes
2026-04-20T22:27:53Z [watchdog] spawned resume.sh (pid 48617)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 141
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              7e8ff7d

Starting ./scripts/run-phase.sh 2
2026-04-20T22:28:54Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T22:33:54Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T22:34:56Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T22:39:57Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
