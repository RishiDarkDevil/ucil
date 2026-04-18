# Root Cause Analysis: WO-0024 (kg-crud-and-hot-staging; features P1-W4-F02 + P1-W4-F08)

**Analyst session**: rca-WO-0024-20260418
**Work-order**: `ucil-build/work-orders/0024-kg-crud-and-hot-staging.json`
**Branch**: `feat/WO-0024-kg-crud-and-hot-staging`
**HEAD**: `7aacaa4da7707e09919130c66a946cd7e34ee9bf`
**Attempts before RCA**: 1 (retry 1 rejected 2026-04-18T08:18:56Z)
**Features touched**: P1-W4-F02, P1-W4-F08

---

## Failure pattern

**One** verifier rejection so far (`ucil-build/rejections/WO-0024.md`, retry 1).

17 of 18 verifier acceptance checks **PASS**. A single check **FAILS**:

```
cargo doc -p ucil-core --no-deps 2>&1 | { ! grep -qE '^(warning|error)'; }
```

The failure has **zero causal connection to WO-0024's diff**:

- `git diff origin/main..feat/WO-0024-kg-crud-and-hot-staging -- crates/ucil-core/src/incremental.rs` = **0 lines** (confirmed 2026-04-18, this RCA session).
- Running the same command on `origin/main` reproduces the same three `error:` lines. The regression was introduced by WO-0009 (commit `5c2739a`, `feat(core): salsa incremental engine skeleton with early-cutoff DAG`) and silently slipped through because WO-0009's gate did not include `cargo doc`.

This is therefore an **administrative rejection**, not a substantive one. The critic report (`ucil-build/critic-reports/WO-0024.md:10`) returned **CLEAN**; all 8 feature tests pass; line coverage is 97.10%; reality-check mutation verifies real code; zero stubs / mocks / forbidden-path diffs; commit hygiene green.

## Reproduction (this RCA session)

```
$ cd /home/rishidarkdevil/Desktop/ucil-wt/WO-0024
$ cargo doc -p ucil-core --no-deps 2>&1 | grep -E '^(warning|error)'
error: `symbol_count` is both a function and a struct
error: `dependent_metric` is both a function and a struct
error: could not document `ucil-core`

$ git diff origin/main..HEAD -- crates/ucil-core/src/incremental.rs | wc -l
0

$ git log --oneline origin/main -- crates/ucil-core/src/incremental.rs | head -1
5c2739a feat(core): salsa incremental engine skeleton with early-cutoff DAG
```

Offending source (`crates/ucil-core/src/incremental.rs:5-6`, identical on `main` and on WO-0024 tip):

```rust
//! ([`FileRevision`]) to two tracked query functions ([`symbol_count`] and
//! [`dependent_metric`]) so the compiler, rustdoc, and the unit-test suite
```

`salsa::tracked fn symbol_count` and `salsa::tracked fn dependent_metric` each macro-expand to **both** a function AND a same-named struct, so `rustdoc` cannot disambiguate the unadorned `[`symbol_count`]` / `[`dependent_metric`]` links. Crate-level lint `#![deny(rustdoc::broken_intra_doc_links)]` at `crates/ucil-core/src/lib.rs:8` upgrades the warning to a hard error.

`rustdoc`'s own suggested fix is a **4-character edit per link** — either append `()` (function form, matches the "query functions" prose) or prefix `struct@`.

---

## Hypothesis tree (ranked, most-likely-first)

### H1 (confidence 99%): Pre-existing rustdoc ambiguity in `incremental.rs` is the sole cause; WO-0024's work is clean

- **Evidence**: Reproduction above — same errors on `origin/main` and on WO-0024 tip; diff against `incremental.rs` is 0 lines; the other 17 verifier checks are green; critic verdict `CLEAN` (`ucil-build/critic-reports/WO-0024.md:10`).
- **Cheap falsifier already run**: the `git diff origin/main..HEAD -- crates/ucil-core/src/incremental.rs | wc -l = 0` check (above) is dispositive.

