---
timestamp: 2026-05-10T11:18:38Z
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
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              d4344b1

Starting ./scripts/run-phase.sh 3
2026-05-10T10:49:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T10:54:30Z [watchdog] invoking scripts/resume.sh --yes
2026-05-10T10:54:30Z [watchdog] spawned resume.sh (pid 55198)
Already up to date.

=== Resume summary ===
Phase:                  3
Features passing:       118 / 234
Work-orders on disk:    94
Unresolved escalations: 38
Open rejections:        15
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              d4344b1

Starting ./scripts/run-phase.sh 3
2026-05-10T10:55:30Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T11:00:30Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T11:01:32Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T11:06:33Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T11:07:35Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T11:12:35Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T11:13:37Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T11:18:38Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
