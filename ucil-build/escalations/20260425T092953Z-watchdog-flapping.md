---
timestamp: 2026-04-25T09:29:53Z
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
Main HEAD:              688b66d

Starting ./scripts/run-phase.sh 2
2026-04-25T09:04:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T09:09:30Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T09:09:30Z [watchdog] spawned resume.sh (pid 31674)
2026-04-25T09:10:30Z [watchdog] loop appears dead; entering 300s quiesce before restart
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 401
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              688b66d

Starting ./scripts/run-phase.sh 2
2026-04-25T09:15:31Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T09:16:38Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T09:21:38Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T09:24:52Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T09:29:53Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
