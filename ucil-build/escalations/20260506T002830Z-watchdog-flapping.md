---
timestamp: 2026-05-06T00:28:30Z
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
2026-05-05T23:49:24Z [watchdog] loop came back on its own; no restart needed
2026-05-06T00:05:27Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-06T00:10:28Z [watchdog] invoking scripts/resume.sh --yes
2026-05-06T00:10:28Z [watchdog] spawned resume.sh (pid 899619)

[resume] Main tree has uncommitted changes:
?? ucil-build/decisions/DEC-0015-search-code-g2-fan-out-and-fused-meta-field.md

  Resolve before resuming (commit or reset).
  Refusing --yes with dirty main tree.
2026-05-06T00:11:28Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-06T00:16:28Z [watchdog] invoking scripts/resume.sh --yes
2026-05-06T00:16:28Z [watchdog] spawned resume.sh (pid 901126)

[resume] Main tree has uncommitted changes:
?? ucil-build/decisions/DEC-0015-search-code-g2-fan-out-and-fused-meta-field.md

  Resolve before resuming (commit or reset).
  Refusing --yes with dirty main tree.
2026-05-06T00:17:29Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-06T00:22:29Z [watchdog] invoking scripts/resume.sh --yes
2026-05-06T00:22:29Z [watchdog] spawned resume.sh (pid 902489)

[resume] Main tree has uncommitted changes:
?? ucil-build/decisions/DEC-0015-search-code-g2-fan-out-and-fused-meta-field.md

  Resolve before resuming (commit or reset).
  Refusing --yes with dirty main tree.
2026-05-06T00:23:30Z [watchdog] loop appears dead; entering 300s quiesce before restart
2026-05-06T00:28:30Z [watchdog] MAX_RESTARTS (3) hit within 3600s — escalating and exiting
```
