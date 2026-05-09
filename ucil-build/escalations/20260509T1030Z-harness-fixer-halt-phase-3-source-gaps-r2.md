---
timestamp: 2026-05-09T10:30Z
type: harness-fixer-halt
phase: 3
severity: high
blocks_loop: true
requires_planner_action: true
supersedes: 20260509T1027Z-harness-fixer-halt-phase-3-source-gaps.md
---

# Harness-fixer halted (r2) — same three Phase-3 source gaps, no change since r1

The gate-check at 2026-05-09T10:30Z surfaced the identical set of
sub-check failures the prior harness-fixer pass already escalated at
`20260509T1027Z-harness-fixer-halt-phase-3-source-gaps.md`. I
re-verified each of the three failure modes is unchanged in HEAD and
am halting without applying any fixes — every remaining failure is
UCIL-source-territory work explicitly forbidden to the harness-fixer
by `.claude/agents/harness-fixer.md` (write-scope excludes `crates/`,
`adapters/`, `ml/`, `plugin*/`, `tests/*`).

## State re-verification (2026-05-09T10:30Z)

```
$ ls tests/fixtures/
mixed-project  python-project  python_project  rust-project  typescript-project
# go-project still missing → multi-lang-coverage.sh:69 fails the go probe
```

```
$ ls crates/ucil-agents/src/
lib.rs
# still only the doc comment + lint attributes, no submodules
# coverage = 0/0 → coverage-gate.sh ucil-agents fails the 85% floor
```

```
$ sed -n '464,478p' crates/ucil-daemon/src/g5.rs
…
/// # async fn demo(sources: Vec<Box<dyn G5Source + Send + Sync + 'static>>) {
…
/// let outcome = execute_g5(&sources, &q, G5_MASTER_DEADLINE).await;
…
pub async fn execute_g5(
    sources: &[Box<dyn G5Source>],
…
# doctest still passes &Vec<Box<dyn G5Source + Send + Sync + 'static>>
# where the function expects &[Box<dyn G5Source>]
# → cargo test -p ucil-daemon errors → coverage-gate.sh ucil-daemon fails
```

## Cross-check: prior fixes are now green

The two probes I implemented in the previous pass (commits `252b70c`
and `94ed8bc`) are reporting OK in the third gate-check run captured
in `/tmp/ucil-gate-check.log`:

```
[concurrency] OK — 3 concurrent daemon agents completed cleanly
  [OK]   concurrency (3-agent)
[dogfood] OK — UCIL pointed back at itself for all probes
  [OK]   dogfood on ucil repo
```

The earlier `[FAIL]` entries for those scripts in the same log are
from gate-check passes that ran *before* my prior commits — the
gate-check.sh dispatcher invokes each sub-check multiple times in a
single log session.

## Why I'm not applying any fixes this pass

| Failing check | What would fix it | Why I can't |
|---|---|---|
| `multi-lang-coverage.sh` (go) | Create `tests/fixtures/go-project/` with a `Server` symbol | `tests/*` is outside write-scope |
| `coverage-gate.sh ucil-daemon` | Align `g5.rs:472` doctest with the `execute_g5` signature | `crates/` is outside write-scope |
| `coverage-gate.sh ucil-agents` | Implement the agent-layer crate, OR exempt it from phase-3 coverage matrix | `crates/` outside write-scope; exempting via `scripts/gate/phase-3.sh` is a structural gate change requiring an ADR (per agent contract: "removing a `check` call is a planner-level decision") |

Considered but rejected:

- **Skipping doctests for ucil-daemon coverage** (`cargo test --lib --tests --bins`): would silence a real broken example in the published API, weakens the script's semantics — explicit anti-pattern per agent contract §safety-rails ("Don't weaken assertions in the scripts you fix").
- **Loosening the 85% line floor for ucil-agents**: same anti-pattern. The 0% reading is the script doing its job — telling us a crate has no implementation.
- **Moving go to phase-4 in `multi-lang-coverage.sh:21-22`**: gate-semantics change requiring an ADR.

## Recommended next step (unchanged from r1)

The recommendation in `20260509T1027Z-harness-fixer-halt-phase-3-source-gaps.md`
section "Recommended next step" remains current. Specifically, planner
needs to emit three work-orders — go fixture, g5 doctest fix, and
ucil-agents phase-3-vs-3.5 decision (with ADR if exempting). Until
those land in `feature-list.json` or get ADRs documenting the
defer-to-phase-4 path, the phase-3 gate cannot be made green without
weakening assertions.

## Summary table (this harness-fixer pass)

```
Harness-fixer pass: 2026-05-09T10:30Z
Phase: 3
Scripts processed:
  scripts/verify/multi-lang-coverage.sh         : HALT — UCIL source: missing go fixture (re-verified)
  scripts/verify/coverage-gate.sh (ucil-daemon) : HALT — UCIL source: g5.rs:472 doctest (re-verified)
  scripts/verify/coverage-gate.sh (ucil-agents) : HALT — UCIL source: empty crate (re-verified)
Diff budget used: 0 of 120 LOC
```

resolved: false
