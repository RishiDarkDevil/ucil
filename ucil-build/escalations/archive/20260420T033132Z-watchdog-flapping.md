---
timestamp: 2026-04-20T03:31:32Z
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
[resume] Pushing main (21 commits ahead of upstream)
fatal: unable to access 'https://github.com/RishiDarkDevil/ucil.git/': Failed to connect to github.com port 443 after 6 ms: Could not connect to server
[_retry] 'git push origin main' failed (rc=128, attempt 3/3); giving up.
fatal: unable to access 'https://github.com/RishiDarkDevil/ucil.git/': Failed to connect to github.com port 443 after 6 ms: Could not connect to server
[_retry] 'git pull --ff-only' failed (rc=1, attempt 3/3); giving up.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 109
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              fae0a9b

Starting ./scripts/run-phase.sh 2
2026-04-20T02:56:15Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T03:01:16Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T03:02:16Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T03:07:17Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T03:08:18Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T03:13:18Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T03:14:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T03:19:29Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T03:20:30Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T03:25:30Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T03:26:31Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T03:31:32Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
