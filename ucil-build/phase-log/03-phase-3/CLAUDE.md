# Phase 3 — Knowledge Graph + Fusion + CEQP (Weeks 9–12)

> Planner-synthesized phase header pending. Master-plan §18 lines 1799+ govern Phase 3 scope.
> The docs-writer fast-path seeded this file at WO-0067 merge time; planner should backfill
> `## Goal`, `## Features in scope`, `## Gate criteria`, `## Dependencies`, `## Risks carried
> from Phase 2`, and `## Standing rules` sections per the 02-phase-2/CLAUDE.md template before
> phase-3 gate-time.

# Lessons Learned Log

(Seeded by docs-writer; appended after each WO merges.)

## Lessons Learned (WO-0067 — classifier-and-reason-parser)

**Features**: P3-W9-F01 (deterministic classifier), P3-W9-F02 (CEQP reason parser)
**Rejections**: 0 (verifier-green on first attempt; verdict PASS at HEAD `56f4b25`)
**Critic blockers**: none — five soft warnings (commit-LOC over 50-line target, subject >70 char on 5 commits, gate-side artefact commits with non-standard `Work-order:` trailers, AC21/AC22 standing carry-over, `pub mod ceqp;` placement) — all accepted via DEC-0005 module-coherence carve-out and WO-0067 scope_in #42 / scope_out #14
**ADRs raised**: none
**Coverage**: `ucil-core` 97.21% line / 95.60% function (floor 85%); new files `fusion.rs` 93.92%, `ceqp.rs` 97.06%

### What worked

- **Pre-baked mutation contract M1/M2/M3 in the RFR + scope_in #36.** Each mutation named the file, the patch, the targeted SA assertion, and the restore command (`git checkout --`). Verifier applied all three in-place (no `git stash`), all detected on the first run, file md5sums confirmed restored before re-running tests. This is the mature pattern post-DEC-0007 (cargo-mutants gate removed).
- **Frozen-test-at-module-root (DEC-0007) with SA-numbered panic messages.** Every `assert!` / `assert_eq!` carries a `(SAn) <semantic name>; left: …; right: …` panic body. Mutation diagnosis is trivial — verifier reads the panic line, identifies the SA, restores the file. No grepping for which test asserted what.
- **Two coordinated features in one WO around shared master-plan sections** (§3.2 + §6.2 + §7.1 + §8.3 + §8.6). The classifier (`fusion.rs`) and the reason parser (`ceqp.rs`) share the same deterministic-fallback orchestration brain; bundling them avoided cross-feature merge-base churn and let the §6.2 `QUERY_WEIGHT_MATRIX` ship in the same diff as the consumer-shape proof in `test_deterministic_classifier`.
- **`extract_gaps` byte-scan in lieu of `regex`.** UTF-8-safe `[a-z0-9_\- ]` capture with high-bit-set termination + `str::find` for the trigger phrases (`"don't know "`, `"unsure about "`, `"need to learn "`, `"unfamiliar with "`) is a cleaner pattern than pulling `regex` into `crates/ucil-core/Cargo.toml`. AC31 (no new deps) + AC32 (no `regex` import) both green.
- **Module-coherence per DEC-0005 for the NEW `ceqp.rs` file (548 LOC in one commit).** Splitting helper-by-helper would have produced stub-shaped intermediate states AND broken the M3 mutation-restoration contract — M3 reorders the intent precedence ladder which lives entirely in this file. Critic precedent cited from WO-0066 / WO-0042.

### What to carry forward

**For planner**:
- WOs that bundle two coordinated features around a shared master-plan-§N.M contract SHOULD pre-bake one mutation per feature in `scope_in[].acceptance` naming the specific assertion that should fire and the restore command. The verifier should not have to invent the mutation patch — see WO-0067 scope_in #36 + RFR Mutation contract section as the template.
- For pure-deterministic modules (no IO / async / logging / plugins), explicitly note in `scope_in` that master-plan §15.2 tracing spans do NOT apply. Without this, the critic / verifier may flag missing `tracing::instrument` as a divergence. WO-0067 lessons_applied #5 carried this from prior WOs successfully.
- Pre-emptively carve out `ucil-build/verification-reports/**` AND `ucil-build/escalations/**` from `forbidden_paths` for any WO whose acceptance includes AC21/AC22 (phase-N gate). The phase-1/phase-2 effectiveness-evaluator + gate-check child agents commit refresh artefacts and `harness-config` escalations to the feat branch during gate execution. WO-0067 scope_in #42 is the working template.
- The standing-protocol coverage workaround (`scripts/verify/coverage-gate.sh` + sccache `RUSTC_WRAPPER` interaction) is now **22 WOs deep** (WO-0058..WO-0067). Continue citing scope_out #14 + AC23 standing-protocol substantive coverage check until a dedicated harness-improvement WO replaces `coverage-gate.sh` to use `env -u RUSTC_WRAPPER`. Bucket B / Bucket D candidate for triage.
- For `crates/ucil-core` re-exports of submodule public surface: P3-W9-F03/F04 (the consumer WOs that wire `classify_query` + `parse_reason` through the daemon orchestration layer) will need to add re-exports. WO-0067 explicitly deferred them per scope_in #58.