### H2 (confidence <1%): Some WO-0024 change transitively exposes the ambiguity that was latent on `main`

- **Why negligible**: the ambiguity is a property of the DOC COMMENT string contents, not of any import graph. `main` reproduces the same errors without any WO-0024 commit present. Re-running `cargo doc -p ucil-core --no-deps` at `/tmp/main-probe` (per rejection §Pre-existence proof, `ucil-build/rejections/WO-0024.md:73-80`) already confirms this.
- Not worth further investigation.

### H3 (confidence <1%): Planner should broaden WO-0024's `scope_in` to include `incremental.rs`

- **Why negligible**: piggy-backing an orthogonal fix onto the feature branch would break the hygienic "one logical change per commit" rule and force an additional verifier cycle on a WO the critic already cleared. Bucket-D micro-WOs exist precisely for this scenario (`ucil-build/CLAUDE.md` — *Escalation protocol / Bucket D*).

---

## Root cause (hypothesis H1, 99% confidence)

**The sole acceptance failure is a pre-existing rustdoc regression in `crates/ucil-core/src/incremental.rs:5-6` introduced by WO-0009 (commit `5c2739a`), not by WO-0024.** The `cargo doc -p ucil-core --no-deps` gate is the first WO in the build that exercises the check against this module, so WO-0024 is incidentally the first point at which the regression surfaces — not a contributor to it.

Evidence:
- `crates/ucil-core/src/incremental.rs:5-6` — two unadorned intra-doc links to `salsa::tracked fn` names that are both functions and structs.
- `crates/ucil-core/src/lib.rs:8` — crate-level `#![deny(rustdoc::broken_intra_doc_links)]` upgrades the warning to an error.
- `git log --oneline origin/main -- crates/ucil-core/src/incremental.rs` shows the file was last touched by `5c2739a` (WO-0009) and no subsequent commit has modified it.
- `git diff origin/main..feat/WO-0024-kg-crud-and-hot-staging -- crates/ucil-core/src/incremental.rs` = 0 lines.
- `rustdoc` prints the exact fix (`ucil-build/escalations/20260418-0820-pre-existing-incremental-rustdoc-bug.md:50-56`, reproduced in this RCA).
- Verifier's report (`ucil-build/rejections/WO-0024.md:57-83`) reproduces on `/tmp/main-probe` at `origin/main`: identical three-line error output with WO-0024 NOT checked out — definitive.

---

## Remediation

**Who**: **planner → triage → executor** (NOT the WO-0024 executor).

The verifier's companion escalation (`ucil-build/escalations/20260418-0820-pre-existing-incremental-rustdoc-bug.md`) with `severity: harness-config`, `blocks_loop: false`, `requires_planner_action: true` is the upstream signal. It satisfies **every** Bucket-D criterion per `ucil-build/CLAUDE.md` (*Bucket D*):

- Narrow bug-fix in UCIL source — **YES**, 1 file, ≤ 8 lines of diff.
- No affected feature has `attempts >= 2` — **YES**, P1-W4-F02 and P1-W4-F08 both carry `attempts = 0` (this verifier did not call `flip-feature.sh`).
- Unambiguous concrete fix — **YES**, `rustdoc` prints the exact patch.

### What a new micro-WO should look like (for the planner to emit)

**Slug**: `fix-incremental-rustdoc-ambiguity`
**Phase**: 1
**feature_ids**: `[]` (pure admin Bucket-D fix; per the triage protocol these do not flip a feature)
**scope_in** (exactly 2 edits, both in `crates/ucil-core/src/incremental.rs`):
1. Line 5: replace `[`symbol_count`]` → `[`symbol_count()`]`.
2. Line 6: replace `[`dependent_metric`]` → `[`dependent_metric()`]`.

The function form (`()`) is preferred over `struct@` because the surrounding prose (`"two tracked query functions"`, `incremental.rs:5`) refers to the *function* forms; doc clarity is preserved and the rendered HTML will link to the functions (which the reader wants) rather than the internal Salsa ingredient structs.

