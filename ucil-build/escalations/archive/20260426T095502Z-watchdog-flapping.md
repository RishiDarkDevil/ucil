---
timestamp: 2026-04-26T09:55:02Z
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
2026-04-26T09:12:40Z [watchdog] invoking scripts/resume.sh --yes
2026-04-26T09:12:40Z [watchdog] spawned resume.sh (pid 55166)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 522
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              2398042

Starting ./scripts/run-phase.sh 2
2026-04-26T09:13:40Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T09:18:41Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T09:19:43Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T09:24:43Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T09:25:45Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T09:30:46Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T09:31:48Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T09:36:49Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T09:37:50Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T09:42:51Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T09:43:58Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T09:48:59Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T09:50:02Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T09:55:02Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
