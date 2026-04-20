---
timestamp: 2026-04-20T02:31:12Z
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
[resume] Pushing main (14 commits ahead of upstream)
fatal: unable to access 'https://github.com/RishiDarkDevil/ucil.git/': Failed to connect to github.com port 443 after 7 ms: Could not connect to server
[_retry] 'git push origin main' failed (rc=128, attempt 3/3); giving up.
fatal: unable to access 'https://github.com/RishiDarkDevil/ucil.git/': Failed to connect to github.com port 443 after 7 ms: Could not connect to server
[_retry] 'git pull --ff-only' failed (rc=1, attempt 3/3); giving up.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 102
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              912e85d

Starting ./scripts/run-phase.sh 2
2026-04-20T01:56:05Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T02:01:06Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T02:02:06Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T02:07:07Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T02:08:08Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T02:13:08Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T02:14:09Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T02:19:09Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T02:20:10Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T02:25:10Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T02:26:11Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T02:31:12Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
