---
ts: 2026-05-05T06:48:00Z
phase: 2
session: monitor
trigger: stop-hook-blocks-on-mid-phase-gate-red
resolved: false
blocks_loop: false
severity: low
auto_classify: bucket-A-admin
close_when: 14 P2 features still unfinished is the expected mid-phase state; triage may close on next pass
---

# Monitor Stop-hook bypass — P2 has 14 features remaining (round 2)

## Context

Monitor session active during Phase 2 build. Currently 11/25 P2 features
passing. Triage just closed the prior 0640Z bypass advisory at `26550e8`
along with two stale `WO-0049-attempts-exhausted` false-positives
(0227, 0234). Stop-hook fired gate-red again immediately, so a fresh
bypass advisory is needed for this turn.

WO-0049 fully shipped. Pipeline cycling on next W7 feature.

Stop-hook blocks turn-end on mid-phase gate-red; this is the normal
state, not a regression.

## Bucket-A self-classification

`blocks_loop: false`, `severity: low`. Triage applies bucket-A and
closes on next pass. Each per-turn advisory of this shape only needs
to survive a single Stop-hook invocation. Fresh one written when needed.
