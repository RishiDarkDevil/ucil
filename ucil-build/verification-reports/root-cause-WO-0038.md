# Root Cause Analysis: WO-0038 (lsp-bridge-integration-test-suite → P1-W5-F08)

**Analyst session**: rca-WO-0038-2026-04-19
**Feature**: P1-W5-F08
**Branch**: `feat/WO-0038-lsp-bridge-integration-test-suite`
**Rejection HEAD**: `8d4365b5c64a87e6358536799d62d5120caa8068` (ready-for-review tip)
**Attempts before RCA**: 1 (retry will be attempt 2)
**Verifier rejection**: `ucil-build/rejections/WO-0038.md`
**Critic report**: `ucil-build/critic-reports/WO-0038.md`

## Failure pattern

`cargo test --test test_lsp_bridge` is **green** — 5 tests pass (4 `#[tokio::test]`
per-fixture bodies + 1 sync coverage-guard). Feature-list `acceptance_tests` for
P1-W5-F08 (`{"kind":"cargo_test","selector":"--test test_lsp_bridge"}`) is
satisfied. The verifier rejected on 2 of the 23 work-order `acceptance_criteria`
shell gates — both are grep-match failures on surface artifacts, not behavioural
failures.

The two failing criteria are **independent defects with different owners**:

1. `acceptance_criteria[0]` — broken regex over `cargo nextest run` output.
   **Planner-side bug**. Cannot be fixed by executor without silently widening
   scope to mutate WO acceptance gates.
2. `acceptance_criteria[17]` — grep expects literal string `tests/fixtures/mixed-project`
   in the test source file; the file cites `mixed-project` 9 times but never
   with the `tests/fixtures/` prefix. **Executor-side oversight**. Trivial
   one-line rustdoc addition.

Neither failure implies a code-correctness problem. All Rust source code on
the branch is correct and the test suite is substantively green.

## Root cause (hypothesis, 99% confidence each — reproduced)

### Defect 1 — `acceptance_criteria[0]` regex over-constrains nextest v0.9 summary

**Claim.** The regex `'tests run: ([5-9]|[1-9][0-9]+).*[0-9]+ passed'` demands
a `<digits> passed` token that appears *later* on the line than the captured
`tests run: <digits>` token. In nextest v0.9.x's summary line, the `<digits>`
right after `tests run: ` **is the same digit** that precedes ` passed` — there
is no second `[0-9]+ passed` match site, so the regex cannot bite.

**Evidence — reproduced at rca-session:**

```
$ cd /home/rishidarkdevil/Desktop/ucil-wt/WO-0038
$ git rev-parse HEAD
d6f27e5eb0b4a4ce6fbb08d3f7d3e9fd5a410d78
$ cargo nextest run --test test_lsp_bridge --no-fail-fast 2>&1 \
    | tee /tmp/rca-WO-0038-nextest.log | tail -3
     Summary [   0.005s] 5 tests run: 5 passed, 0 skipped
$ grep -E 'tests run: ([5-9]|[1-9][0-9]+).*[0-9]+ passed' /tmp/rca-WO-0038-nextest.log
(no match)
$ echo $?
1
$ grep -E 'tests run: ([5-9]|[1-9][0-9]+) passed' /tmp/rca-WO-0038-nextest.log
     Summary [   0.005s] 5 tests run: 5 passed, 0 skipped
$ echo $?
0
```

**Source.** `ucil-build/work-orders/0038-lsp-bridge-integration-test-suite.json:58`
(field `acceptance_criteria[0]`, the first element of the array).

**Why the regex fails.** After `([5-9]|[1-9][0-9]+)` consumes the single `5`,
the cursor sits at ` passed, 0 skipped`. `.*` greedily swallows to end of line,
then backtracks searching for `[0-9]+ passed`. The only digit remaining is the
`0` in `0 skipped`; no amount of backtracking produces a `[0-9]+ passed` site
after the capture. The pattern was presumably written for an older
`running N tests ... N passed` format that nextest no longer emits.

**Confidence: 99%.** Reproduced directly; also matches the verifier's and
critic's independent observation.

