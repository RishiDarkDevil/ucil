---
timestamp: 2026-04-24T21:31:16Z
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
Main HEAD:              6164622

Starting ./scripts/run-phase.sh 2
2026-04-24T20:56:04Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T21:01:04Z [watchdog] invoking scripts/resume.sh --yes
2026-04-24T21:01:04Z [watchdog] spawned resume.sh (pid 47648)
Already up to date.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 337
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              6164622

Starting ./scripts/run-phase.sh 2
2026-04-24T21:02:04Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T21:07:05Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T21:08:07Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T21:13:08Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T21:14:10Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T21:19:10Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T21:20:12Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T21:25:13Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T21:26:15Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T21:31:16Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
