---
timestamp: 2026-04-20T22:09:49Z
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
2026-04-20T21:27:33Z [watchdog] invoking scripts/resume.sh --yes
2026-04-20T21:27:33Z [watchdog] spawned resume.sh (pid 37628)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 134
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              4c3eb29

Starting ./scripts/run-phase.sh 2
2026-04-20T21:28:33Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T21:33:33Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T21:34:35Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T21:39:36Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T21:40:38Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T21:45:39Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T21:46:41Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T21:51:41Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T21:52:43Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T21:57:44Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T21:58:46Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T22:03:47Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T22:04:49Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T22:09:49Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
