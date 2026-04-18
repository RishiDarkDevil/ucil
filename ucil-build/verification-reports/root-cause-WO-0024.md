# Root Cause Analysis: WO-0024 (kg-crud-and-hot-staging; features P1-W4-F02 + P1-W4-F08)

**Analyst session**: rca-WO-0024-20260418-retry2
**Work-order**: `ucil-build/work-orders/0024-kg-crud-and-hot-staging.json`
**Branch**: `feat/WO-0024-kg-crud-and-hot-staging`
**HEAD**: `7aacaa4da7707e09919130c66a946cd7e34ee9bf` (unchanged from retry 1)
**Attempts before RCA**: 2 (retry 1 rejected 2026-04-18T08:18:56Z; retry 2 rejected 2026-04-18T08:31:58Z)
**Features touched**: P1-W4-F02, P1-W4-F08
**Supersedes**: retry-1 RCA at same path (same analyst conclusion, now confirmed across two verifier sessions)

---

## Failure pattern

**Two consecutive verifier rejections** (`ucil-build/rejections/WO-0024.md` is the retry-2 overwrite; retry-1 body is in git history at commit `2d940de`). Both rejections have **identical** failure signatures:

- 17 of 18 verifier acceptance checks **PASS**.
- The same single check **FAILS** both times:

```
cargo doc -p ucil-core --no-deps 2>&1 | { ! grep -qE '^(warning|error)'; }
```

- Branch tip `7aacaa4` is bit-for-bit unchanged between retry 1 and retry 2 (the executor correctly performed no work on this branch between verifier cycles — per retry-1 RCA §"What the WO-0024 executor should NOT do", `ucil-build/verification-reports/root-cause-WO-0024.md` prior revision).
- **`git diff origin/main..HEAD -- crates/ucil-core/src/incremental.rs` = 0 lines** at both retry 1 and retry 2.

Retry 2 confirms the retry-1 diagnosis with zero new variance: the bug is pre-existing, the executor's work is untouched, and nothing in the WO-0024 diff is a contributor. This is a **pure administrative rejection loop**: the verifier is mechanically correct to reject (it has no authority to waive criterion #5 per `.claude/agents/verifier.md:43-45` — "Any failure → OVERWRITE rejection"), but the executor has nothing to change.

---

## Reproduction (this RCA session, 2026-04-18 ~14:00 UTC)

