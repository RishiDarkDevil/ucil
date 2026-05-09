---
timestamp: 2026-05-09T10:27Z
type: harness-fixer-halt
phase: 3
severity: high
blocks_loop: true
requires_planner_action: true
---

# Harness-fixer halted — three Phase-3 sub-checks need UCIL source work

The phase-3 gate has three sub-checks that fail not because the gate
scripts are broken but because the UCIL source they assert against
is unfinished. Two of those scripts (`concurrency.sh`,
`dogfood-on-self.sh`) were TODO placeholders that I implemented in
this same harness-fixer pass — those are now green. The remaining
three failures are UCIL-source-territory work the harness-fixer is
forbidden to touch:

- `tests/fixtures/go-project/` does not exist; the harness asserts a
  go fixture from phase 3 onward.
- `crates/ucil-daemon/src/g5.rs:472` doctest fails to compile because
  the example passes a `&Vec<Box<dyn G5Source + Send + Sync>>` where
  the function signature now expects a `&[Box<dyn G5Source>]`. Until
  the doctest is fixed, `cargo test -p ucil-daemon` errors and
  `coverage-gate.sh ucil-daemon` fails.
- `crates/ucil-agents/` is an empty crate — only a `lib.rs` with a
  doc comment and lint attributes, no submodules, no tests. Coverage
  comes back as 0/0 and the gate's 85% line floor is unreachable.

Each of these is a planner-emitted feature work-order, not a harness
patch. Listing each one with the suggested next step.

## 1. `multi-lang-coverage.sh` — go fixture missing

### Investigation log

Direct run:

```
$ bash scripts/verify/multi-lang-coverage.sh 3
[multi-lang] phase=3 required=rust python typescript go
[multi-lang] rust OK — EvalContext resolved to tests/fixtures/rust-project/src/eval_ctx.rs
[multi-lang] python OK — Evaluator resolved to tests/fixtures/python-project/src/python_project/evaluator.py
[multi-lang] typescript OK — FilterEngine resolved to tests/fixtures/typescript-project/src/filter-engine.ts
[multi-lang] go FAIL — fixture dir missing: tests/fixtures/go-project
[multi-lang] FAIL — 1 language(s) failed the probe
```

The script's `case` statement asserts go support from phase 3:

```bash
case "$PHASE" in
  1|2) REQUIRED="rust python typescript" ;;
  3|3.5|4) REQUIRED="rust python typescript go" ;;
```

`tests/fixtures/` confirms no `go-project` directory:

```
$ ls tests/fixtures/
mixed-project  python-project  python_project  rust-project  typescript-project
```

Master plan §1457 lists go among supported languages but the
phase-3 deliverables (master plan §1795–1825) call for "all 8 tool
groups operational" and don't pin go fixture creation to a specific
week. No feature in `feature-list.json` schedules a go fixture.

### What human review must decide

Two paths, both belong to the planner / user, not the harness-fixer:

1. **Add a go-project fixture as a phase-3 feature.** Mirror the
   shape of `tests/fixtures/python-project` (a small but real
   multi-file program with a clear top-level type the probe can
   resolve, e.g. `Server`). Schedule the fixture creation + the
   tree-sitter-go pipeline wiring as a planner WO before phase-3
   gate can pass.

2. **Defer go to phase 4 in the harness assertion.** If the master
   plan reading is that the four-language deliverable lands with
   the host adapters in phase 4, edit
   `scripts/verify/multi-lang-coverage.sh:19-23` to move go to the
   phase-4 line. This is a gate-semantics change and needs an ADR;
   the harness-fixer is forbidden from making it unilaterally.

## 2. `coverage-gate.sh ucil-daemon` — broken doctest

### Investigation log

```
$ cargo test --package ucil-daemon
...
test crates/ucil-daemon/src/g5.rs - g5::execute_g5 (line 459) stdout
error[E0308]: mismatched types
   --> crates/ucil-daemon/src/g5.rs:472:26
    |
472 | let outcome = execute_g5(&sources, &q, G5_MASTER_DEADLINE).await;
    |               ---------- ^^^^^^^^ expected `&[Box<dyn G5Source>]`,
    |                                   found `&Vec<Box<dyn G5Source + Send + Sync>>`
test result: FAILED. 32 passed; 1 failed; 3 ignored
error: doctest failed, to rerun pass `-p ucil-daemon --doc`
```

The library code itself compiles cleanly — the daemon binary is
healthy and the new `concurrency.sh` + `dogfood-on-self.sh` probes
both successfully drive it. Only the doctest example is stale: it
constructs a `Vec<Box<dyn G5Source + Send + Sync>>` and passes a
slice reference whose element type no longer matches the (also-
stale-or-new) function signature. Either the doctest needs the
trait bounds aligned, or the function signature widened to accept
the `Send + Sync` variant.

