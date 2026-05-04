---
timestamp: 2026-04-19T20:35:26Z
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
2026-04-19T19:52:47Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T19:52:47Z [watchdog] spawned resume.sh (pid 107882)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 60
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              9b76e55

Starting ./scripts/run-phase.sh 2
2026-04-19T19:53:48Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T19:58:48Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T19:59:50Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T20:04:51Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T20:05:53Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T20:10:53Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T20:11:55Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T20:16:56Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T20:17:58Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T20:22:59Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T20:24:01Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T20:29:23Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T20:30:25Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T20:35:26Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