```
$ cd /home/rishidarkdevil/Desktop/ucil-wt/WO-0024
$ cargo doc -p ucil-core --no-deps 2>&1 | grep -E '^(warning|error)'
error: `symbol_count` is both a function and a struct
error: `dependent_metric` is both a function and a struct
error: could not document `ucil-core`

$ git -C /home/rishidarkdevil/Desktop/ucil-wt/WO-0024 log -1 --oneline
7aacaa4 chore(work-order): WO-0024 ready-for-review marker — P1-W4-F02 + P1-W4-F08

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

### H1 (confidence 99.5%, ↑ from retry 1's 99%): Pre-existing rustdoc ambiguity in `incremental.rs` is the sole cause; WO-0024's work is clean; the Bucket-D fix has not been emitted, so the retry loop cannot converge

- **Evidence (retry-2 specific)**:
  - Retry-2 rejection (`ucil-build/rejections/WO-0024.md:15-29`) explicitly records that HEAD is unchanged (`7aacaa4` → `7aacaa4`), source diff on `incremental.rs` remains 0 lines, and the same three `error:` lines are emitted.
  - Retry-2 verifier re-ran the `cargo clean && cargo doc` cycle from a pristine worktree and reproduced the failure (`ucil-build/rejections/WO-0024.md:47-61`).
  - Retry-2 verifier additionally probed `origin/main` in an ephemeral worktree `/tmp/main-probe-r2` (`ucil-build/rejections/WO-0024.md:79-89`) and reproduced the identical three-line error output with WO-0024 NOT checked out — definitive proof the regression is on `main` and not WO-0024-introduced.
- **Cheap falsifier already run (twice now)**: `git diff origin/main..HEAD -- crates/ucil-core/src/incremental.rs | wc -l = 0` across both retries. Dispositive.
- **New signal in retry 2**: Zero work-order source changed between retry 1 and retry 2 (the executor correctly did nothing; the orchestrator re-ran the verifier on the unchanged branch). This is NOT an executor regression — it is the expected behaviour when an RCA says "no executor work required" but the planner has not yet emitted the unblocking Bucket-D micro-WO.

### H2 (confidence <0.3%): Some environmental flake (rustdoc version, cargo cache) caused a false failure

- **Why negligible**: Retry 1 and retry 2 ran on different verifier sessions with fresh `cargo clean && git clean -fdx` invocations; both produced the identical three-line error. Retry 2 additionally reproduced the failure on `origin/main` in an ephemeral worktree. Deterministic, not flaky.

### H3 (confidence <0.2%): Planner should broaden WO-0024's `scope_in` to include `incremental.rs`

- **Why negligible**: piggy-backing an orthogonal fix onto the feature branch would break the hygienic "one logical change per commit" rule (`/home/rishidarkdevil/Desktop/ucil/.claude/rules/commit-style.md`) and force an additional verifier cycle on a WO the critic already cleared. Bucket-D micro-WOs exist precisely for this scenario (`ucil-build/CLAUDE.md` — *Escalation protocol / Bucket D*). Re-scoping a merged-ready WO to include a one-file doc fix is strictly worse on every axis (commit hygiene, review-friendliness, verifier retry count).

---

## Root cause (hypothesis H1, 99.5% confidence)

**The sole acceptance failure is a pre-existing rustdoc regression in `crates/ucil-core/src/incremental.rs:5-6` introduced by WO-0009 (commit `5c2739a`), not by WO-0024.** The loop is stuck because the Bucket-D micro-WO recommended in the retry-1 RCA has not yet been emitted by the planner (or auto-converted by triage from `ucil-build/escalations/20260418-0820-pre-existing-incremental-rustdoc-bug.md`). Until that micro-WO lands on `main`, every re-verification of WO-0024 will fail on the same criterion with the same error lines and no observable change.

Supporting evidence (cumulative across retry 1 + retry 2):
- `crates/ucil-core/src/incremental.rs:5-6` — two unadorned intra-doc links to `salsa::tracked fn` names that are both functions and structs. Source unchanged since WO-0009.
- `crates/ucil-core/src/lib.rs:8` — crate-level `#![deny(rustdoc::broken_intra_doc_links)]` upgrades the warning to an error.
- `git log --oneline origin/main -- crates/ucil-core/src/incremental.rs` shows the file's last touching commit is `5c2739a` (WO-0009) — the file has been untouched by all WO-0010 through WO-0024 work.
- `git diff origin/main..feat/WO-0024-kg-crud-and-hot-staging -- crates/ucil-core/src/incremental.rs` = 0 lines (confirmed retry 1, retry 2, and this RCA session).
- `rustdoc` emits the exact fix (`help: to link to the function, add parentheses`) in its own diagnostic — no design judgement required.
- Retry-2 `/tmp/main-probe-r2` reproduction on `origin/main` (no WO-0024 commits present) emits the same three `error:` lines: definitive that the 3-error delta WO-0024 introduces over `origin/main` is **0**.
- Critic verdict `CLEAN` (`ucil-build/critic-reports/WO-0024.md:10`); 8/8 knowledge_graph tests PASS; 97.10% line coverage; reality-check mutation confirms real code; zero stubs / mocks / forbidden-path diffs.

---

## Remediation

**Who**: **planner → triage → (future) executor of a new micro-WO**. The WO-0024 executor has nothing left to do. `feat/WO-0024-kg-crud-and-hot-staging` tip `7aacaa4` is merge-ready as-is.

### Why this cannot go to the WO-0024 executor

The orchestrator may be tempted to route this back to the executor as "retry 3" context. That is a **wrong** routing because:

