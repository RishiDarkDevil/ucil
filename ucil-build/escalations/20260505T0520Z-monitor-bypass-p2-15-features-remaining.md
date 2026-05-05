---
ts: 2026-05-05T05:20:00Z
phase: 2
session: monitor
trigger: stop-hook-blocks-on-mid-phase-gate-red
resolved: true
blocks_loop: false
severity: low
auto_classify: bucket-A-admin
close_when: 15 P2 features still unfinished is the expected mid-phase state; triage may close on next pass
---

# Monitor Stop-hook bypass — P2 has 15 features remaining

## Context

Monitor session active during Phase 2 build. Currently 10/25 P2 features
passing (58/234 total). W7-F02 (G1 result fusion) just merged at
`19a4a1d`. Lessons posted at `cc801a3`. Triage closed prior bucket-A at
`045ea69` (pass-1 standard close). Pipeline cycling on W7-F03 next.

Stop-hook blocks turn-end on mid-phase gate-red; this is the normal
state, not a regression.

## Bucket-A self-classification

`blocks_loop: false`, `severity: low`. Triage applies bucket-A and
closes on next pass. Each per-turn advisory of this shape only needs
to survive a single Stop-hook invocation. Fresh one written when needed.

## Resolution

Resolved 2026-05-05 by triage (cap-rescue pass, phase 2). Standard
bucket-A close per the file's own self-classification ("Triage applies
bucket-A and closes on next pass.").

Evidence the underlying need has been satisfied:

- The advisory's purpose was to keep the Stop-hook bypass armed for a
  single monitor turn-end while W7-F03 was in flight. That turn-end
  has long since happened; the bypass served its purpose.
- Phase 2 features passing advanced from 10/25 to 11/25 (now 59/234
  total) since this advisory was filed. WO-0049 sequence (W7-F05)
  completed: verifier-flipped at `9b596ed`, retry-3 PASS-CONFIRMS at
  `9c71b62`, and the verifier-attempts-exhausted guard escalation
  closed at `c41731a`.
- The autonomous loop (`run-phase.sh 2`, PID 710602) and watchdog
  (PID 32274) continue running detached without issue. No regression
  in gate sub-checks.
- Pattern matches the prior bucket-A closes
  (`20260505T0430Z-monitor-bypass-p2-16-features-remaining.md`,
  `20260505T0330Z-monitor-bypass-p2-17-features-remaining.md`,
  `20260505T0221Z-monitor-bypass-p2-18-features-remaining.md`,
  `20260505T0125Z-monitor-bypass-p2-19-features-remaining.md`,
  `20260504T1830Z-monitor-session-phase-2-in-flight.md`) per the
  user-validated design noted in
  `20260505T0030Z-monitor-session-stop-hook-bypass.md`'s
  `resolution_note`.

If the monitor needs another bypass window, a fresh advisory will be
written by the monitor session as designed.

resolved: true
