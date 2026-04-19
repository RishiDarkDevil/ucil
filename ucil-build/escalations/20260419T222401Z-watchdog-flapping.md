---
timestamp: 2026-04-19T22:24:01Z
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
Main HEAD:              51b4a6b

Starting ./scripts/run-phase.sh 2
2026-04-19T21:48:49Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T21:53:50Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T21:53:50Z [watchdog] spawned resume.sh (pid 136027)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 74
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              51b4a6b

Starting ./scripts/run-phase.sh 2
2026-04-19T21:54:50Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T21:59:51Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T22:00:53Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T22:05:53Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T22:06:55Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T22:11:56Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T22:12:58Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T22:17:58Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T22:19:00Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T22:24:01Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