1. The `scope_in` of WO-0024 (`ucil-build/work-orders/0024-kg-crud-and-hot-staging.json:13-40`) does not include `crates/ucil-core/src/incremental.rs`. Touching that file on the WO-0024 branch would violate `scope_out` and the commit-style "one logical change per commit" rule.
2. `attempts` on P1-W4-F02 and P1-W4-F08 both remain `0` in `feature-list.json` (verifier did not flip anything on a reject path). The feature-attempt ceiling (3) is not reached and further executor retries on WO-0024 would spin the attempts counter on features whose implementation is already correct.
3. The fix (`s/[\`symbol_count\`]/[\`symbol_count()\`]/` and same for `dependent_metric`) belongs in `incremental.rs`, a file owned by a different feature (P1-W3-F06, Salsa incremental engine). Modifying that file is the concern of the Bucket-D micro-WO, not WO-0024.
4. Retry 3 on an unchanged branch would deterministically reproduce the same failure — burning a verifier cycle with zero information gain.

### Preferred path — planner emits Bucket-D micro-WO

The verifier's companion escalation (`ucil-build/escalations/20260418-0820-pre-existing-incremental-rustdoc-bug.md`) with `severity: harness-config`, `blocks_loop: false`, `requires_planner_action: true` is the upstream signal. It satisfies **every** Bucket-D criterion per `ucil-build/CLAUDE.md` (*Bucket D*):

- Narrow bug-fix in UCIL source — **YES**, 1 file, ≤ 8 lines of diff.
- No affected feature has `attempts >= 2` — **YES**, P1-W4-F02 and P1-W4-F08 both carry `attempts = 0` (verifier does not call `flip-feature.sh` on a reject path).
- Unambiguous concrete fix — **YES**, `rustdoc` prints the exact patch.

**Proposed micro-WO** (unchanged from retry-1 RCA — re-stated here so this retry-2 RCA is self-contained):

- **Slug**: `fix-incremental-rustdoc-ambiguity`
- **Phase**: 1
- **feature_ids**: `[]` (pure admin Bucket-D fix — does not flip a feature)
- **scope_in** (exactly 2 edits, both in `crates/ucil-core/src/incremental.rs`):
  1. Line 5: replace `[`symbol_count`]` → `[`symbol_count()`]`.
  2. Line 6: replace `[`dependent_metric`]` → `[`dependent_metric()`]`.
- **acceptance_criteria**:
  - `cargo doc -p ucil-core --no-deps 2>&1 | { ! grep -qE '^(warning|error)'; }` (the gate that currently fails).
  - `cargo nextest run -p ucil-core incremental::` — all pre-existing incremental::test_* continue to pass (regression guard).
  - `cargo clippy -p ucil-core --all-targets -- -D warnings`.
  - `git diff origin/main..HEAD --name-only` touches only `crates/ucil-core/src/incremental.rs`.
- **Estimated commits**: 1 · **Estimated diff**: +4 / -2 lines · **Complexity**: trivial.

The function form (`()`) is preferred over `struct@` because the surrounding prose (`"two tracked query functions"`, `incremental.rs:5`) refers to the *function* forms; doc clarity is preserved and the rendered HTML links to the functions (which the reader wants) rather than the internal Salsa ingredient structs.

### Alternative remediation — triage auto-converts

If the orchestrator reaches `scripts/run-phase.sh` triage pass 1 before the planner acts, the triage agent should auto-convert the escalation per Bucket-D policy (`.claude/agents/triage.md` — "concrete < 60-line fix in UCIL source"). The escalation already proposes the micro-WO scope; triage's job is to write the WO JSON and append `## Resolution — converted to WO-NNNN` + `resolved: true` to the escalation file.

### After the micro-WO lands on `main`

