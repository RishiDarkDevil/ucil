---
timestamp: 2026-04-19T15:33:22Z
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
2026-04-19T14:51:06Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T14:51:06Z [watchdog] spawned resume.sh (pid 47138)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 25
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              9629c80

Starting ./scripts/run-phase.sh 2
2026-04-19T14:52:06Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T14:57:07Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T14:58:09Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T15:03:10Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T15:04:12Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T15:09:12Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T15:10:14Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T15:15:15Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T15:16:17Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T15:21:17Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T15:22:19Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T15:27:20Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-19T15:28:22Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-19T15:33:22Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
