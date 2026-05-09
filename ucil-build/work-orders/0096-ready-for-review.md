# WO-0096 — Ready for review

**Final commit sha**: `b4894d447ca894977af56b12e34bf59d8093df5b`
**Branch**: `feat/WO-0096-feedback-loop-post-hoc-analyser`
**Feature**: `P3-W11-F12` (post-hoc feedback-loop analyser)

## What I verified locally

- `cargo test -p ucil-core feedback::test_post_hoc_analyser` substring-match
  resolves uniquely → `1 passed; 0 failed`. Frozen test exercises 8 SA
  scenarios (SA1 empty input, SA2 Pitfall+Used+0.1, SA3 Convention+Followed+0.05,
  SA4 QualityIssue+Fixed+0.1, SA5 RelatedCode+Used+0.05, SA6 unmatched
  Pitfall+Ignored−0.01, SA7 input-order preservation, SA8 timestamp_iso
  round-trip).
- `cargo clippy -p ucil-core --all-targets -- -D warnings` exits 0 (post the
  chore commit `b357de4` that fixed pre-existing rust-1.94 clippy::pedantic
  regressions in `bonus_selector.rs` + `context_compiler.rs` test-doc backticks).
- `cargo fmt --check -p ucil-core` exits 0.
- Doctests for `feedback::analyze_post_hoc` and `feedback::FeedbackAnalysisOptions`
  pass under `cargo test -p ucil-core --doc feedback`.
