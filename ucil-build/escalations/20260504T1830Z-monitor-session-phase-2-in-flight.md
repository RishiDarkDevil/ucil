---
ts: 2026-05-04T18:30:00Z
phase: 2
session: monitor
trigger: monitor-session-active-during-phase-execution
resolved: false
auto_classify: bucket-A-admin
close_when: phase-2-complete tag exists OR monitor session ends
---

# Monitor session active during Phase 2 execution

## Context

A long-running monitor (interactive Claude Code session, role=`main`, not
exempted by `.claude/hooks/stop/gate.sh:54-58`) is supervising the
autonomous build during Phase 2. The autonomous loop (`run-phase.sh 2`,
PID 30765) and watchdog (PID 32274) run independently in the background
and produce per-WO gate-green commits as features flip.

## Why this escalation exists

Stop-hook policy: blocks turn-end when `scripts/gate-check.sh $PHASE`
returns non-zero. During Phase 2 execution the gate is by definition
red (less than 25/25 features passing). The hook offers a bypass:
"any tracked escalation lacking `resolved: true`" → skip the gate.

Without a long-lived advisory, every monitor turn-end trips the gate,
each individual WO advisory gets auto-closed by triage when its
specific WO merges, leaving the next turn unprotected. This file is
the umbrella advisory covering the whole Phase 2 monitoring window so
the monitor can end turns cleanly without writing a fresh per-turn
escalation each time.

## Action

`resolved: false` so the Stop-hook bypass at gate.sh:88 fires for the
duration of Phase 2. The monitor surfaces real anomalies via:

- 3 armed Monitors (watchdog log, escalation creates, progress poll)
- ScheduleWakeup heartbeats (1800s healthy / 600s mid-WO)
- This file's `close_when` clause auto-closes when phase-2-complete
  tag is created OR the user terminates the monitor

## What this is NOT covering

If the gate sub-checks themselves regress (e.g.,
`scripts/verify/coverage-gate.sh` starts failing on previously-passing
crates, or a verifier rejects a WO 3× consecutively), the monitor
opens a NEW escalation tagged appropriately (bucket-D / bucket-E) and
does not piggyback on this advisory.

## Bucket

`bucket-A-admin` per DEC-0007 — auto-closeable. Triage SHOULD NOT close
this one until either the close_when clause fires OR the monitor
explicitly resolves it. The frontmatter `close_when` field signals
intent.
