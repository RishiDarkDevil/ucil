---
id: DEC-0018
status: accepted
date: 2026-05-07
authored_by: monitor session (with explicit user authorization 2026-05-07T17:30Z — "fix everything end to end. No slacking off.")
supersedes: none
superseded_by: none
related_to:
  - scripts/gate/phase-2.sh
  - ucil-build/feature-list.json (P3.5-W12-* through P3.5-W13-*)
---

# DEC-0018: Remove `ucil-agents` from Phase 2's coverage gate

## Status
Accepted, 2026-05-07.

## Context

`scripts/gate/phase-2.sh` requires `cargo llvm-cov` line coverage on
six crates:

```bash
for crate in ucil-core ucil-daemon ucil-treesitter \
             ucil-lsp-diagnostics ucil-embeddings ucil-agents; do
  check "coverage gate: ${crate}"          scripts/verify/coverage-gate.sh "${crate}" 85 75
done
```

The Phase 2 gate-check at HEAD `0d05864` reported:

```
[coverage-gate] Running cargo llvm-cov on 'ucil-agents' (min_line=85%, min_branch=75%)...
[coverage-gate] FAIL — ucil-agents line=0% branch=n/a
  [FAIL] coverage gate: ucil-agents
[FAIL] phase-specific checks failed
```

`ucil-agents/src/lib.rs` is a 9-line placeholder with only attribute
declarations and a doc comment — no `pub` items, no `mod` declarations,
no tests. Coverage is 0 % because there is no code to cover.

`jq '.features[] | select(.crate=="ucil-agents") | "\(.id) phase=\(.phase)"'`
on the frozen feature oracle (`ucil-build/feature-list.json`,
freeze commit) returns **13 entries, all in Phase 3.5**:

```
P3.5-W12-F01 phase=3.5: LlmProvider trait …
P3.5-W12-F02 phase=3.5: Ollama local provider …
P3.5-W12-F03 phase=3.5: Claude API provider …
…
P3.5-W13-F06 phase=3.5: Architecture Narrator background agent …
```

**Zero of those features are scheduled for Phase 2.** Phase 2's master
plan §18 deliverables (W6–W8: tree-sitter G2, plugin host, embeddings,
LanceDB) do not include any agent-layer work. The agent layer is
explicitly the Phase-3.5 / Week-12-13 deliverable per master plan §18.

The comment at the top of the loop in `phase-2.sh` —

> Anti-laziness quality gates — Phase 2 lights up embeddings + agents
> crates on top of Phase 1's four. Auto-skip any crate dir not yet present.

— is incorrect. The phrase "lights up agents" was anticipatory; it does
not match the feature oracle, which is the canonical scope source per
`CLAUDE.md` Oracle hierarchy rule 2. The "auto-skip if dir not present"
escape hatch does not fire because `crates/ucil-agents/` does exist as
an empty placeholder workspace member.

## Decision

Remove `ucil-agents` from Phase 2's coverage-gate for-loop. Phase 2's
coverage gate now covers exactly the five crates with phase-2-active
features:

```bash
for crate in ucil-core ucil-daemon ucil-treesitter \
             ucil-lsp-diagnostics ucil-embeddings; do
  check "coverage gate: ${crate}"          scripts/verify/coverage-gate.sh "${crate}" 85 75
done
```

The 85 % line / 75 % branch thresholds remain unchanged for every other
crate. No `#[ignore]`, `.skip`, `xfail`, fixture mod, or test
relaxation is involved — the change is purely **scope alignment**
between gate-spec and feature-oracle.

`scripts/gate/phase-3.sh` carries the same scoping bug
(`ucil-agents ucil-cli` in its for-loop). That gate runs only at Phase
3 ship-time, so the fix is deferred to a separate ADR at that gate-time
(or to a `/phase-start 3` planner pass), not bundled here.
`scripts/gate/phase-3.5.sh`, `phase-4.sh`, … still need explicit
coverage targets for `ucil-agents` once its real implementation lands;
that is the responsibility of those phases' planners.

## Rationale

1. The feature oracle (`feature-list.json`) is rule-2 in `CLAUDE.md`'s
   Oracle hierarchy. Phase-2 contains zero `ucil-agents` features, so
   gating Phase-2 ship on `ucil-agents` coverage contradicts the oracle.
2. Anti-laziness rule "Loosen a coverage target. Without an ADR." is
   satisfied by this ADR. The 85 % / 75 % thresholds remain in effect
   for every crate that Phase 2 actually touches.
3. The check at `coverage-gate.sh ucil-agents 85 75` would only ever
   PASS if Phase 2 either:
   - introduced premature implementation of agent code (a different
     anti-laziness violation: scope-creep without planner approval), or
   - added a no-op test that exercised the empty `lib.rs` (a different
     anti-laziness violation: tests that test nothing real), or
   - relaxed the 85 % threshold (the explicit anti-laziness violation
     this ADR is bypassing the right way).
4. The cleanest path is to align scope, leaving the Phase 3.5 gate to
   add the crate at the correct phase.

## Consequences

### Positive
- Phase 2 ship is unblocked for the legitimate scope (`P2-W6-*`
  through `P2-W8-*` features, all with `passes: true` at HEAD).
- The five remaining coverage-gate crates retain full 85 % / 75 %
  rigor.
- Future-phase planners are explicitly responsible for re-adding
  `ucil-agents` to their phase's coverage target when the crate has
  real implementation.

### Negative / risks
- `phase-3.sh` retains the same bug; that gate will FAIL at Phase 3
  ship-time unless a follow-up ADR amends it. Mitigation: track as a
  Phase-3 planner deliverable.
- If a future executor writes `ucil-agents` source during Phase 2 (out
  of scope) the coverage gate will not catch the regression here.
  Mitigation: per-feature work-orders are scoped by `crate` field; any
  WO touching `ucil-agents` would be flagged by the planner as
  out-of-phase before reaching the executor.

## Revisit trigger

When `/phase-start 3` runs, the Phase 3 planner pass should evaluate
the same scoping bug in `phase-3.sh` and either:
- author DEC-0018-supersede (or sibling DEC-NNNN) to remove
  `ucil-agents` from `phase-3.sh` until Phase 3.5, or
- add a Phase-3 work-order that introduces real `ucil-agents` source
  with passing tests.

## References

- `CLAUDE.md` (root): Oracle hierarchy, anti-laziness contract
- `scripts/gate/phase-2.sh`: the affected gate
- `ucil-build/feature-list.json`: canonical Phase-3.5 scope of
  `ucil-agents` (P3.5-W12-F01 through P3.5-W13-F06)
- `master-plan-v2.1-final.md` §18 Phase 2 / Phase 3.5
