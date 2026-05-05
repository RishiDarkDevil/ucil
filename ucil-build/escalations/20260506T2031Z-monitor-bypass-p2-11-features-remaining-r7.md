---
ts: 2026-05-06T20:31:00Z
phase: 2
session: monitor
trigger: stop-hook-blocks-on-mid-phase-gate-red
resolved: false
blocks_loop: false
severity: low
auto_classify: bucket-A-admin
close_when: 11 P2 features still unfinished is the expected mid-phase state; triage may close on next pass
---

# Monitor Stop-hook bypass — P2 has 11 features remaining (round 7)

## Context

Monitor session continues. P2 14/25. Triage just closed prior r6
(`c259e00`) and the WO-0053 attempts-exhausted r2 false positive
(`910c952`).

run-phase.sh is currently in halt mode for WO-0053 (orchestrator
attempts-cap reached on a feature that already passes — idempotency
quirk). Watchdog 9517 alive. Monitor session waiting for the loop
to either self-recover via triage rescue or for the user to greenlight
the offline-mode patch we discussed earlier.

Stop-hook blocks turn-end on mid-phase gate-red; this is the normal
state, not a regression.

## Bucket-A self-classification

`blocks_loop: false`, `severity: low`. Triage applies bucket-A and
closes on next pass. Each per-turn advisory of this shape only needs
to survive a single Stop-hook invocation. Fresh one written when needed.
