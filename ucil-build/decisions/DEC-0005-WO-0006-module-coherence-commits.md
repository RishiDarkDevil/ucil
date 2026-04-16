---
id: DEC-0005
title: Accept oversized module-introduction commits in WO-0006
date: 2026-04-17
status: accepted
work_order: WO-0006
features: [P1-W2-F02, P1-W2-F03, P1-W2-F06]
raised_by: critic
extends: DEC-0001
commits_cited: [711fb2c, 3cedd46, aa8c4aa]
---

# DEC-0005: Accept oversized module-introduction commits in WO-0006

## Context

The critic review of WO-0006 (symbol extraction + chunker + two-tier storage)
flagged three commits that exceed the ".claude/rules/commit-style.md" soft rule
*"~50 lines of diff per commit is a soft target"* and the critic-protocol
hard ceiling of 200 lines. The code was rated CLEAN on every other axis: no
stubs, no mocked critical collaborators (real tree-sitter grammars, real
filesystem), no skipped tests, full rustdoc coverage, substantive tests that
exercise real tree-sitter parsing and real `.ucil/` directory creation.

The three commits:

| SHA | Subject | Lines | File introduced |
|-----|---------|-------|-----------------|
| `711fb2c` | `feat(treesitter): add SymbolExtractor, SymbolKind, ExtractedSymbol` | 605 (+605 / -0) | `crates/ucil-treesitter/src/symbols.rs` |
| `3cedd46` | `feat(treesitter): add AST-aware Chunker producing ≤512-token Chunk values` | 293 (+293 / -0) | `crates/ucil-treesitter/src/chunker.rs` |
| `aa8c4aa` | `feat(daemon): add StorageLayout two-tier .ucil/ directory initialiser` | 234 (+234 / -0) | `crates/ucil-daemon/src/storage.rs` |

## Decision

Accept all three commits as-is. Do NOT rebase/rewrite history. Allow the
verifier session to proceed. This ADR extends the DEC-0001 precedent to
cover the specific class of "new module file containing type + impl +
unit-test-mod in a single commit."

## Rationale

### For `711fb2c` (symbols.rs, 605 lines)

`SymbolExtractor` is a language-generic AST visitor whose tree-sitter query
DSL rules cross-reference `SymbolKind` variants and `ExtractedSymbol` fields.
A split into (types) → (Rust rules + tests) → (Python rules + tests) →
(TypeScript rules + tests) would produce intermediate states where:

- The types commit has a `SymbolKind` enum with variants none of which are
  yet produced by any impl — clippy's `dead_code` lint fails.
- The per-language commit sequence introduces a public method
  (`SymbolExtractor::extract`) that returns empty `Vec<ExtractedSymbol>`
  until the final language is added — deceptive intermediate state.

`#[cfg(test)] mod tests` block sits in the same file and covers all four
languages; splitting by language would require either duplicating the test
module across commits (noise) or removing and re-adding tests in later
commits (worse).

23 `SymbolKind` variants × 4 tree-sitter grammar rulesets × 6 unit tests is
the minimum coherent commit for a new module that binds all three.

### For `3cedd46` (chunker.rs, 293 lines)

`Chunker` iterates over `SymbolExtractor` output (directly depends on
`711fb2c`). Its AST-aware chunking algorithm is a single recursive function;
splitting "types" from "algorithm" produces an unusable intermediate commit.
Tests (4 of them) bind to the algorithm's invariants, not to the types —
moving them to a separate commit means the impl commit has no test coverage
until the next commit, violating the "every feat commit has a test" rule.

### For `aa8c4aa` (storage.rs, 234 lines)

`StorageLayout` is a single type with a single `init()` method that creates
the two-tier `.ucil/` directory structure (shared, per-branch, sessions,
logs, plugins). The 234 lines include: type definition (40L),
`init()` impl (90L), `StorageError` variants (15L), and one integration
test (`test_two_tier_layout`, 85L) that calls `init()` on a `tempdir` and
asserts every directory exists + idempotency. The impl has no value without
the test proving the directory layout matches the master-plan spec.

## Consequences

- WO-0006 critic verdict is treated as CLEAN-with-ADR for purposes of
  proceeding to the verifier step.
- The corresponding escalation
  `ucil-build/escalations/20260415-1856-wo-WO-0006-attempts-exhausted.md`
  is resolved (marker added in separate commit).
- Two warnings from the critic report remain non-blocking:
  - W1 (missing `Feature: P1-W2-F03` trailer on refactor commit `705e5d6`):
    annotated in post-mortem; future refactors touching multiple features
    must carry all relevant feature trailers.
  - W2 (`ChunkError::ParseRequired` unused variant): the executor will
    remove it in a follow-up fix commit OR an ADR will be raised if kept
    for forward-compat.
- The soft rule *"~50 lines of diff per commit"* remains in force.
  DEC-0001 + DEC-0005 together establish a pattern for
  **module-introduction commits**: a single new `*.rs` file containing
  coherent type + impl + `#[cfg(test)] mod tests` may exceed the soft limit
  when splitting would produce non-compiling or dead-code intermediate
  states. Planners should still prefer smaller work-orders; executors
  should still prefer smaller commits when feasible.

## Pattern recognition

If this pattern recurs at ≥ 5 future work-orders (DEC-0001 + DEC-0005 +
3 more), promote it to an inline exception in
`.claude/rules/commit-style.md` under a new "Module-introduction exception"
subsection with the specific tests: (a) single new file, (b) types + impl +
unit-test mod coherent, (c) no test-less impl in intermediate states.

## Revisit trigger

If the triage agent begins auto-emitting this ADR for more than ~20% of
work-orders, the soft commit-size rule is too tight for the project's file
organization and should be raised globally (probably to 400L) rather than
continuing to rubber-stamp ADR-per-WO.
