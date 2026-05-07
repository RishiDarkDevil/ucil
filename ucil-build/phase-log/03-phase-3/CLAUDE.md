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

## Lessons Learned (WO-0068 — cross-group-executor-and-fusion)

**Features**: P3-W9-F03 (cross-group parallel executor), P3-W9-F04 (cross-group RRF fusion)
**Rejections**: 0 (verifier-green on first attempt; verdict PASS at HEAD `4a609f25`)
**Critic blockers**: none — three soft warnings (`.expect()` in private `join_all_cross_group` helper mirroring WO-0047 `join_all_g1` precedent; two single-commit LOC over 50-line target — 788 + 597 LOC — both pre-allowed by scope_in #39 + DEC-0005 module-coherence; `lib.rs` re-export block placed positionally per scope_in #18 contra alphabetical ordering — flagged as planner-ambiguity)
**ADRs raised**: none
**Coverage**: `ucil-core` 97.20% line (floor 85%); new file `cross_group.rs` 97.14%

### What worked

- **Pre-baked M1/M2/M3 mutation contract in the RFR + scope_in #38.** Each named the file, the patch, the targeted SA, and the restore command. Verifier applied all three in-place via `Edit`, took an md5 snapshot before any mutation, restored via `git checkout --`, confirmed md5 match. M3 detected via `usize::MAX` underflow panic on `Group::G1 = 0 → 0 - 1` — a stronger signal than the SA failure alone. Continues the WO-0067 mature pattern.
- **Frozen-test-at-module-root with SA-numbered panic messages (DEC-0007).** Every `assert!`/`assert_eq!` carries `(SAn) <semantic name>; left: …; right: …` body. Verifier reads the panic line, identifies the SA, restores. No grep needed. This is the third consecutive WO (WO-0066, WO-0067, WO-0068) using the pattern with zero verifier friction.
- **Two coordinated features in one WO around shared master-plan §6.1 + §6.2 contracts.** F03 (executor) consumes the `Group` enum the F04 (fusion) ranking surface uses; bundling avoided cross-feature merge-base churn and let `CROSS_GROUP_RRF_K = 60` ship in the same diff as `execute_cross_group`. Same template as WO-0067 (classifier + reason parser, §3.2 + §6.2 + §7.1 + §8.3 + §8.6).
- **DEC-0008 §4 dependency-inversion seam pattern (UCIL-owned `GroupExecutor` trait) avoided the `ucil-core` → `ucil-daemon` cycle.** Real `GroupExecutor` impls (G1Adapter, G2Adapter, G3..G8) land in `ucil-daemon` follow-up WOs paired with their plugin-install WOs. The trait + orchestration shell ships in `ucil-core` standalone — same shape as `G1Source` from WO-0047.
- **Module-coherence per DEC-0005 for the NEW `cross_group.rs` file (788 LOC + 597 LOC test in two cohesive commits).** Splitting helper-by-helper would have produced stub-shaped intermediate states (e.g. `execute_cross_group` without `run_group_executor` would not compile) AND broken the M1/M2/M3 mutation-restoration contract — all three mutations target call-sites inside one file. Critic precedent cited from WO-0067 ceqp.rs (548 LOC), WO-0066, WO-0042.
- **The §6.2 sentinel-row canary scaled.** F04 SA6 reuses the WO-0067 pattern: assert `used_weights == [0, 0, 3.0, 0, 0, 0, 0, 0]` for `QueryType::Remember`. Caught M3 (weight-row off-by-one) immediately.
- **Async frozen test via `#[tokio::test]` at module root.** F03's `test_cross_group_parallel_execution` is `#[tokio::test] pub async fn` and resolves the `cargo test cross_group::test_cross_group_parallel_execution` substring selector after the executor hoisted the tests out of `mod tests { ... }` per DEC-0007.

### What to carry forward

**For planner**:
- **Frozen-test selector substring-match REQUIRES module-root placement.** When prescribing a `cargo test <crate> <module>::<test_name>` selector in `acceptance_criteria`, the test MUST live at module root (NOT inside `mod tests { ... }`). Otherwise the path becomes `<module>::tests::<test_name>` and substring-matching fails. Pre-emptively prescribe `#[cfg(test)] pub async fn` (or sync) at module root in `scope_in` per DEC-0007 + WO-0047 precedent. WO-0068 needed a follow-up refactor commit (#3 in the commit log) to hoist tests after a discovery — future WOs should bake the placement into scope_in.
- **Verify-script grep regex MUST tolerate `pub`/`async` modifiers for async tests.** The pattern `^[[:space:]]*fn test_*` in scope_in #35/36 does not match `pub async fn test_*`. WO-0068 used the relaxed pattern `^[[:space:]]*(pub )?(async )?fn test_*` (or `fn test_*` without line-anchor). Future WOs that prescribe an async frozen test must use the relaxed shape.
- **Per-group-timeout-cap-by-master_deadline directive is mutually exclusive with a deterministic-master-trip SA.** WO-0068 scope_in #12 (b) prescribed `per_group_deadline = std::cmp::min(master_deadline, CROSS_GROUP_PER_GROUP_DEADLINE)` AND SA4 prescribed `master_deadline = 100 ms` trips master. Under tokio's `Timeout::poll` (polls inner-first), the cap collapses both timeouts to 100 ms and the inner per-group timeout fires first — producing `master_timed_out = false` instead of `true`. Future WOs prescribing both contracts must drop one. WO-0068 dropped the cap (per-group constant always); document either choice explicitly so the executor doesn't have to choose.
- **`lib.rs` re-export block: choose ONE — alphabetical OR positional, not both.** WO-0068 scope_in #18 prescribed positional placement ("BETWEEN `fusion::*` and `incremental::*`") AND asserted "alphabetical-by-module ordering preserved" — the two are contradictory because `cross_group` precedes `fusion` alphabetically. Future WOs should pick one rule. The `pub mod` declaration at `lib.rs:11` IS alphabetical (between `ceqp;` and `fusion;`); aligning the re-export block to the same convention is the cleaner choice for future WOs.
- **Standing coverage workaround now 24 WOs deep (WO-0058..WO-0068).** Continue citing scope_out + AC29 standing protocol until a dedicated harness-improvement WO replaces `coverage-gate.sh` to use `env -u RUSTC_WRAPPER`. Same Bucket B / Bucket D triage status as WO-0067.
- **F03/F04 IS the consumer WO that wired `ucil-core` re-exports of `cross_group::*`.** Every downstream Phase 3 WO that consumes `execute_cross_group` / `fuse_cross_group` (G1Adapter wiring, G2Adapter wiring, G3..G8 adapters, MCP-tool dispatch wiring) imports from `ucil_core::*` directly. The deferred re-exports of F01/F02's `classify_query` + `parse_reason` from WO-0067 still need a consumer WO; bundle them into the first daemon-side WO that wires the classify-then-dispatch pipeline.
- **Async-trait workspace dep added with `.workspace = true` (no whitespace before `=`).** AC37's literal grep pattern `^\+[a-z_-]+\s*=` does not match `+async-trait.workspace = true` because of the `.workspace` infix. Future WOs adding workspace deps should either use a more permissive AC pattern (`^\+[a-z_-]+(\.[a-z_-]+)?\s*=`) or document the spirit-vs-literal interpretation in scope_in.

**For executor**:
- **Hoist DEC-0007 frozen tests OUT of `mod tests { ... }` if the cargo-test selector substring-match must resolve.** WO-0067 ceqp.rs ships frozen tests at module root with `#[cfg(test)] pub fn`. WO-0068 initially nested under `mod tests { ... }`, then refactored (commit `07436d5f`) to hoist them out. The pattern: `#[cfg(test)] pub async fn test_*` (or sync) at module root, NOT under `mod tests`. Cite WO-0047 `executor::test_g1_parallel_execution` precedent.
- **Per-group `tokio::time::timeout` deadline MUST be strictly larger than tight master deadlines, not min'd with master_deadline.** Otherwise `Timeout::poll` (polls inner-first) lets the per-group win on tight masters and SA-style "master deadline trips" tests fail non-deterministically. WO-0068 chose `per_group_deadline = CROSS_GROUP_PER_GROUP_DEADLINE` unconditionally (4500 ms) — the per-group only wins on true global stalls (sleeper > 4.5 s), the master only wins on tight budgets (master < 4.5 s). Document the inversion inline at the deadline-computation site if scope_in is contradicted.
- **`.expect()` in private helpers documenting structurally-unreachable invariants is acceptable when a precedent exists.** WO-0068's `join_all_cross_group` mirrors WO-0047's `join_all_g1` (`crates/ucil-daemon/src/executor.rs:1067`). The function returns `Vec<T>` (not `Result`), so propagating the impossibility would require a return-type change with no caller-visible benefit. Cite the precedent in the rustdoc adjacent to the expect — critic check 1 (Warnings) accepts under WO-0047 carry.
- **When scope_in prescribes contradictory rules, follow the literal/positional directive verbatim AND flag the contradiction in the RFR.** WO-0068 followed scope_in #18 placement directive (positional after `fusion::*`) over the alphabetical claim. Critic flagged as planner-ambiguity, not executor failure. Don't try to resolve planner contradictions in-WO — document and move on.
- **Bundle the NEW source file + Cargo.toml dep + `lib.rs` mod decl + `lib.rs` re-export block in ONE cohesive feat commit per DEC-0005**, even if it crosses 500-800 LOC. Splitting (e.g. file-without-mod-decl) produces stub-shaped intermediate states the critic flags. WO-0068 commit 1 (788 LOC) is the template. Cite DEC-0005 + WO-0067 ceqp.rs precedent in the commit body.
- **Word-ban scrub MUST cover comments AND identifiers AND module-level prose.** WO-0068 needed a separate `docs(core): scrub fakes from cross_group production-code comment` commit (`6f6e7b9`) to remove the word "fakes" from a non-`#[cfg(test)]` module-level comment. Test helpers named `AvailableExec` / `SleepingExec` / `ErroringExec` (with `Exec` suffix) under `#[cfg(test)]` are exempt. Pre-flight grep BEFORE first push: `head -n $(grep -n '^#\[cfg(test)\]' <file> | head -1 | cut -d: -f1) <file> | grep -niE 'mock|fake|stub'` — must return empty.

**For verifier**:
- **`models::test_coderankembed_inference` is artefact-gated, not a regression.** `cargo test --workspace --no-fail-fast` panics in `crates/ucil-embeddings/src/models.rs` when the ONNX model + tokenizer are absent. Run `bash scripts/devtools/install-coderankembed.sh` FIRST, then re-run workspace tests — the test passes once artefacts are present (per WO-0059 panic-on-missing-fixture contract). Skip this WO-checks-AC26 step if the installer has already been run in the verifier session env. WO-0068 verification report row #27 documents the workflow.
- **M1/M2/M3 mutation contract: take md5 snapshot via `md5sum <file> > /tmp/<file>-orig.md5sum`, apply via `Edit` (in-place), run targeted `cargo test -p <crate> <module>::<test>` selector, observe SA-tagged panic, restore via `git checkout -- <file>`, confirm md5 matches snapshot.** WO-0068 used `855824ef0218f0ebab278ccdc2b2b621` as the snapshot reference — pattern is now stable across WO-0066, WO-0067, WO-0068.
- **M3-style index-off-by-one mutations on enum-keyed arrays (`weights[g as usize - 1]`) detect via `attempt to subtract with overflow` panic on the `0 - 1` underflow case** (here: `Group::G1 = 0`). This is a stronger detection signal than the SA failure alone — verifier sees the panic immediately. Future WOs with similar enum-keyed array lookups can rely on this.
- **Coverage-gate sccache RUSTC_WRAPPER workaround: `env -u RUSTC_WRAPPER cargo llvm-cov --package <crate> --summary-only --json | jq '.data[0].totals.lines.percent'`.** Now 24 WOs deep. The gate-script `coverage-gate.sh` reports `cargo llvm-cov errored` for `ucil-core`, `ucil-embeddings` under `RUSTC_WRAPPER=sccache`. Skip the `[FAIL] coverage gate: ucil-{core,embeddings}` lines per scope_out + AC29 standing protocol.

**Test-type effectiveness (which acceptance_tests caught bugs vs. were ceremonial)**:
- All three mutations (M1 executor timeout bypass, M2 fusion drops rank term, M3 weight-row off-by-one) detected on first run by the frozen-test-at-module-root selector. SA1..SA7 sub-assertions correctly localised the failures. Zero ceremonial assertions — every SA mapped to a load-bearing semantic check.
- The §6.2 `QUERY_WEIGHT_MATRIX` sentinel-row canary (Remember = `[0, 0, 3.0, 0, 0, 0, 0, 0]`) — already proven in WO-0067 SA5 — proved its value AGAIN in F04 SA6: it caught M3 (weight-row off-by-one) via the underflow panic. Continue using this pattern for any future row-indexed-by-enum-position table.
- The deterministic-master-trip SA4 (`master_deadline = 100 ms` with 7 s sleeper, asserting `master_timed_out == true` AND `wall < 2 s`) is a powerful regression canary against per-group-cap-by-master_deadline misimplementations. Carry to any future fan-out orchestration tests.

### Technical debt incurred

- **None new.** Pre-existing follow-ups carry:
  1. `scripts/verify/coverage-gate.sh` harness improvement to use `env -u RUSTC_WRAPPER` (now 24 WOs deep).
  2. `scripts/reality-check.sh` pre-existing-stash bug (carries from WO-0067).
  3. Planner ambiguity in scope_in #18 (positional placement vs. alphabetical claim) — should be resolved in the next planner pass that touches `lib.rs` re-export blocks. Pure planner-side hygiene; no UCIL source impact.
  4. Planner contradiction in scope_in #12 (b) (per-group cap vs. SA4 deterministic-master-trip) — same pattern: future WOs prescribing async fan-out with both per-group + master deadlines should pick ONE shape and document the rationale.
- WO-0068 inherits but does NOT advance debt items 1 + 2; debt items 3 + 4 are net-new planner-side observations from this WO that future planner work should absorb.
