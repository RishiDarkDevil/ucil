---
timestamp: 2026-04-20T01:37:03Z
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
fatal: unable to access 'https://github.com/RishiDarkDevil/ucil.git/': Failed to connect to github.com port 443 after 6 ms: Could not connect to server
[_retry] 'git pull --ff-only' failed (rc=1, attempt 3/3); giving up.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 95
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              33587bd

Starting ./scripts/run-phase.sh 2
2026-04-20T00:55:45Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T01:00:46Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T01:01:46Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T01:06:47Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T01:07:47Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T01:12:48Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T01:13:59Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T01:18:59Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T01:20:00Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T01:25:00Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T01:26:01Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T01:31:01Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T01:32:02Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T01:37:03Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