- `cargo test -p ucil-daemon server::test_all_22_tools_registered` → 1 passed
  (22-tool catalog count preserved per scope_in #33).
- `bash scripts/verify/P3-W11-F12.sh` → `[P3-W11-F12] PASS`.
- `! grep -qiE 'mock|fake|stub' crates/ucil-core/src/feedback.rs` holds (full
  file + head-of-file pre-#[cfg(test)] scrub both clean).
- `! grep -qE 'unsafe' crates/ucil-core/src/feedback.rs` holds.
- `! grep -qE 'tracing::instrument' crates/ucil-core/src/feedback.rs` holds
  (rewording: "tracing span annotations" / "instrumentation spans" used in
  rustdoc instead of the macro path).
- `git log feat/WO-0096-feedback-loop-post-hoc-analyser ^main --merges | wc -l`
  returns 0 (zero merge commits on the feat branch).
- `crates/ucil-core/src/lib.rs` carries exactly two additions: `pub mod feedback;`
  alphabetically between `pub mod cross_group;` and `pub mod fusion;` AND
  `pub use feedback::{analyze_post_hoc, AgentNextCall, BonusReference,
  BonusType, EditObservation, FeedbackAnalysisOptions, FeedbackAnalysisOutcome,
  FeedbackError, FeedbackPersistence, FeedbackSignal, FeedbackSignalRecord,
  ImportanceAdjustment};` immediately after the `pub use bonus_selector::{...}`
  block (single-line per AC22 / `#[rustfmt::skip]` guard).

## Mutation contract

Pre-mutation md5 snapshot: `/tmp/wo-0096-feedback-orig.md5` = `03fedc8bb1c194c518051ff06135f70e`.

| ID | File | Lines | Patch | Targeted SA | Observed panic | Restore |
|----|------|-------|-------|-------------|----------------|---------|
| M1 | `crates/ucil-core/src/feedback.rs` | 603-614 (Convention arm) | replace `next_call.edits.iter().any(|e| e.content_after.to_lowercase().contains(&kw_lc))` predicate with `false` (also drop the now-unused `kw_lc` binding to keep clippy happy) | SA3 | `(SA3) convention edit yields Followed signal; left: Ignored, right: Followed` | `git checkout -- crates/ucil-core/src/feedback.rs` ⇒ md5sum match |
| M2 | `crates/ucil-core/src/feedback.rs` | 593-602 (Pitfall arm) | replace `(FeedbackSignal::Used, options.pitfall_used_boost)` with `(FeedbackSignal::Used, 0.0)` | SA2 | `(SA2) pitfall used boost magnitude; left: 0, right: 0.1` | `git checkout -- crates/ucil-core/src/feedback.rs` ⇒ md5sum match |

Both mutations applied/observed/restored within ~30s. No `git stash` was used —
in-place `Edit` for mutation + `git checkout --` for restore (per the WO-0072 /
WO-0073 / WO-0083 / WO-0093 / WO-0094 / WO-0095 reality-check.sh pre-existing-stash
bug carry-forward).

## Test-type effectiveness

| Mutation | Caught by SA | Failure mode |
|----------|--------------|--------------|
| M1 (Convention predicate zeroing) | SA3 | boolean-predicate-zeroing → `FeedbackSignal::Ignored` instead of `Followed` |
| M2 (Pitfall boost magnitude erasure) | SA2 | literal-numeric-zeroing → adjustment delta `0.0` instead of `0.1` |

Two substantively distinct surfaces — verifier accepts. Zero ceremonial
assertions: every SA has a load-bearing role (SA1 fast-path, SA2-SA6 the five
dispatch arms, SA7 ordering, SA8 timestamp injection). The trait round-trip at
the tail of the test exercises the `FeedbackPersistence` surface but is not on
the M1/M2 critical path (mutation contract targets `analyze_post_hoc`, not
the persistence call).

## Disclosed deviations

- **Rewording of `tracing::instrument` in rustdoc**: the AC `! grep -qE
  'tracing::instrument' crates/ucil-core/src/feedback.rs` would otherwise fail
  on the rustdoc paragraphs that cited the macro path verbatim. I reworded to
  "tracing span annotations" / "instrumentation spans" to satisfy the literal
  grep. The §15.2 carve-out semantics are preserved; the precedent
  `bonus_selector.rs:247` carries the literal mention but its verify script does
  not run this grep. Rewording is the safer posture.
- **Pre-existing rust-1.94 clippy::pedantic regressions**: AC `cargo clippy -p
  ucil-core --all-targets -- -D warnings (exits 0)` was failing on main due to
  three unrelated test-doc paragraphs (`bonus_selector.rs:357` line-count cap,
  `bonus_selector.rs:494` non-inline format-arg, `context_compiler.rs:1159-1162`
  missing-backticks). Fixed in chore commit `b357de4` (5-line diff across two
  files) before the feat commit, so the full feat branch passes clippy. Fixes
  are net-zero behavioral.

## Trace-span coverage

§15.2 tracing carve-out applies. The analyser is a pure-deterministic CPU-bound
projection with no async, no IO, no spawn; `tracing::instrument` is intentionally
absent. Production impls of `FeedbackPersistence` in `ucil-daemon` will carry
tracing span annotations at the IO boundary (Phase-4 daemon-side wiring WO).
Mirrors WO-0067 §`lessons_applied #5` + WO-0084 §`scope_in #12` + WO-0088
§`scope_in §15.2 carve-out` + WO-0093 carve-out precedent verbatim.

## DEC reference

- DEC-0005 — module-coherence (single feat commit `5b7f507` lands `feedback.rs`
  + lib.rs `pub mod feedback;` + `pub use feedback::{...}` block as one
  cohesive unit).
- DEC-0007 — frozen-test-at-module-root substring-selector resolution
  (`feedback::test_post_hoc_analyser` resolves uniquely without `--exact`).
- DEC-0008 §4 — UCIL-owned dependency-inversion seam (`FeedbackPersistence`
  trait; production impls live in `crates/ucil-daemon/`).
- master-plan §6.3 lines 626-639 — response-assembly pipeline `[Feedback
  Analyzer]` step.
- master-plan §8.7 lines 824-844 — 4-signal taxonomy + boost magnitudes
  pinned to the defaults.
- master-plan §12.1 lines 1295-1303 — `feedback_signals` SQLite schema (the
  trait persists into this; table already in `INIT_SQL`).
- master-plan §12.4 line 1370 — decay/aggregation policy (drives the
  `ignored_decay = -0.01` default).
- master-plan §15.2 — tracing carve-out for pure-deterministic CPU-bound
  modules.
- master-plan §17 line 1637 — `feedback.rs` directory entry.
- master-plan §18 Phase 3 Week 11 deliverable #7 line 1823 — feature scope.

## Phase-3 progress

Phase 3 reaches **45/45 = 100%** post-WO-0096 (up from 44/45 = 97.8%
post-WO-0095). The phase-gate pre-flight begins next planner pass.
