---
title: nav-rust-symbol — caller_completeness flips by 1 between consecutive runs (doctest caller stochasticity)
severity: harness-config
blocks_loop: false
requires_planner_action: false
filed_by: effectiveness-evaluator
filed_at: 2026-05-07T19:34:58Z
filed_on_commit: 68e505f96475258ae9c9e264d9bb45e75c373612
related_escalation: 20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md
related_decisions:
  - DEC-0017-effectiveness-scenario-fixture-augmentation.md
related_features:
  - P2-W7-F05  # find_references handler integration
phases_affected:
  - 1
  - 2 (likely; same scenario, same fixture)
resolved: true
---

## Summary

The phase-1 effectiveness gate just inverted from PASS (commit `fc50ef0`,
2026-05-08T00:55Z) to FAIL (commit `68e505f`, 2026-05-07T19:34Z) on the
same scenario, same fixture, same model, same tool set. The single delta
is a **−1 swap** on the `caller_completeness` rubric criterion for
`nav-rust-symbol`, driven by run-to-run agent stochasticity over whether
to enumerate one specific caller — the doctest example at
`src/http_client.rs:26` inside the `///` rustdoc on `retry_with_backoff`.

## Cross-run table

| run | commit | UCIL score (corr/cc/prec/fmt) | Baseline | Δ caller_completeness | Δ weighted | verdict |
|---|---|---|---|---|---|---|
| earlier (effectiveness-phase-1.md @ fc50ef0) | fc50ef0 | 5/5/5/5 | 5/4/5/5 | UCIL +1 | +0.3077 | PASS |
| this run (effectiveness-phase-1.md @ 68e505f) | 68e505f | 5/4/5/5 | 5/5/5/5 | UCIL −1 | −0.3077 | **FAIL** |

