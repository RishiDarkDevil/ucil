---
filed_by: effectiveness-evaluator
filed_at: 2026-05-07T03:57Z
phase: 1
severity: harness-config
blocks_loop: false
requires_planner_action: false
related_scenario: tests/scenarios/nav-rust-symbol.yaml
related_fixture: tests/fixtures/rust-project
related_report: ucil-build/verification-reports/effectiveness-phase-1.md
commit_at_filing: f4adc41497d141bfcfd7adb6e539d13e5d9c75a8
resolved: true
---

# Effectiveness scenario `nav-rust-symbol` rs-line acceptance check is flaky on `rust-project` fixture

## What happened

The `effectiveness-evaluator` ran `tests/scenarios/nav-rust-symbol.yaml`
against `tests/fixtures/rust-project` at commit `f4adc41`. Both UCIL and
baseline produced **substantively-correct** answers ("no qualifying
functions exist" — matching ground truth). Both LLM-judges scored both
sides identically: 5/5 correctness, 5/5 caller_completeness, 5/5
precision, 4/5 formatting → weighted mean 4.9231 each, **Δ weighted = 0.0**.

However, **acceptance check #3** (`grep -qE "\.rs:[0-9]+"`) failed on
both sides because neither output contained the literal `<filename>.rs:<line>`
form — there was nothing to cite by file:line when the truthful answer is
"no qualifying functions found".

Per the strict letter of `.claude/agents/effectiveness-evaluator.md` §6
("FAIL: acceptance_checks red on UCIL run"), this triggered a **per-scenario
FAIL → gate FAIL → exit 1**, despite UCIL not regressing the substantive
behaviour.

## Why it's a defect (not a UCIL regression)

The prior effectiveness run (4 hours ago, at commit `70aa72e`, recorded in
the same report file before this run overwrote it) reported PASS on the
same scenario with the same fixture and same UCIL surface. The prior
outputs were 65 lines (UCIL) / 92 lines (baseline) — substantially more
verbose than this run's 46 / 27 lines. With longer narrative output,
agents incidentally include module-level file:line annotations like
"src/parser.rs:42 — tokenizer entry" in their inventory tables, which
satisfies `grep -qE "\.rs:[0-9]+"`. With terser output (as in this run),
the regex fails.

**This makes the rs-line check stochastically satisfiable** — it depends
on LLM narrative-style variance, not on the agent's correctness or the
UCIL surface. The same scenario × fixture pairing produces noisy
PASS/FAIL across runs.

The substantive UCIL state (`find_definition` real, `find_references`
phase-1 stub) is unchanged between `70aa72e` (PASS) and `f4adc41` (FAIL).
The diff between these two commits is the prior effectiveness report
itself plus a work-order RFR document — no source code changes. So the
verdict flip is purely LLM stochasticity.

## Root cause

The scenario was authored assuming the fixture would contain at least one
qualifying function (HTTP retry with exponential backoff). The fixture
`rust-project` is a self-contained expression parser/evaluator that
contains **zero** matching functions (no HTTP, no async, no retry,
zero external dependencies). When the truthful answer is "none found",
the agent has nothing to cite by file:line, and the rs-line check is
satisfied only incidentally if the agent's narrative happens to include
file:line annotations on the (non-qualifying) modules in its inventory.

The acceptance check was a sensible guard against the agent producing a
non-grounded answer (e.g., narrative without any references). It just
wasn't designed for the negative-ground-truth case.

## Proposed remediations

Three independent options (any one resolves the flake):

### Option A — augment `rust-project` fixture with positive content

Add a small `src/http_client.rs` module to the fixture containing a real
exponential-backoff retry function, e.g.:

```rust
//! Stub HTTP retry for fixture testing.
use std::thread;
use std::time::Duration;

pub fn retry_with_backoff<F, T, E>(mut op: F, max_attempts: u32) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
{
    let mut delay = Duration::from_millis(100);
    for attempt in 1..=max_attempts {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if attempt == max_attempts => return Err(e),
            Err(_) => {
                thread::sleep(delay);
                delay *= 2; // exponential backoff
            }
        }
    }
    unreachable!()
}
```

…and at least one call site in `src/main.rs`. The scenario then has a
positive ground truth, and acceptance checks are naturally satisfied.

This is the cleanest remediation: it makes the scenario actually
exercise UCIL's `find_definition` + `find_references` for a non-trivial
case (which is the scenario's stated purpose).

**Caveat**: per CLAUDE.md root invariants, modifying `tests/fixtures/**`
to make a test pass is forbidden. This remediation requires planner /
ADR approval — augmenting the fixture is not the same as making a
specific test pass, but the rule is conservative.

### Option B — make the rs-line check conditional

Edit `tests/scenarios/nav-rust-symbol.yaml` so the rs-line check is
gated on the agent having claimed to find at least one function:

```yaml
acceptance_checks:
  - name: output file exists
    cmd: 'test -f /tmp/ucil-eval-out/nav-rust-symbol.md'
  - name: non-empty
    cmd: 'test $(wc -l < /tmp/ucil-eval-out/nav-rust-symbol.md) -ge 5'
  - name: cites file:line when at least one function found
    cmd: '! grep -q "^## " /tmp/ucil-eval-out/nav-rust-symbol.md || grep -qE "\.rs:[0-9]+" /tmp/ucil-eval-out/nav-rust-symbol.md'
```

This is a small scenario edit. It preserves the original guard intent
(don't accept a hand-wavy positive answer without file:line) while
allowing a clean negative answer.

### Option C — split into positive/negative scenarios

Keep `nav-rust-symbol` as the negative case (no functions to find) with a
relaxed rs-line check, and add a sibling `nav-rust-symbol-positive`
scenario that uses an augmented fixture (per Option A) to exercise the
positive-match path.

## Recommended disposition

The user / planner picks one of A, B, C. A is the most informative for
phase-1 effectiveness (actually exercises UCIL navigation); B is the
quickest fix; C is the most principled.

Until then, the gate-side reading is unstable: this scenario produces
PASS or FAIL depending on LLM verbosity. The autonomous loop should not
treat a FAIL on this scenario as a UCIL regression without checking the
report's "Substantive judge-tie" line.

## Resolution criteria

This escalation can be resolved (`resolved: true`) when:

- Option A applied: a real HTTP-retry function lives in
  `tests/fixtures/rust-project/src/http_client.rs` (or similar), with at
  least one caller in `src/main.rs`, and the next effectiveness re-run
  PASSes acceptance check #3 deterministically.

- OR Option B applied: the scenario yaml's rs-line check is conditional
  on the agent having declared a function found, and the next
  effectiveness re-run PASSes deterministically on the negative-ground-
  truth fixture.

- OR Option C applied: scenario split, both negative + positive variants
  PASS deterministically.

- OR a triage-pass-3 default-Bucket-E disposition has converted this to
  a halt-and-page for the user.

## Resolution (deferred to Phase-8 effectiveness audit)

Resolved 2026-05-07 by monitor session after triage Bucket-E halt.
Per the escalation's own self-classification (`blocks_loop: false`) and
the guidance line "the autonomous loop should not treat a FAIL on this
scenario as a UCIL regression without checking the report's Substantive
judge-tie line":

- This is a **Phase 1** scenario flake. Phase 1 already shipped (tag
  `phase-1-complete`). The substantive UCIL behaviour did not regress
  between `70aa72e` (PASS) and `f4adc41` (FAIL); both LLM judges scored
  4.9231 / 4.9231 weighted, Δ = 0.0.
- All three proposed remediations (A: augment fixture, B: conditional
  scenario yaml, C: split scenarios) touch territory that needs planner
  / ADR approval (`tests/fixtures/**` is denylist per root CLAUDE.md;
  `tests/scenarios/**` carries spec-equivalent weight). Triage Bucket-E
  halted conservatively.
- This must NOT block Phase 2's final WO (P2-W8-F08 find_similar MCP).
  Marking deferred so the autonomous loop can complete Phase 2 and
  proceed to /phase-ship 2.

**Carried forward**: a Phase-8 release-prep effectiveness audit must
re-evaluate this scenario with one of Options A/B/C applied, in an ADR
that authorises the fixture/scenario edit. Until then, treat scenario
`nav-rust-symbol` PASS/FAIL on `rust-project` as advisory only.

resolved: true

## Superseded by ADR DEC-0017 (2026-05-07T16:55Z)

The "deferred to Phase-8 audit" disposition above is now superseded.
Per ADR DEC-0017 (with explicit user authorisation 2026-05-07T16:48Z),
the rust-project fixture has been augmented with a real
`retry_with_backoff` helper at `tests/fixtures/rust-project/src/http_client.rs`
plus a call site in `src/main.rs` (commit `1c42c77`). The
nav-rust-symbol scenario now has positive ground truth — the agent has
exactly one qualifying function to find and cite by file:line. The
next effectiveness run is expected to PASS this scenario
deterministically.

resolved: true
