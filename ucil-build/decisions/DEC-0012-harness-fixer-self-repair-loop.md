# DEC-0012: Harness-fixer agent closes the "no-owner-for-harness-bugs" gap

**Status**: accepted
**Date**: 2026-04-19
**Extends**: DEC-0009 (gate-side verification scripts), DEC-0011 (harness-config escalations)

## Context

On 2026-04-18/19 the phase-1 gate stayed red for ~4 hours on three
harness-script bugs — all sub-120-LOC, all in `scripts/verify/`:

1. `coverage-gate.sh` — `cargo llvm-cov report` errored on corrupt-header
   `.profraw` files from integration tests that spawn subprocesses.
   Fix: swap the one-shot invocation for a staged test+prune+report
   with `llvm-profdata show` validating each profraw file.
2. `multi-lang-coverage.sh` — script was a literal `exit 1` TODO
   placeholder. Fix: implement real MCP probes (~80 LOC) against the
   rust/python/typescript fixture projects.
3. `diagnostics-bridge.sh` — `pyright-langserver --stdio` over a FIFO
   was flaky (client had to advertise `publishDiagnostics` capability
   AND wait 15-25s for analysis). Fix: switch to pyright batch CLI
   (`pyright --outputjson`) — same analyzer, deterministic, 0.3s.

The harness failed to fix any of these autonomously. Each failure was
reported over and over (integration-tester wrote the same FAIL report
12+ times), but no agent applied a fix because:

- **Planner** only schedules work on `feature_ids` in
  `feature-list.json`. All 48 phase-1 features are `passes=true`, so
  planner's candidate-feature query returns empty → no WO emitted.
- **Integration-tester** is contractually forbidden from editing source
  (`.claude/agents/integration-tester.md` §Rules: "Do not edit source").
- **Root-cause-finder** only activates on WO-level verifier rejections,
  not on gate sub-check failures.
- **Triage** is instructed to apply bucket-B fixes only when the
  escalation includes a concrete diff. Monitor heartbeats and
  integration-tester failures are symptom-only, never prescriptive.
- **Triage pass-3 anti-thrashing rule** defaults everything to halt,
  which fired before bucket-B could operate.

This gap manifested as a pathological state: gate sub-checks fail →
triage halts or auto-resolves heartbeats → watchdog restarts
run-phase.sh → gate sub-checks fail again → loop.

## Decision

Create a new subagent — **harness-fixer** — whose single job is to
diagnose and patch failing harness scripts. Auto-invoke it from
`scripts/gate-check.sh` when `phase-N.sh` exits non-zero.

Write-scope (enforced by path-guards):
- `scripts/verify/*.sh`
- `scripts/gate/phase-*.sh` (bug fixes only, not structural changes)
- `scripts/_retry.sh`, `scripts/_watchdog.sh`
- `.githooks/*`

Out of scope (fallback to bucket-E escalation if touched):
- Anything under `crates/`, `adapters/`, `ml/`, `plugin*/`, `tests/*`
- `.claude/agents/*`, `.claude/settings.json`, `.claude/hooks/stop/gate.sh`
- `scripts/gate-check.sh`, `scripts/flip-feature.sh`
- `ucil-build/feature-list.json`, the master plan, existing ADRs

Hard limits per invocation:
- **120 LOC diff total** (matches bucket-B threshold from `.claude/agents/triage.md`)
- **3 iterations per failing script**
- If either cap is hit, write `type: harness-fixer-halt` escalation
  (`severity: high`, `blocks_loop: true`, `requires_planner_action: true`)
  with the investigation log.

## Consequences

- The harness now has end-to-end self-repair coverage for scripts it
  owns. No user intervention needed for sub-120-LOC harness bugs.
- The feature pipeline (planner → executor → critic → verifier) is
  unchanged. harness-fixer runs on a separate path — it's only
  invoked by the gate-check dispatcher, not by the work-order loop.
- `scripts/run-phase.sh` gate-green exit now creates the
  `phase-N-complete` git tag + advances `ucil-build/progress.json`
  to phase N+1. This eliminates the watchdog-flapping state where
  the gate was green but no tag existed, causing the watchdog to
  restart run-phase.sh in a tight loop.

## Rollback

`UCIL_SKIP_HARNESS_FIXER=1` in the environment short-circuits the
fixer invocation in `scripts/gate-check.sh` — use for debugging the
fixer itself. The harness-fixer agent file and launcher are
self-contained additions; removing them restores the prior behaviour.

## Revisit trigger

- If harness-fixer's 120-LOC cap proves too tight (fixes get halted
  just under the cap), re-evaluate per-invocation budget.
- If harness-fixer commits >5 scripts per phase, that's a signal the
  harness itself needs a refactor — escalate to planner for a
  phase-specific hardening WO.
- If harness-fixer starts touching paths outside its scope (the
  path-guards should block this, but if one slips through), tighten
  the contract in `.claude/agents/harness-fixer.md`.