**For executor**:
- When a NEW source file ships, bundle the module declaration in `lib.rs`, all `const` data tables, the public API, and the frozen test in ONE commit per DEC-0005. Splitting helper-by-helper breaks the M3-style mutation-restoration contract and produces stub-shaped intermediate states the critic flags. Cite DEC-0005 + WO-0066 / WO-0042 precedent in the commit body.
- When AC21/AC22 (phase-N gate) is expected to exit 1 due to the standing-protocol coverage workaround, the RFR MUST: (a) cite scope_out #14, (b) report the AC23 standing-protocol measured value (`env -u RUSTC_WRAPPER cargo llvm-cov ...`), (c) call out exit=1 explicitly with the failing sub-check name. Format from WO-0067's "Disclosed Deviations" §1 carries forward.
- Never import `regex` into `crates/ucil-core` for simple sentinel-trigger / vocabulary-match patterns. Use `str::contains` + `str::find` + manual byte-level scan with high-bit-set UTF-8-safe termination. Rationale: `crates/ucil-core` is the workspace-shared dependency root; pulling `regex` here transitively burdens every downstream crate.
- For `Default` trait derivations on enums: `#[default]` should always cite the master-plan rationale in a doc comment (e.g., "most-permissive default per §8.6" for `QueryType::UnderstandCode`, "read-only is the safest default" for `PlannedAction::Read`). Critic check 8 (Doc + public API) examines this; missing rationale is a soft warning.
- `pub mod` declarations in `lib.rs` should be alphabetically sorted; this is the established pattern in `crates/ucil-core/src/lib.rs` (`fusion`, `incremental`, `knowledge_graph`, `otel`, `schema_migration`, `types`). When scope_in says "directly under" but alphabetical placement applies, the alphabetical placement wins per the lib.rs precedent. The verify script's `pub mod <name>` grep is positional-agnostic.

**For verifier**:
- For deterministic-fallback modules with M1/M2/M3 pre-baked in the RFR, apply each mutation via in-place file edit (NOT `git stash`), run the targeted `cargo test -p <crate> <module>::<test>` selector, observe the SA-tagged panic, then restore via `git checkout -- <file>`. Confirm restoration via md5sum match against a `/tmp/<file>-orig.<ext>` snapshot taken before any mutation. WO-0067's "Mutation checks" table is the template.
- `scripts/reality-check.sh` has a **known harness bug**: its unconditional `git stash pop` interacts badly with pre-existing unrelated stashes from earlier session runs (the worktree often carries auto-stashes from Phase 1/2 sessions). Symptom: false-positive merge conflicts in unrelated `verification-reports/coverage-*.md` files. Workaround: the M1/M2/M3 in-place mutation contract from the RFR is the authoritative anti-laziness layer — reality-check.sh is informational only. Bucket B candidate for triage. (Carried from WO-0067 verification report §"Note on `scripts/reality-check.sh`".)
- For `crates/ucil-core` AND `crates/ucil-embeddings` coverage: ALWAYS use `env -u RUSTC_WRAPPER cargo llvm-cov --package <crate> --summary-only --json | jq '.data[0].totals.lines.percent'` (AC23 standing protocol). The gate-script `coverage-gate.sh` produces near-zero (`5%` / `0%`) values due to sccache `RUSTC_WRAPPER` interaction. The actual coverage is consistently ≥85%. Skip the `[FAIL] coverage gate: ucil-{core,embeddings}` lines in `phase-{1,2}.sh` output per scope_out #14.

**Test-type effectiveness (which acceptance_tests caught bugs vs. were ceremonial)**:
- All three mutations (M1 tool_name bypass, M2 row-swap, M3 intent-Understand-first) detected on first run by the frozen-test-at-module-root selector. SA1..SA6 / SA1..SA8 sub-assertions correctly localised the failure. Zero ceremonial assertions — every SA mapped to a load-bearing semantic check (verbatim §6.2 row, §3.2 tool mapping, §8.3 intent variant, JSON wire shape).
- The §6.2 `QUERY_WEIGHT_MATRIX` SA5 sentinel check (Remember row `[0, 0, 3.0, 0, 0, 0, 0, 0]`) is a particularly effective canary against matrix-row-shift bugs — should be carried to any future row-indexed-by-enum-position table elsewhere in the codebase.

### Technical debt incurred

- **None new.** Two pre-existing follow-ups remain:
  1. `scripts/verify/coverage-gate.sh` harness improvement to use `env -u RUSTC_WRAPPER` (now 22 WOs deep).
  2. `scripts/reality-check.sh` pre-existing-stash bug — unconditional `git stash pop` should detect + skip when the popped stash is unrelated.
- Both are Bucket B / Bucket D candidates for triage; neither blocks any feature WO. WO-0067 does not move the needle on either; both inherited from WO-0058+.