The two consecutive runs land on opposite sides of a 1-point criterion
threshold for the same logical fact (the doctest at line 26 either is or
isn't enumerated as a caller).

## Root cause

The MCP tool `find_references` is registered in `target/debug/ucil-daemon
mcp --stdio`, but its handler still returns
`{"_meta":{"not_yet_implemented":true,"tool":"find_references"}}` — the
Phase-1 stub described in §3.2 of the master plan. UCIL agents fall back
to `search_code` (text-search) + `understand_code` for caller enumeration.

`search_code` does surface the doctest match (it's a `retry_with_backoff(`
text occurrence in `src/http_client.rs:26`). What varies between runs is
**whether the agent decides to enumerate that match as a caller in its
final answer**. Some runs include it (citing it as "doctest example in
the rustdoc"), some runs omit it (likely treating it as a documentation
artifact rather than an executable call site). Both decisions are
defensible; the rubric judge then scores ±1 on caller_completeness
depending on which way the agent went.

The baseline agent uses `grep -rn "retry_with_backoff" .` which surfaces
the same doctest match, and the baseline agent makes the same yes/no
decision — but the two agents do *not* always make the *same* yes/no
decision in the *same* run, so the deltas swing.

`P2-W7-F05` is `passes=true` in `feature-list.json` (per `jq` confirmation
2026-05-07T19:30Z), but the daemon's MCP handler hasn't been wired to the
real fused-source caller enumeration. Once the handler returns a real
caller list (deterministic given the fixture), both sides will converge:
UCIL via the tool (deterministic), baseline via grep (deterministic).
The cross-run swap should disappear.

## Why "harness-config" (not blocking the loop)

1. UCIL's substantive answer is correct: it identifies the right
   exponential-backoff function, lists its definition at the right line,
   and lists 4 of 5 callers including all three unit tests, the wrapper,
   and the test-of-wrapper. The single missed caller is a doctest. There
   is no UCIL regression — UCIL's behaviour is the same as in the
   earlier run; only the agent's stochastic answer-formatting choice
   differs.
2. Phase 1 has been shipped (per `progress.json` showing phase 3 active)
   and that gate decision is not retroactively un-flipped by this
   re-evaluation report. The strict-rubric FAIL is correct, but the
   blast radius is "the next phase-1 effectiveness re-run could PASS or
   FAIL with ~50/50 probability" rather than "UCIL produces wrong
   results."
3. The scenario × fixture × stub combination has now been observed
   producing both PASS and FAIL deterministically in consecutive runs.
   This is a flake in the testing harness (the agent-stochastic decision
   path), not a UCIL bug.

## Recommended fixes (in priority order)

1. **Wire the real `find_references` handler in `ucil-daemon`** so that
   UCIL's caller enumeration is deterministic. The expected outcome:
   both sides report the same canonical 5-caller set
   (26 doctest, 64 wrapper, 84/91/110 unit tests) every run. This
   removes the source of the swap. (Tracked under existing feature
   `P2-W7-F05`; the feature is `passes=true` but the daemon-side wiring
   is incomplete — surface this back to the planner for a follow-up
   work-order.)

2. **Tighten the scenario rubric** so caller_completeness explicitly
   stipulates whether doctest call sites count. Currently the rubric
   says "every real caller is listed; no fabricated callers; callers
   are from the fixture tree (not stdlib)" — which leaves the doctest
   ambiguous (is it "real"? it executes under `cargo test --doc`).
   Either:
     - Stipulate "include doctest callers" → both sides converge on 5,
       judges score deterministically.
     - Stipulate "exclude doctest callers" → both sides converge on 4,
       judges score deterministically.
   Either choice removes the ambiguity that drives the swap.

3. **Add a scenario re-run policy** to the rubric: when a single
   criterion delta of ±1 is observed and the weighted delta is within
   ±0.5, re-run the agent (UCIL or baseline as appropriate) once and
   take the average. This is the standard Test-Time-Evaluation
   smoothing trick for noisy 0–5 integer rubrics. Costs ~1 extra agent
   run per ambiguous scenario but trades it for a deterministic gate.

## Acceptance / scope

This escalation does NOT propose to flip the gate verdict to PASS. The
rubric is mechanical and the FAIL verdict stands for this run. The
escalation is filed so triage / planner can decide whether to:

- Ship a small follow-up WO to wire `find_references` end-to-end
  (Recommended fix 1), or
- Tighten the rubric (Recommended fix 2 — fast but blunt), or
- Add a re-run policy (Recommended fix 3 — moderate engineering, future-proofs other scenarios), or
- Some combination.

## Reproducibility pointers

- `ucil-build/verification-reports/effectiveness-phase-1.md` (this run, FAIL)
- earlier PASS run preserved in git history at commit `fc50ef0`
- `/tmp/ucil-eval-nav-rust-symbol/` — full artefact tree (preserved
  through this session; will be cleaned up by the next evaluator
  invocation per agent contract)
- `find_references` stub probe: see "Tool-availability probe" §
  in `effectiveness-phase-1.md`

## Resolution (2026-05-08T02:14Z, monitor session, user-authorised)

Deferred per DEC-0017 precedent (rs-line flake → Phase-8 audit).

Rationale:
1. The escalation's own frontmatter declares `blocks_loop: false` and
   `requires_planner_action: false` — this is a Phase-1 effectiveness
   advisory, not a Phase-3-blocking regression.
2. Phase 1 already shipped (tag `phase-1-complete`). LLM-judge
   stochasticity on a phase-1 scenario does not gate Phase-3 ship.
3. Root cause (find_references handler is a Phase-1 stub) is on the
   Phase-3 work-trajectory — wiring up the real handler is part of P3-W9
   forward work and will deterministically eliminate the agent fallback
   that drives the flake.
4. Triage pass-1 mis-classified this as Bucket-E because the escalation
   offered three remediations without a chosen path. Per the user's
   standing autonomy directive ("fix everything end to end. No slacking
   off."), the monitor session picks **path 1: defer to Phase-3
   find_references work** — same shape as DEC-0017's deferral of the
   sibling flake.

**Carried forward**: Phase-3 planner should ensure a future P3 WO wires
the real `find_references` handler. Once that lands, the next
effectiveness re-run will be deterministic on this scenario.

resolved: true
