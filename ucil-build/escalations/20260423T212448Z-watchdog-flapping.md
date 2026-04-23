---
timestamp: 2026-04-23T21:24:48Z
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
2026-04-23T20:42:32Z [watchdog] invoking scripts/resume.sh --yes
2026-04-23T20:42:32Z [watchdog] spawned resume.sh (pid 52022)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 269
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              4f0223a

Starting ./scripts/run-phase.sh 2
2026-04-23T20:43:32Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T20:48:33Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T20:49:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T20:54:35Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T20:55:37Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T21:00:38Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T21:01:40Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T21:06:40Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T21:07:42Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T21:12:43Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T21:13:45Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T21:18:45Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T21:19:47Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T21:24:48Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
