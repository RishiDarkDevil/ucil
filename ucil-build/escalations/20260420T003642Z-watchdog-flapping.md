---
timestamp: 2026-04-20T00:36:42Z
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
2026-04-19T23:54:30Z [watchdog] invoking scripts/resume.sh --yes
2026-04-19T23:54:30Z [watchdog] spawned resume.sh (pid 158964)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 88
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              3203d37

Starting ./scripts/run-phase.sh 2
2026-04-19T23:55:30Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T00:00:31Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T00:01:36Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T00:06:37Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T00:07:37Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T00:12:38Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T00:13:38Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T00:18:39Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T00:19:40Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T00:24:40Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T00:25:41Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T00:30:41Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T00:31:42Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T00:36:42Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
