---
raised_by: verifier
raised_at: 2026-04-18T08:18:56Z
severity: harness-config
blocks_loop: false
requires_planner_action: true
related_work_order: WO-0024
related_features: [P1-W4-F02, P1-W4-F08]
---
# Escalation: Pre-existing `cargo doc` failure in `crates/ucil-core/src/incremental.rs` blocks WO-0024 gate

## Summary

While verifying WO-0024 (`kg-crud-and-hot-staging`, features P1-W4-F02
and P1-W4-F08), acceptance criterion #5 — `cargo doc -p ucil-core
--no-deps 2>&1 | { ! grep -qE '^(warning|error)'; }` — **fails on the
WO-0024 branch AND on `origin/main`** because of two ambiguous intra-doc
links in `crates/ucil-core/src/incremental.rs` that were introduced
silently by WO-0009 (`feat(core): salsa incremental engine skeleton with
early-cutoff DAG`, commit `5c2739a`).

The WO-0024 branch does NOT touch `incremental.rs`
(`git diff origin/main..HEAD -- crates/ucil-core/src/incremental.rs` is
`0 lines`). Per the strict reading of verifier.md step 8, the criterion
failure forces a REJECT, but the executor's actual implementation is
production-quality (critic report `CLEAN`; all eight feature tests pass;
coverage 97.10%; reality-check mutation verifies real code).

## The bug

`crates/ucil-core/src/incremental.rs` lines 5-6:

```rust
//! ([`FileRevision`]) to two tracked query functions ([`symbol_count`] and
//! [`dependent_metric`]) so the compiler, rustdoc, and the unit-test suite
```

`salsa::tracked fn symbol_count` and `salsa::tracked fn dependent_metric`
each expand to **both** a function and a struct with the same name, so
`rustdoc` cannot disambiguate the unadorned intra-doc links. Compiler
output on every `cargo doc -p ucil-core`:

```
error: `symbol_count` is both a function and a struct
error: `dependent_metric` is both a function and a struct
error: could not document `ucil-core`
```

`rustdoc`'s own suggested fix is a 4-character edit per link:

```
help: to link to the function, add parentheses
  |  [`symbol_count()`]  [`dependent_metric()`]
help: to link to the struct, prefix with `struct@`
  |  [`struct@symbol_count`]  [`struct@dependent_metric`]
```

Either form suffices. Since the surrounding prose clearly refers to
**query functions**, the function form (`()`) is the obvious choice.

## Why it slipped through WO-0009

WO-0009's acceptance gate did not include `cargo doc -p ucil-core
--no-deps`. The regression passed verification silently. It is caught
now for the first time because WO-0024's planner listed `cargo doc` as
a gate criterion without preflighting the check on `origin/main`.

## Classification — why this is a clean Bucket-D candidate

Per `.claude/rules/triage` / root CLAUDE.md triage protocol:

> **Bucket D (convert to micro-WO)**: escalation describes a narrow
> bug-fix in UCIL source (`crates/`, `adapters/`, `ml/`, `plugin*/`,
> `tests/` non-fixture) that's **< 60 lines / < 4 files**, no feature
> with `attempts >= 2`. Triage writes a new short-scoped work-order with
> empty `feature_ids` and the fix as `scope_in`, resolves the escalation
> with a "converted to WO-NNNN" note.

This escalation satisfies every Bucket-D requirement:

- Narrow bug-fix in UCIL source? **YES** — two 4-character edits in
  `crates/ucil-core/src/incremental.rs`, ≤ 8 lines of diff in 1 file
  (well under the 60-line / 4-file ceiling).
- Feature `attempts >= 2` on any affected feature? **NO** — `P1-W4-F02`
  and `P1-W4-F08` both carry `attempts = 0` in `feature-list.json` (this
  verifier did not call `flip-feature.sh`; verifier.md step 8 says "Do
  NOT flip anything" on a reject path).
- Is there an unambiguous concrete fix? **YES** — `rustdoc` prints the
  patch in the error output (see above). No design judgement required.

## Proposed micro-WO (for triage to author)

**Suggested slug**: `fix-incremental-rustdoc-ambiguity`
**Phase**: 1 (to match the active phase — independent of any feature)
**Week**: n/a (harness/admin work; can carry `week = 0` or inherit the
current week)
**feature_ids**: `[]` (pure admin fix per Bucket-D spec)
**scope_in**:
- `crates/ucil-core/src/incremental.rs` lines 5-6 — change `[`symbol_count`]`
  → `[`symbol_count()`]` and `[`dependent_metric`]` →
  `[`dependent_metric()`]`. Verify no other ambiguous intra-doc link
  hits remain in the crate.

**acceptance_criteria**:
- `cargo doc -p ucil-core --no-deps 2>&1 | { ! grep -qE '^(warning|error)'; }`
- `cargo nextest run -p ucil-core incremental::` all pass (regression
  guard; the existing 5 tests must still work)
- `cargo clippy -p ucil-core --all-targets -- -D warnings`
- `git diff origin/main..HEAD --name-only` touches only
  `crates/ucil-core/src/incremental.rs`

**Estimated commits**: 1
**Estimated diff**: 4 lines added, 2 lines removed (net +2)
**Estimated complexity**: trivial

## After the micro-WO lands

1. Triage or verifier re-runs the full WO-0024 gate against
   `feat/WO-0024-kg-crud-and-hot-staging` tip (commit `7aacaa4`, which
   is merge-ready after rebase onto the micro-WO's merge commit on
   `main`).
2. Criterion 5 (`cargo doc`) goes green; all other 17 checks remain
   green (this verifier already ran them from a clean-slate cargo env).
3. `flip-feature.sh P1-W4-F02 pass` and `flip-feature.sh P1-W4-F08 pass`
   can run in the next verifier session.
4. WO-0024 branch merges to `main`; P1 Week-4 cascade unblocks (P1-W4-F03
   symbol resolution, P1-W4-F04 extraction pipeline, P1-W4-F10
   get_conventions, transitively F05/F09 + P1-W5-F02/F09).

## Verifier's position

The executor's WO-0024 work is production-quality; the critic verdict is
`CLEAN`; the only failing acceptance criterion is pre-existingly broken
and the fix is out of scope for WO-0024 (not listed in `scope_in`;
`scope_out` forbids a new ADR absent a spec ambiguity or build failure —
`cargo build --workspace` is green, only `cargo doc` is red). Rejecting
is the protocol-correct call; a Bucket-D micro-WO is the protocol-correct
remediation.

`blocks_loop: false` — this escalation is administrative and safe for
triage pass 1 to auto-convert. It does not require human review beyond
approval of the micro-WO's scope.

---

(No `resolved: true` yet — awaiting triage/planner action to emit the
fix-WO and re-run WO-0024 gate.)
