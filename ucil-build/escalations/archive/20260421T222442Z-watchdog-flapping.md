---
timestamp: 2026-04-21T22:24:42Z
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
2026-04-21T21:43:28Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T21:48:28Z [watchdog] invoking scripts/resume.sh --yes
2026-04-21T21:48:28Z [watchdog] spawned resume.sh (pid 76398)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 187
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              426f3f3

Starting ./scripts/run-phase.sh 2
2026-04-21T21:49:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T21:54:29Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T21:55:31Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T22:00:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T22:01:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T22:06:34Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T22:07:36Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T22:12:37Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T22:13:38Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T22:18:39Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T22:19:41Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T22:24:42Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
