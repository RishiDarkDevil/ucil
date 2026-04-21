---
timestamp: 2026-04-21T17:29:00Z
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
2026-04-21T16:46:43Z [watchdog] invoking scripts/resume.sh --yes
2026-04-21T16:46:43Z [watchdog] spawned resume.sh (pid 12491)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 152
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              27b6b3a

Starting ./scripts/run-phase.sh 2
2026-04-21T16:47:43Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T16:52:44Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T16:53:46Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T16:58:47Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T16:59:49Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T17:04:49Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T17:05:51Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T17:10:52Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T17:11:54Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T17:16:55Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T17:17:57Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T17:22:57Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-21T17:24:00Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-21T17:29:00Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
