---
timestamp: 2026-04-23T19:24:00Z
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
2026-04-23T18:41:43Z [watchdog] invoking scripts/resume.sh --yes
2026-04-23T18:41:43Z [watchdog] spawned resume.sh (pid 26739)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 255
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              23e6446

Starting ./scripts/run-phase.sh 2
2026-04-23T18:42:43Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T18:47:44Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T18:48:46Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T18:53:47Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T18:54:49Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T18:59:49Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T19:00:51Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T19:05:52Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T19:06:54Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T19:11:54Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T19:12:56Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T19:17:57Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-23T19:18:59Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-23T19:24:00Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