### Defect 2 — `tests/fixtures/mixed-project` literal absent from test rustdoc

**Claim.** `acceptance_criteria[17]` greps for the literal
`tests/fixtures/mixed-project` in `tests/integration/test_lsp_bridge.rs`. The
three sibling tests include this literal in their rustdoc:

- `rust-project` → `tests/integration/test_lsp_bridge.rs:344`
- `python-project` → `tests/integration/test_lsp_bridge.rs:432`
- `typescript-project` → `tests/integration/test_lsp_bridge.rs:509`

The fourth sibling (`test_mixed_project_diagnostics_and_calls`) rustdoc at
`tests/integration/test_lsp_bridge.rs:583-588` omits the `tests/fixtures/`
prefix. The file cites `mixed-project` at lines 6, 583, 587, 591, 592, 593,
672, 688, 705, 715, 736 — **none** prefixed with `tests/fixtures/`.

**Evidence — reproduced at rca-session:**

```
$ grep -q 'tests/fixtures/rust-project'       tests/integration/test_lsp_bridge.rs ; echo $?
0
$ grep -q 'tests/fixtures/python-project'     tests/integration/test_lsp_bridge.rs ; echo $?
0
$ grep -q 'tests/fixtures/typescript-project' tests/integration/test_lsp_bridge.rs ; echo $?
0
$ grep -q 'tests/fixtures/mixed-project'      tests/integration/test_lsp_bridge.rs ; echo $?
1
```

**Source.** `tests/integration/test_lsp_bridge.rs:583-588` — rustdoc for
`test_mixed_project_diagnostics_and_calls`. The sibling precedent at
line 344 (rust), line 432 (python), line 509 (typescript) demonstrates the
canonical format: `tests/fixtures/<name>/src/<file>.<ext>` appears as the
terminal segment of the rustdoc paragraph.

**Confidence: 99%.** Same defect the critic flagged on 2026-04-19 and the
verifier confirmed. Single-word literal omission.

## Remediation

**Two remediations are required; one alone is NOT sufficient.**

### Remediation A (planner) — fix `acceptance_criteria[0]`

**Who**: planner
**What**: amend `ucil-build/work-orders/0038-lsp-bridge-integration-test-suite.json`
field `acceptance_criteria[0]`. Replace

```
cd $REPO_ROOT && cargo nextest run --test test_lsp_bridge --no-fail-fast 2>&1 | tee /tmp/nextest-WO-0038.log && grep -E 'tests run: ([5-9]|[1-9][0-9]+).*[0-9]+ passed' /tmp/nextest-WO-0038.log > /dev/null
```

with a regex that matches nextest v0.9's actual summary line, e.g.:

```
cd $REPO_ROOT && cargo nextest run --test test_lsp_bridge --no-fail-fast 2>&1 | tee /tmp/nextest-WO-0038.log && grep -E 'tests run: ([5-9]|[1-9][0-9]+) passed' /tmp/nextest-WO-0038.log > /dev/null
```

(remove the `.*[0-9]+` tokens between `tests run: <count>` and ` passed`).

**Alternative**: drop `acceptance_criteria[0]` entirely. `acceptance_criteria[1]`
(`cargo test --test test_lsp_bridge --no-fail-fast && ! grep FAILED`) already
validates test passage with less surface-fragility. The two criteria are
redundant.

**Commit message suggestion**:
`chore(planner): amend WO-0038 acceptance_criteria[0] — fix nextest v0.9 regex`
with Phase/Feature/Work-order trailers.

**Acceptance**: after amend, the criterion must match the real nextest output
(test locally via `grep -E <new_pattern> /tmp/rca-WO-0038-nextest.log`).

**ADR opportunity (optional, not required for this retry)**: if the planner
expects future WOs to assert nextest summary lines, a short ADR codifying the
exact grep idiom (and calling out the v0.9 format) avoids recurrence. Not
blocking.

### Remediation B (executor) — add `tests/fixtures/mixed-project` rustdoc literal

**Who**: executor
**What**: in `tests/integration/test_lsp_bridge.rs`, extend the rustdoc block
at lines 583-588 to include the literal `tests/fixtures/mixed-project` path.
Mirror the sibling precedent at `:344`, `:432`, `:509`.