### What harness-fixer cannot do

`crates/ucil-daemon/**` is UCIL source — out of write-scope. The
fix is one of:

- Tighten the function signature to match what the doctest builds
  (`pub async fn execute_g5(sources: &[Box<dyn G5Source + Send + Sync>], …)`).
- Loosen the doctest construction to match the current signature
  (`let sources: Vec<Box<dyn G5Source>> = …`).
- A combination, depending on what callers of `execute_g5` expect.

### What human review must decide

Schedule a planner WO scoped at "fix the g5 doctest signature
mismatch in `crates/ucil-daemon/src/g5.rs:459-475` so `cargo test
-p ucil-daemon --doc` is green". One-line change in either
direction — but the choice between widening the function and
narrowing the doctest is a UCIL-source-API decision.

## 3. `coverage-gate.sh ucil-agents` — empty crate

### Investigation log

```
$ cat crates/ucil-agents/src/lib.rs
//! `ucil-agents` — internal agent implementations: provider, interpreter, synthesis,
//! conflict resolution, clarification, convention, memory curator, architecture.
#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

$ ls crates/ucil-agents/src
lib.rs

$ cargo test -p ucil-agents
running 0 tests
test result: ok. 0 passed; 0 failed
```

Coverage report (already generated, see
`ucil-build/verification-reports/coverage-ucil-agents.md`):

```
{
  "lines": {"count": 0, "covered": 0, "percent": 0},
  "functions": {"count": 0, "covered": 0, "percent": 0}
}
```

The crate has zero source code. Master plan §1829-1845 (Phase 3.5
"Agent layer") schedules the seven agents that should populate this
crate (`LlmProvider` trait + Ollama/Claude/OpenAI/host-passthrough/
none, Query Interpreter, Synthesis Agent, Conflict Mediator,
Clarification, Convention Extractor, Memory Curator, Architecture
Narrator).

### What harness-fixer cannot do

Implementing agents is UCIL source. Out of write-scope. Even
"weakening the floor for ucil-agents at phase 3" is a gate-semantics
change requiring an ADR — and is the wrong move because the floor
is doing exactly what it's supposed to: telling us a crate has no
implementation.

### What human review must decide

Either:

1. **Promote the agent-layer features to phase 3** so coverage can
   land before the phase-3 gate. Master plan currently calls Phase
   3.5 "Weeks 12–13"; if the gate is meant to enforce a crate-by-
   crate floor at the phase boundary, those features need to be in
   the phase-3 cohort.

2. **Exempt ucil-agents from the phase-3 coverage matrix.** Edit
   `scripts/gate/phase-3.sh:19` to drop `ucil-agents` from the
   crate list and re-add it at the phase-3.5 gate. This is the
   harness-side option but requires an ADR — the gate is currently
   asserting "every shipped crate is covered by 85%/75%", which is
   a real anti-laziness lever the master plan §15 calls out.

## Summary table (this harness-fixer pass)

```
Harness-fixer pass: 2026-05-09T10:27Z
Phase: 3
Scripts processed:
  scripts/verify/concurrency.sh          : FIXED — commit 252b70c
  scripts/verify/dogfood-on-self.sh      : FIXED — commit 94ed8bc
  scripts/verify/multi-lang-coverage.sh  : HALT  — UCIL source: missing go fixture
  scripts/verify/coverage-gate.sh (ucil-daemon)  : HALT — UCIL source: g5.rs:472 doctest
  scripts/verify/coverage-gate.sh (ucil-agents)  : HALT — UCIL source: empty crate
Diff budget used: ~120 of 120 LOC (concurrency 78 net, dogfood 41 net)
```

## Recommended next step

Spawn a planner pass to emit three work-orders against
`feature-list.json`:

1. WO-go-fixture-phase-3: add `tests/fixtures/go-project` with a
   small multi-file go program defining `Server` (matching the
   probe symbol in `multi-lang-coverage.sh:43`). If the master plan
   reading is that go is phase-4 work, write an ADR that moves the
   harness assertion instead.
2. WO-fix-g5-doctest: align signature/example types in
   `crates/ucil-daemon/src/g5.rs` lines 459–476 so
   `cargo test -p ucil-daemon --doc` exits 0.
3. WO-ucil-agents-phase-3-vs-3.5: choose between scheduling
   ucil-agents implementation features into phase 3 or amending
   `scripts/gate/phase-3.sh` to drop the crate from the phase-3
   coverage matrix (ADR required).
