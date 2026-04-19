---
timestamp: 2026-04-19T18:16:15Z
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
Main HEAD:              ea85790

Starting ./scripts/run-phase.sh 2
2026-04-19T17:47:06Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T17:52:07Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T17:52:07Z [watchdog] spawned resume.sh (pid 82967)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 46
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              ea85790

Starting ./scripts/run-phase.sh 2
2026-04-19T17:53:07Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T17:58:08Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T17:59:10Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T18:04:10Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T18:05:12Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T18:10:13Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T18:11:15Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T18:16:15Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
