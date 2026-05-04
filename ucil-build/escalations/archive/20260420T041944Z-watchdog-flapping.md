---
timestamp: 2026-04-20T04:19:44Z
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
2026-04-20T03:50:35Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T03:55:35Z [watchdog] invoking scripts/resume.sh --yes
2026-04-20T03:55:35Z [watchdog] spawned resume.sh (pid 204412)

[resume] Pushing main (28 commits ahead of upstream)
fatal: unable to access 'https://github.com/RishiDarkDevil/ucil.git/': Failed to connect to github.com port 443 after 6 ms: Could not connect to server
[_retry] 'git push origin main' failed (rc=128, attempt 3/3); giving up.
fatal: unable to access 'https://github.com/RishiDarkDevil/ucil.git/': Failed to connect to github.com port 443 after 6 ms: Could not connect to server
[_retry] 'git pull --ff-only' failed (rc=1, attempt 3/3); giving up.

=== Resume summary ===
Phase:                  2
Features passing:       48 / 234
Work-orders on disk:    41
Unresolved escalations: 116
Open rejections:        8
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              7391909

Starting ./scripts/run-phase.sh 2
2026-04-20T03:56:35Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T04:01:36Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T04:02:38Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T04:07:39Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T04:08:41Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T04:13:41Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-04-20T04:14:43Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-04-20T04:19:44Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