Suggested text (exact wording flexible; the hard constraint is that the
string `tests/fixtures/mixed-project` must appear somewhere in the file):

```rust
/// P1-W5-F08 — `mixed-project` fixture: three diagnostics (one per
/// language) land as three `quality_issues` rows referencing
/// `tests/fixtures/mixed-project/src/{main.rs,main.py,index.ts}`.  A
/// single incoming-call dispatch against the `.rs` symbol asserts the
/// relation endpoint path ends with `main.rs` (the mixed-project rust
/// half).
```

**Commit message suggestion**:
`docs(tests-integration): cite tests/fixtures/mixed-project in test rustdoc`
with Phase/Feature/Work-order trailers. Also update `ucil-build/work-orders/0038-ready-for-review.md`
`final_commit` field to the new sha.

**Acceptance**: `grep -q 'tests/fixtures/mixed-project' tests/integration/test_lsp_bridge.rs`
exits 0.

**Risk**: negligible. Rustdoc-only, no code behaviour change. Local
`cargo doc --workspace --no-deps` must still render clean (no broken intra-doc
links from the new text) — the suggested wording uses only backtick literals,
no doc-link syntax.

### Order of operations for the retry

Both remediations are independent and commit to different files
(`ucil-build/work-orders/0038-*.json` vs `tests/integration/test_lsp_bridge.rs`).
They can be applied in either order. The verifier will re-run all 23 criteria
from clean-slate on the next spin; both gates must be green.

## Passing state (for context)

21 of 23 acceptance_criteria already PASS on the rejection HEAD (see
`ucil-build/rejections/WO-0038.md:116-142`). `cargo test --test test_lsp_bridge`
is green with 5 passed / 0 failed. `cargo clippy --workspace --all-targets
-- -D warnings` is clean. `cargo doc --workspace --no-deps` is clean.
`cargo fmt --check` is clean. The diff respects every `forbidden_path`. No
stubs, no `#[ignore]`, no mocks of critical deps, every `.await` on the bridge
is timeout-wrapped.

## If either hypothesis is wrong

**If A is wrong** (unlikely; reproduced twice): the alternative regex
`'([5-9]|[1-9][0-9]+) tests run'` (anchored on the *leading* count token)
also matches the v0.9 summary. Either anchor style works.

**If B is wrong** (unlikely; critic + verifier + rca all saw the same miss):
the backup is to add the literal to a `const FIXTURE_PATHS: &[&str]` array
inside the file, or as a doc-only comment on the `FIXTURES` array at
line 715. Any site containing the literal bytes satisfies the grep.

## Related ADRs + references

- `DEC-0010` (`ucil-build/decisions/DEC-0010-tests-integration-workspace-crate.md`)
  — this WO ships the `tests/integration` workspace crate per the DEC-0010
  layout. No DEC-0010 obligations are breached by either remediation.
- `DEC-0005` (`ucil-build/decisions/DEC-0005-WO-0006-module-coherence-commits.md`)
  — tests live at module root (no `mod tests { }` wrapper). Honoured on this
  branch.
- `.claude/rules/rust-style.md` (timeouts on every IO await) — honoured at
  `tests/integration/test_lsp_bridge.rs:91,388,409,473,490,550,567,645,652,659,676`.

## Summary for the outer loop

| Action | Owner | File | Size |
|--------|-------|------|------|
| Fix nextest regex | **planner** | `ucil-build/work-orders/0038-lsp-bridge-integration-test-suite.json` (field `acceptance_criteria[0]`) | ~1 line |
| Add `tests/fixtures/mixed-project` literal | **executor** | `tests/integration/test_lsp_bridge.rs:583-588` rustdoc | ~1 line |
| Update ready-for-review final_commit | **executor** | `ucil-build/work-orders/0038-ready-for-review.md` | ~1 line |

Both are trivial in code footprint and orthogonal in scope. A retry that
applies both should clear verifier on attempt 2 without further intervention.
No ADR is strictly required, though the planner may optionally land one to
codify a nextest-summary regex idiom for future WOs.
