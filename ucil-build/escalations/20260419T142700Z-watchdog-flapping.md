---
timestamp: 2026-04-19T14:27:00Z
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
2026-04-19T13:45:45Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T13:50:46Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T13:50:46Z [watchdog] spawned resume.sh (pid 35446)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 18
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              48bf786

Starting ./scripts/run-phase.sh 2
2026-04-19T13:51:46Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T13:56:47Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T13:57:49Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T14:02:49Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T14:03:51Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T14:08:52Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T14:09:54Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T14:14:55Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T14:15:56Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T14:20:57Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T14:21:59Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T14:27:00Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
