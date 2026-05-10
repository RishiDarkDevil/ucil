---
timestamp: 2026-05-10T07:02:44Z
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
Work-orders on disk:    94
Unresolved escalations: 10
Open rejections:        15
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              556060f

Starting ./scripts/run-phase.sh 3
2026-05-10T06:45:34Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T06:50:35Z [watchdog] invoking scripts/resume.sh --yes
2026-05-10T06:50:35Z [watchdog] spawned resume.sh (pid 20928)
Already up to date.

=== Resume summary ===
Phase:                  3
Features passing:       118 / 234
Work-orders on disk:    94
Unresolved escalations: 10
Open rejections:        15
Orphans killed:         0
Worktrees auto-stashed: 0
Corrupt WOs quarantined:0
Main HEAD:              556060f

Starting ./scripts/run-phase.sh 3
2026-05-10T06:51:35Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T06:56:35Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
2026-05-10T06:57:43Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-10T07:02:44Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```

## Resolution

Self-inflicted noise from a flap loop on 2026-05-10. The phase-3 strict gate has 3 known source gaps (see harness-fixer halts at 1027Z+1030Z); run-phase exits 1 on each respawn because those gaps are unresolved, watchdog respawns, escalation repeats. Watchdog killed manually 2026-05-10T11:30Z; the underlying source gaps still need user action but THIS heartbeat is closed.

resolved: true