1. The orchestrator re-verifies WO-0024 on the existing tip `7aacaa4` (or a fast-forward rebase onto the micro-WO's merge commit — no conflict possible because WO-0024 does not touch `incremental.rs`). **No executor work on `feat/WO-0024-kg-crud-and-hot-staging` is required.**
2. All 17 previously-green checks remain green (they were already re-run from a clean-slate `cargo clean` by retry-2 verifier).
3. Criterion 5 (`cargo doc`) turns green.
4. `flip-feature.sh P1-W4-F02 pass` and `flip-feature.sh P1-W4-F08 pass` run in that verifier session.
5. WO-0024 merges to `main`. Week-4 cascade unblocks: P1-W4-F03 (symbol resolution), P1-W4-F04 (extraction pipeline), P1-W4-F05 (daemon `find_definition`), P1-W4-F09, P1-W4-F10 (get_conventions), and transitively P1-W5-F02 / P1-W5-F09.

### What the WO-0024 executor must NOT do

- Do **NOT** add `incremental.rs` edits to the `feat/WO-0024-*` branch. That file is outside the WO's `scope_in` (`ucil-build/work-orders/0024-kg-crud-and-hot-staging.json:13-40`) and bundling an orthogonal fix onto a feature branch breaks the "one logical change per commit" hygiene rule (`/home/rishidarkdevil/Desktop/ucil/.claude/rules/commit-style.md`).
- Do **NOT** rebase / `--amend` / force-push the existing WO-0024 commits. `7aacaa4` is merge-viable as-is.
- Do **NOT** open a fresh supersede-WO (WO-0025) for `kg-crud-and-hot-staging`. The problem is *not* in that code. Retry 1 + retry 2 produced identical verifier telemetry on an unchanged branch — the remediation has not moved.
- Do **NOT** treat this retry-2 rejection as "another two attempts exhausted" on WO-0024 — the executor's `attempts` budget is measured per-executor-cycle, and the executor has had zero substantive work to do since retry 1 closed.

---

## If hypothesis H1 is wrong (defensive fallbacks — unchanged from retry 1)

Extremely unlikely given the two-session deterministic reproduction on `origin/main`, but for completeness:

- **Fallback A**: if the micro-WO lands and `cargo doc -p ucil-core --no-deps` still emits `^warning`/`^error` lines, `grep -rn '\[`' crates/ucil-core/src/` may reveal other unadorned intra-doc links that also resolve to ambiguous Salsa-generated symbols. Remediation: extend the micro-WO's scope to cover all such references (still a narrow Bucket-D fix; the 60-line ceiling is generous).
- **Fallback B**: if the ambiguity ever becomes unfixable via doc-comment surgery, the structural escape hatch is a file-level `#![allow(rustdoc::broken_intra_doc_links)]` on `crates/ucil-core/src/incremental.rs` **with an ADR** justifying the scope-limited allow. Avoid this unless H1's fix is actually insufficient.

---

## Loop-convergence note (new in retry 2)

The retry-1 → retry-2 cycle has demonstrated that **re-running the verifier on an unchanged branch produces an unchanged result**. Further retries against the current state will also produce unchanged results. The orchestrator must break this loop by routing the Bucket-D micro-WO recommendation upward (to the planner / triage), NOT by issuing another verifier pass on `feat/WO-0024-*`. Until the micro-WO is emitted and merged to `main`, WO-0024 re-verification is guaranteed to fail on criterion #5 and guaranteed to pass all 17 other criteria.

**Recommended orchestrator action on this RCA output**:
1. Halt further WO-0024 verifier cycles until `incremental.rs` is fixed on `main`.
2. Invoke the planner to emit the Bucket-D micro-WO (or let triage auto-convert the escalation).
3. Once that micro-WO is merged, re-invoke the verifier on WO-0024's unchanged tip `7aacaa4`.

---

## Summary for the outer loop

- **WO-0024's executor work remains production-quality and merge-ready** (critic `CLEAN`; 8/8 tests PASS; 97.10% line coverage; mutation oracle confirms real code; zero stubs / mocks / forbidden-path diffs).
- **The blocker is still the same unrelated one-file doc-comment bug** (`crates/ucil-core/src/incremental.rs:5-6`) introduced by WO-0009 commit `5c2739a`.
- **The remediation is still a Bucket-D micro-WO** authored by the planner (or auto-converted by triage from `escalations/20260418-0820-pre-existing-incremental-rustdoc-bug.md`), **not** any further executor work on `feat/WO-0024-kg-crud-and-hot-staging`.
- **Retries without a micro-WO are guaranteed non-progress** — retry 1 and retry 2 produced bit-identical verifier telemetry on a bit-identical branch tip. The orchestrator must unblock upstream, not re-verify downstream.
- After the micro-WO lands, re-verify WO-0024 on tip `7aacaa4` — it will pass clean on the first re-run, both features flip to `passes=true`, and the Week-4 cascade unblocks.
