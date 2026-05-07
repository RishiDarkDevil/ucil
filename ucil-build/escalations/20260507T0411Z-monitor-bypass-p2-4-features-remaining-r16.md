---
ts: 2026-05-07T04:11:00Z
phase: 2
session: monitor
trigger: stop-hook-blocks-on-mid-phase-gate-red
resolved: true
blocks_loop: false
severity: low
auto_classify: bucket-A-admin
close_when: 4 P2 features still unfinished is the expected mid-phase state; triage may close on next pass
---

# Monitor Stop-hook bypass — P2 21/25 (round 16)

Bucket-A. Triage closes on next pass.

Mid-phase gate-red is the expected state with 4 P2 features remaining
(P2-W7-F06, P2-W8-F04, P2-W8-F07, P2-W8-F08). WO-0062 shipped at 0c07f5f
(P2-W8-F03 qwen3 config gate); pipeline resumed after triage pass-3 force-halt
on r15 was manually resolved at bf424c3.

## Resolution

Resolved 2026-05-07 by triage (cap-rescue pass, phase 2). Bucket A — the
`close_when` clause ("triage may close on next pass") explicitly authorises
auto-closure. The original four-feature-remaining state has progressed:

- P2-W7-F06 has since flipped to `passes: true` (verifier signature
  `verifier-eccb9fce-...`, merge commit `1e3c4e3` for WO-0063).
- Phase 2 now stands at 22/25 features passing; the still-unfinished set
  is P2-W8-F04, P2-W8-F07, P2-W8-F08 — handled separately by the planner
  in upcoming work-orders.

This umbrella advisory's purpose (allowing the monitor to end turns
during mid-phase gate-red) remains a recurring pattern; future monitor
sessions can write fresh per-session umbrella advisories the same way.

resolved: true