**acceptance_criteria**:
- `cargo doc -p ucil-core --no-deps 2>&1 | { ! grep -qE '^(warning|error)'; }` (the gate that currently fails).
- `cargo nextest run -p ucil-core incremental::` — all pre-existing tests continue to pass (regression guard).
- `cargo clippy -p ucil-core --all-targets -- -D warnings`.
- `git diff origin/main..HEAD --name-only` touches only `crates/ucil-core/src/incremental.rs`.

**Estimated commits**: 1 · **Estimated diff**: +4 / -2 lines · **Complexity**: trivial.

### After the micro-WO merges

1. Re-verify WO-0024 on the existing tip `7aacaa4` — **no executor work on `feat/WO-0024-kg-crud-and-hot-staging` is required**. A fast-forward rebase onto the micro-WO's merge commit is sufficient (the WO-0024 branch does not conflict with a 2-line change in `incremental.rs`).
2. All 17 previously-green checks remain green (clean `cargo clean && cargo nextest` already executed by retry-1 verifier).
3. Criterion 5 (`cargo doc`) goes green.
4. `flip-feature.sh P1-W4-F02 pass` and `flip-feature.sh P1-W4-F08 pass` run in that subsequent verifier session.
5. WO-0024 merges to `main`. Week-4 cascade unblocks: P1-W4-F03 (symbol resolution), P1-W4-F04 (extraction pipeline), P1-W4-F05 (daemon `find_definition`), P1-W4-F09, P1-W4-F10 (get_conventions), and transitively P1-W5-F02 / P1-W5-F09.

### What the WO-0024 executor should NOT do

- Do **NOT** add `incremental.rs` edits to the `feat/WO-0024-*` branch. That file is outside the WO's `scope_in` (`ucil-build/work-orders/0024-kg-crud-and-hot-staging.json:13-40`) and bundling an orthogonal fix onto a feature branch breaks the "one logical change per commit" hygiene rule (`/home/rishidarkdevil/Desktop/ucil/.claude/rules/commit-style.md`).
- Do **NOT** rebase / `--amend` / force-push the existing WO-0024 commits. `7aacaa4` is merge-viable as-is.
- Do **NOT** open a fresh supersede-WO (WO-0025) for kg-crud-and-hot-staging. The problem is *not* in that code.

---

## If hypothesis H1 is wrong (defensive fallbacks)

Extremely unlikely given the definitive reproduction on `origin/main`, but for completeness:

- **Fallback A**: if the micro-WO lands and `cargo doc -p ucil-core --no-deps` still emits `^warning`/`^error` lines, `grep -rn '\[`' crates/ucil-core/src/` may reveal other unadorned intra-doc links that also resolve to ambiguous Salsa-generated symbols (e.g. if more `#[salsa::tracked]` functions are added later). Remediation: extend the micro-WO's scope to cover all such references (still a narrow Bucket-D fix; the 60-line ceiling is generous).
- **Fallback B**: if the ambiguity ever becomes unfixable via doc-comment surgery (e.g. a future `#[salsa::tracked fn] struct` pattern the macro adopts), the structural escape hatch is a crate-level `#![allow(rustdoc::broken_intra_doc_links)]` on `crates/ucil-core/src/incremental.rs` **with an ADR** justifying the scope-limited allow. Avoid this unless H1's fix is actually insufficient.

---

## Summary for the outer loop

- **WO-0024's executor work is production-quality and ready to merge.** Do not retry the executor.
- **The blocker is an unrelated one-file doc-comment bug** (`crates/ucil-core/src/incremental.rs:5-6`) introduced by WO-0009.
- **The remediation is a Bucket-D micro-WO** authored by the planner (or auto-converted by triage from `escalations/20260418-0820-pre-existing-incremental-rustdoc-bug.md`), **not** any further executor work on `feat/WO-0024-kg-crud-and-hot-staging`.
- After the micro-WO lands, re-verify WO-0024 on tip `7aacaa4` — it will pass clean on the first re-run.
