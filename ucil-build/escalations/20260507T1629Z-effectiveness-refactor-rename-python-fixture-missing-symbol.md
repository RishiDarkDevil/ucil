---
title: refactor-rename-python scenario references compute_score, but python-project fixture has no such symbol
phase: 2
component: tests/scenarios + tests/fixtures
severity: harness-config
blocks_loop: false
requires_planner_action: true
filed_by: effectiveness-evaluator (phase-2 run)
filed_at: 2026-05-07T16:29Z
---

## Summary

The phase-2-tagged effectiveness scenario `tests/scenarios/refactor-rename-python.yaml`
asserts in its task description: *"In the Python fixture at the current working
directory, there is a function named `compute_score`."*

This claim is **false** for the current `tests/fixtures/python-project/` fixture.
The fixture is a self-contained interpreter (lexer / parser / evaluator) and
contains zero occurrences of `compute_score` anywhere — neither as a definition,
nor as a call site, nor as a string reference.

## Evidence

```
$ grep -rn "compute_score\|compute_relevance" tests/fixtures/python-project/
(no output — exit 1)

$ find tests/fixtures/python-project -type f -name "*.py" | xargs wc -l
    92 src/python_project/__init__.py
  1405 src/python_project/evaluator.py
  1138 src/python_project/types.py
  1049 src/python_project/parser.py
  1090 src/python_project/lexer.py
   466 tests/test_parser.py
   588 tests/test_evaluator.py
   415 tests/test_lexer.py
  6243 total

$ find tests/fixtures/python-project -name "*.py" -exec grep -h "^def \|^    def \|^class " {} +
# (truncated — 60+ functions, all parser/lexer/evaluator helpers; no scoring logic)
```

The fixture's surface is `Lexer`, `Parser`, `Evaluator`, `Environment`, `Token`,
`ASTNode`, `Value`, etc. — an interpreter for a small expression language. There
is no scoring, ranking, relevance, or recommendation surface that a
`compute_score` function would belong to.

## Impact

The scenario's acceptance check #2 (`grep -rn --include="*.py"
"\bcompute_relevance_score\b" .`) cannot be satisfied without the agent
**fabricating** a function under that name. A truthful agent (UCIL or baseline)
will report "no `compute_score` function exists in this fixture; nothing to
rename", which is the correct answer to the actual fixture state but FAILs
acceptance check #2 deterministically.

This is structurally identical to the `nav-rust-symbol`-on-`rust-project`
scenario-fixture flake escalated in
`20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`: a scenario
asserting the existence of code that does not exist in the named fixture.
Unlike the `nav-rust-symbol` case (where acceptance check satisfaction is
*stochastic* — depends on whether the agent incidentally emits a `.rs:LINE`
token), the `refactor-rename-python` case is *deterministic*: every truthful
run will FAIL acceptance check #2.

## Per-agent-contract action taken

Per `.claude/agents/effectiveness-evaluator.md` §"Hard rules":

> If a scenario is bad (ambiguous task, impossible-to-score rubric), file an
> escalation describing the defect and skip it with `skipped_scenario_defect`.

The scenario is being marked **`skipped_scenario_defect`** in the phase-2
effectiveness report. No UCIL run, no baseline run, no judge call.

## Recommended remediations (planner / ADR triage)

A — **Augment the fixture** (preferred): add a `src/python_project/scoring.py`
module that defines `compute_score(...)` plus at least 3 call sites in
`evaluator.py` / `tests/test_evaluator.py`. This makes the rename non-trivial
and gives both `find_references` and `refactor` real targets. **Both
`tests/fixtures/**` and `tests/scenarios/**` are protected by root CLAUDE.md;
this requires planner approval + an ADR.**

B — **Rewrite the scenario** to target a symbol that exists in the current
fixture. Candidate: rename `Evaluator._dispatch` → `Evaluator._evaluate_node`,
which has ~25 call sites across `evaluator.py` + `test_evaluator.py`. Same
DEC-required path as (A).

C — **Defer to Phase 8 effectiveness audit** (consistent with the
`nav-rust-symbol` deferral in `20260507T0357Z-...`). The phase-2 effectiveness
gate runs with one phase-2-eligible scenario (`nav-rust-symbol`) instead of
two; a Phase-8 dedicated scenario-fixture-alignment WO does both fixes at
once.

## Resolution policy

Marking `blocks_loop: false` because:
- The phase-2 gate's effectiveness check still has `nav-rust-symbol` as the
  surviving scenario for substantive evidence of UCIL behaviour.
- This escalation is the second known scenario-fixture defect class and
  should be triaged together with the existing `nav-rust-symbol` escalation
  in the Phase-8 audit.
- Phase 2 already has 73/234 features passing and the merge / verifier
  pipeline should not stall on a scenario-spec defect.

A planner triage pass should: (a) mark this escalation `resolved: true` once
either remediation A/B/C is selected, and (b) update `tests/scenarios/README.md`
with a sentence stating that scenarios MUST cite at least one identifier that
`grep -q` finds in the named fixture.
