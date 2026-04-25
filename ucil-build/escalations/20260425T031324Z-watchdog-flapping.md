---
timestamp: 2026-04-25T03:13:24Z
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
2026-04-25T02:31:07Z [watchdog] invoking scripts/resume.sh --yes
2026-04-25T02:31:07Z [watchdog] spawned resume.sh (pid 93948)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 372
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              74383f7

Starting ./scripts/run-phase.sh 2
2026-04-25T02:32:07Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T02:37:08Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T02:38:10Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T02:43:10Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T02:44:12Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T02:49:13Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T02:50:15Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T02:55:16Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T02:56:18Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T03:01:18Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T03:02:20Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T03:07:21Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-25T03:08:23Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-25T03:13:24Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
