---
id: DEC-0007
title: Remove cargo-mutants mutation-gate from per-WO verifier cycle
date: 2026-04-17
status: accepted
supersedes_partial: worker-A-quality-gates-wiring
raised_by: user
---

# DEC-0007: Remove cargo-mutants from per-WO verifier gate

## Context

Worker A (audit pass, 2026-04-17 early hours) wired `scripts/verify/mutation-gate.sh`
— a `cargo-mutants` wrapper enforcing a 70% mutation-kill-rate floor — into:

- Every phase gate `scripts/gate/phase-{1..8}.sh`
- The verifier agent's workflow step 7a (`.claude/agents/verifier.md`)

This was added as "Tier 1.1" of a 3-tier quality-gate audit. It was **not**
in the original master plan; the master plan's "mutation check" refers to
the stash-and-run style at `scripts/reality-check.sh` (which remains in
force as verifier workflow step 6b).

## Observed failure

WO-0008 (`daemon-lifecycle-session-state`, P1-W3-F01 + P1-W4-F07) was
rejected 3× by the verifier on the per-WO mutation-gate. Verifier findings
(per `ucil-build/triage-log.md` and rejection commit `b512bfb`):

- **Functional tests: GREEN** — lifecycle::* 7/7, session_manager::test_session_state_tracking 1/1
- **Clippy: clean**
- **Coverage gate (line 85% / branch 75%): not reported as failing**
- **Reality-check (stash-based mutation): passed**
- **`cargo-mutants` score: 41% on `ucil-daemon`** — 7 surviving mutants on `session_manager.rs`, 5 concentrated on the `test_session_state_tracking` path

The surviving mutants were boundary-condition variants (`>` vs `>=` in TTL
expiry math, default-value swaps in new `SessionInfo` fields) that are
legitimately hard to kill with functional assertions. Each retry incurred
a full `cargo clean + cargo-mutants` cycle (~10–20 min wall-clock, ~20k
API tokens).

## Decision

**Remove `scripts/verify/mutation-gate.sh` invocations from `scripts/gate/phase-1.sh`
through `scripts/gate/phase-7.sh` and from `.claude/agents/verifier.md`
step 7a.**

Retain as a **Phase 8 release-one-shot** only, at a relaxed 50% floor
(not 70%). The script itself (`scripts/verify/mutation-gate.sh`) stays in
the repo — just not wired into the per-WO verifier cycle or phase-<8 gates.

## What anti-laziness coverage remains

| Layer | Mechanism | Catches |
|---|---|---|
| 1 | `scripts/reality-check.sh` (verifier step 6b) | Tests that don't actually exercise the code (stash → fail; pop → pass) |
| 2 | Critic adversarial review (step 3/4 pre-verifier) | `todo!()`, `unimplemented!()`, mocks of critical deps, `.skip`, `#[ignore]`, weak assertions |
| 3 | `scripts/verify/coverage-gate.sh` (verifier step 7a — now the only quality gate) | < 85% line coverage, < 75% branch (nightly-only) |
| 4 | Fresh-session verifier cycle | Non-reproducible local-only green |
| 5 | `scripts/verify/effectiveness-gate.sh` per phase | Scenarios where UCIL underperforms grep+Read baseline |

Layers 1–3 are blocking at WO-time. Layer 4 is structural (session-id
enforcement in `flip-feature.sh`). Layer 5 is per-phase-gate.

Layer 5-equivalent for mutation testing = `cargo-mutants` as a **Phase 8
one-shot** (the only place it still runs). At Phase 8 we have enough
matured code + tests that a holistic 50% mutation score is a meaningful
release bar, not a per-WO obstacle.

## Consequences

- **WO-0008 unblocks**: 20260416-2146-wo-WO-0008-attempts-exhausted.md
  can be marked `resolved: true`; next verifier pass will green-flip
  P1-W3-F01 + P1-W4-F07 (functional tests already pass, coverage gate
  reportedly passed).
- **Verifier cycle-time drops**: per-WO cycle reduces from ~25 min to
  ~5–8 min (no cargo-mutants run). Token spend per WO roughly halves.
- **Aspirational mutation signal lost at WO-time**: a future Phase-2 WO
  that ships a function with trivially killable mutants won't be caught
  by the per-WO gate. The reality-check + critic + coverage triad
  catches most of this; whatever slips through is caught at Phase 8.
- **Pattern-matching with master plan**: this aligns the harness with
  what the plan actually said ("Mutation check: stash the feature's code
  → tests fail → pop → tests pass") rather than what Worker A inferred.

## Revisit trigger

Re-add per-crate `cargo-mutants` gate at Phase 5 or 7 entry IF:
- ≥ 3 WOs in a single phase are found to have silently-broken tests
  (implementation swap / typo that tests didn't catch) that a mutation
  gate would have flagged, AND
- The cost (measured in Opus tokens + wall-clock) of cargo-mutants per
  crate has dropped by ≥ 50% (via faster cargo-mutants versions or
  narrower `--file` scoping per WO diff).

Otherwise, keep as Phase 8 one-shot.
