---
timestamp: 2026-04-19T21:35:46Z
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
2026-04-19T20:53:30Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T20:53:30Z [watchdog] spawned resume.sh (pid 124088)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 67
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              6b4d80e

Starting ./scripts/run-phase.sh 2
2026-04-19T20:54:30Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T20:59:31Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T21:00:33Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T21:05:33Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T21:06:35Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T21:11:36Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T21:12:38Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T21:17:38Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T21:18:40Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T21:23:41Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T21:24:43Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T21:29:43Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T21:30:45Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T21:35:46Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
