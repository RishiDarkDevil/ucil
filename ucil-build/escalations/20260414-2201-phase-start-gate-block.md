---
timestamp: 2026-04-14T22:01Z
phase: 0
week: 1
raised_by: claude-code (main session)
severity: harness-config
blocks_loop: false
---

# Stop-hook gate-check blocks `/phase-start` turn

## Context

User invoked `/phase-start 0` at the very start of phase 0. The skill's
documented job is to get the planner moving — synthesize
`phase-log/00-phase-0/CLAUDE.md` and emit WO-0001. Both happened cleanly
(commits `fb25b10`, `ca0fe20`, pushed to `origin/main`).

## Symptom

Stop-hook refuses to end the session because `scripts/gate-check.sh 0`
reports:

```
[FAIL] Unfinished features in phase 0:
P0-W1-F01,P0-W1-F02,...,P0-W1-F14
```

This is structurally correct — 0/14 features pass, gate cannot be
green. But it's structurally impossible to make it green on a
phase-start turn: the planner just emitted the first work-order;
implementation spans many executor/critic/verifier sessions over the
remainder of Week 1.

## Root cause (hypothesis)

The Stop-hook appears to fire `gate-check.sh $PHASE` unconditionally at
end-of-turn and treat failure as session-blocking. Per
`ucil-build/CLAUDE.md` §"Phase gate formula" and
`.claude/CLAUDE.md` §"Commit + push cadence", the gate is the bar for
**phase completion** / `/phase-ship`, not for every turn. On
non-terminal turns (planner-only, partial-executor, critic, etc.) the
tree-dirty and ahead-of-upstream checks should stand, but the gate
check should be skipped or downgraded to a warning.

## Work completed this turn

- `ucil-build/phase-log/00-phase-0/CLAUDE.md` (62 lines, planner-synthesized from master plan §18 Phase 0)
- `ucil-build/work-orders/0001-workspace-skeleton.json` (WO-0001: P0-W1-F01 + P0-W1-F10)
- `scripts/seed-features-chunked.sh` + `scripts/verify/*.sh` (seeding leftovers, tracked so verifier runs can find them)
- Commits: `fb25b10`, `ca0fe20` pushed to `origin/main`

## Proposed resolution

Pick whichever the user prefers — I should not edit Stop-hook config
without confirmation.

**Option A — skip gate-check at phase-start** (recommended)
Teach the Stop-hook to skip the gate when
`git log -1 --format=%s HEAD` starts with `docs(phase-`, when the only
changes this turn are under `ucil-build/phase-log/`, `ucil-build/work-orders/`,
or `ucil-build/decisions/`, or when a sentinel file
`ucil-build/.phase-start-grace` exists.

**Option B — gate only on `/phase-ship`**
Remove gate-check from Stop-hook entirely; fire it only from the
`phase-ship` skill. Matches the documented invariant that the gate is
the ship bar.

**Option C — explicit escalation bypass** (what this file does)
Stop-hook allows the session to end if a fresh escalation file is
present in `ucil-build/escalations/` and is tracked/committed.

## Ask of the user

Merge this escalation, then either:
1. Edit `.claude/hooks/stop-*.sh` (or equivalent) to implement Option A/B.
2. Or leave the bypass in place and run `scripts/run-phase.sh` — the
   outer loop will drive planner → executor → critic → verifier until
   the gate is genuinely green, at which point `/phase-ship 0`
   completes the phase cleanly.

## Not blocked on

- Code: nothing. WO-0001 is ready for an executor.
- Spec: nothing. Phase 0 scope is clear.
- External deps: nothing. No docker needed in P0.

This is a harness-ergonomics issue only. The build pipeline is healthy.
