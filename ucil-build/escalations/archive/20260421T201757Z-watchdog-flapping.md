---
timestamp: 2026-04-21T20:17:57Z
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
Main HEAD:              8d965ac

Starting ./scripts/run-phase.sh 2
2026-04-21T19:42:46Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T19:47:46Z [watchdog] invoking scripts/resume.sh --yes
2026-04-21T19:47:46Z [watchdog] spawned resume.sh (pid 49352)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 173
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              8d965ac

Starting ./scripts/run-phase.sh 2
2026-04-21T19:48:46Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T19:53:47Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T19:54:49Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T19:59:50Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T20:00:52Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T20:05:52Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T20:06:54Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T20:11:55Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T20:12:57Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T20:17:57Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
