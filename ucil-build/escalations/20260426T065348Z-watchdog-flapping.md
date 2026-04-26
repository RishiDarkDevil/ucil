---
timestamp: 2026-04-26T06:53:48Z
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
2026-04-26T06:11:26Z [watchdog] invoking scripts/resume.sh --yes
2026-04-26T06:11:26Z [watchdog] spawned resume.sh (pid 21895)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 501
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              4827b05

Starting ./scripts/run-phase.sh 2
2026-04-26T06:12:27Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T06:17:27Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T06:18:31Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T06:23:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T06:24:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T06:29:35Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T06:30:36Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T06:35:37Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T06:36:42Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T06:41:43Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T06:42:45Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T06:47:46Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-26T06:48:47Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-26T06:53:48Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
