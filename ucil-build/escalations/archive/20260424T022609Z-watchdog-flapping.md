---
timestamp: 2026-04-24T02:26:09Z
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
[_retry] 'git push origin main' failed (rc=128, attempt 3/3); giving up.
fatal: unable to access 'https://github.com/RishiDarkDevil/ucil.git/': Failed to connect to github.com port 443 after 5 ms: Could not connect to server
[_retry] 'git pull --ff-only' failed (rc=1, attempt 3/3); giving up.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 304
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              f79b752

Starting ./scripts/run-phase.sh 2
2026-04-24T01:45:02Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T01:50:02Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T01:51:03Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T01:56:03Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T01:57:04Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T02:02:04Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T02:03:05Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T02:08:05Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T02:09:06Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T02:14:06Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T02:15:07Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T02:20:08Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-24T02:21:08Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-24T02:26:09Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
