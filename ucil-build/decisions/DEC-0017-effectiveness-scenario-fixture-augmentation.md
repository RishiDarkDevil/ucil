---
id: DEC-0017
title: ADR — effectiveness scenarios authored against fixtures missing the asserted symbols; augment fixtures
status: accepted
date: 2026-05-07
authored_by: monitor (with explicit user authorization on 2026-05-07T16:48Z)
phase: 2
extends: none
related_escalations:
  - 20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md
  - 20260507T1629Z-effectiveness-refactor-rename-python-fixture-missing-symbol.md
related_features:
  - none directly; unblocks Phase-2 effectiveness gate
---

## Context

Two effectiveness scenarios — `tests/scenarios/nav-rust-symbol.yaml`
(phases 1–8) and `tests/scenarios/refactor-rename-python.yaml` (phases
2–8) — assert facts about their fixtures (`tests/fixtures/rust-project/`
and `tests/fixtures/python-project/`) that are **not true**:

- `nav-rust-symbol` claims the rust fixture contains "functions that
  perform an HTTP retry with exponential backoff." The fixture is a
  toy expression evaluator with **zero HTTP code** and no retry
  helpers anywhere in its tree. A truthful agent answers "no
  qualifying functions found" — the substantively correct answer that
  matches ground truth — but acceptance check #3
  (`grep -qE "\.rs:[0-9]+"`) cannot deterministically pass on a
  truthful empty answer.
- `refactor-rename-python` claims the python fixture has "a function
  named `compute_score`." The fixture is a toy lexer/parser/evaluator
  with **zero `compute_score` references** anywhere. A truthful agent
  answers "no such function exists; nothing to rename" — but
  acceptance check #2 (`grep -rn --include="*.py" "\bcompute_relevance_score\b"`)
  cannot pass on a truthful answer that doesn't fabricate code.

Both flakes have been observed during Phase-2 effectiveness runs
(commit `76045c6`), have been documented as escalations citing
`severity: harness-config` and `blocks_loop: false`, and have LLM-judge
weighted means within the ±0.5 noise window between UCIL and baseline
(no UCIL regression detected).

The fix that resolves both flakes is fixture augmentation: add the
asserted symbols to each fixture, with realistic-but-stubby
implementations and at least one caller, so the scenarios have a
non-empty ground truth to find / rename.

The root `CLAUDE.md` states:

> Modify files under `tests/fixtures/**` to make a test pass.

…as a forbidden behaviour for the executor. The intent of that rule is
to prevent an executor from silencing a real failure by changing the
spec. **This ADR is not the executor silencing a failure** — it is a
spec-author/planner-level repair of a fixture/scenario pair that was
authored inconsistently from day one. The substantive UCIL behaviour
under the corrected scenarios will still need to be verified by the
effectiveness evaluator on the next run; the fix only makes the
acceptance checks satisfiable for the truthful answer.

## Decision

Augment both fixtures with the missing symbols, authorised under this
ADR, so that Phase-2 (and onward) effectiveness scenarios are testable.

### `tests/fixtures/rust-project/`

Add a real (not stub) HTTP-retry-with-exponential-backoff helper:

```rust
// src/http_client.rs
//! Minimal HTTP retry helper with exponential backoff.
//! Used by main.rs to demonstrate exponential-backoff retry for the
//! UCIL nav-rust-symbol effectiveness scenario.
use std::thread;
use std::time::Duration;

/// Retry an operation with exponential backoff: delay doubles after each
/// failure, starting at `initial_delay`.
pub fn retry_with_backoff<F, T, E>(
    mut op: F,
    max_attempts: u32,
    initial_delay: Duration,
) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
{
    let mut delay = initial_delay;
    for attempt in 1..=max_attempts {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if attempt == max_attempts => return Err(e),
            Err(_) => {
                thread::sleep(delay);
                delay = delay.checked_mul(2).unwrap_or(delay);
            }
        }
    }
    unreachable!("loop guarantees a return on the last attempt")
}
```

…with at least one call site in `src/main.rs` (a small startup-banner
fetch, no real network — backed by a closure that returns Ok on the
second attempt).

### `tests/fixtures/python-project/`

Add a real (not stub) scoring module:

```python
# src/python_project/scoring.py
"""Scoring helpers for the python-project test fixture.

Used by tests/scenarios/refactor-rename-python.yaml to exercise the
rename-symbol-everywhere refactor flow.
"""
from __future__ import annotations
from typing import Iterable

def compute_score(values: Iterable[float], weights: Iterable[float]) -> float:
    """Compute a weighted score from parallel iterables of values and weights.

    Returns 0.0 if either iterable is empty. Values and weights are zipped
    pairwise; extra elements on either side are ignored.
    """
    total = 0.0
    weight_sum = 0.0
    for v, w in zip(values, weights):
        total += v * w
        weight_sum += w
    return total / weight_sum if weight_sum else 0.0
```

…with at least one call site in another module (e.g. `evaluator.py` or
a small new helper) and a passing pytest test in
`tests/test_scoring.py`.

## Rationale

1. The fixtures are part of the spec, but the spec is mutually
   inconsistent: the scenarios assert symbols the fixtures don't
   contain. Either the scenarios or the fixtures must change. Changing
   the scenarios (Option B / C from the original escalations) means
   weakening the acceptance checks to tolerate empty answers, which
   degrades the effectiveness signal for genuinely failing UCIL
   behaviour. Changing the fixtures preserves the acceptance contract
   and gives effectiveness real teeth.
2. Both fixture additions are *real working code with tests*, not
   stubs. They expand the surface UCIL's tools index — making the
   effectiveness signal **stronger** (positive ground truth instead of
   negative ground truth).
3. Per root `CLAUDE.md`, the deny-list rule on `tests/fixtures/**`
   applies to the **executor agent** ("modify files to make a test
   pass"). This ADR is an explicit planner / user-authorised fixture
   patch with a clear motivation; it is not silencing a failure.

## Consequences

1. The next effectiveness run for phases 1+ should report:
   - `nav-rust-symbol` PASS (one matching function found, cited at
     `src/http_client.rs:<line>`).
   - `refactor-rename-python` PASS (one function `compute_score` exists
     for the agent to rename, with at least one call site to update).
2. `scripts/verify/effectiveness-gate.sh 2` will exit 0, unblocking
   `scripts/gate-check.sh 2` and Phase-2 ship.
3. The two prior deferral escalations
   (`20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`
   and `20260507T1629Z-effectiveness-refactor-rename-python-fixture-missing-symbol.md`)
   are superseded by this ADR; they should remain `resolved: true` with
   a "superseded by DEC-0017" trailer.
4. UCIL's tree-sitter symbol extraction now has slightly larger
   fixture surfaces; the per-fixture symbol count + chunk count will
   shift modestly. The cargo/pytest test suites still pass since the
   additions are net-new modules with their own tests.

## Revisit trigger

Reopen this ADR if:
- A future effectiveness scenario asserts symbols that don't exist
  in the corresponding fixture (the same pattern recurring); the
  preferred remediation should be to author the scenario against the
  fixture's actual surface, not to keep augmenting fixtures.
- The fixture additions cause a regression in UCIL's tree-sitter
  symbol-extraction tests (they shouldn't — the additions are
  ordinary Rust / Python modules — but flag if they do).
