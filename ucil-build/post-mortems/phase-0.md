# Phase 0 Post-Mortem

**Phase**: 0
**Dates**: 2026-04-14 → 2026-04-15
**Features completed**: 14 / 14
**Commits**: 115
**Rejections**: 5
**Escalations**: 9 (all resolved)

## What was built

- **Workspace skeleton** (F01, F10): Cargo workspace with 7 crates (`ucil-core`, `ucil-cli`, `ucil-daemon`, `ucil-treesitter`, `ucil-embeddings`, `ucil-lsp-diagnostics`, `ucil-agents`), pnpm TypeScript workspace under `adapters/`, and uv-managed Python package under `ml/`. Full §17 directory layout verified.
- **Core type system** (F02): Seven domain types in `ucil-core/src/types.rs` — `QueryPlan`, `Symbol`, `Diagnostic`, `KnowledgeEntry`, `ToolGroup`, `CeqpParams`, `ResponseEnvelope` — with serde roundtrip, Clone, and PartialEq tests.
- **Schema migration** (F07): `stamp_version()` / `check_version()` in `schema_migration.rs` with real SQLite (rusqlite + tempfile), including downgrade guard.
- **OpenTelemetry skeleton** (F09): `init_tracer()` in `ucil-core/src/otel.rs` with span creation and shutdown test.
- **`ucil init` command** (F03, F04, F05, F06): CLI entry point that creates `.ucil/`, detects project languages, supports `--llm-provider` selection (F04), runs plugin health verification with timeout guard (F05), and writes `.ucil/init_report.json` (F06).
- **CI pipeline** (F08): `.github/workflows/ci.yml` with Rust (`cargo test` + `clippy`), TypeScript (`biome check` + `pnpm build`), and Python (`ruff` + `mypy` + `pytest`) jobs.
- **Test fixtures** (F11–F14): Four language fixtures committed to `tests/fixtures/`:
  - `rust-project` (~5.8K LOC) with cargo-check integration test (F11)
  - `python-project` (~4.8K LOC expression interpreter) with 4 pytest tests (F12)
  - `typescript-project` (~4.2K LOC task-query engine) with 15 vitest tests (F13)
  - `mixed-project` with intentional lint defects across Rust/TS/Python (F14)

## What broke

- **`scripts/reality-check.sh` pipefail bug** (WO-0001): `grep` returning exit 1 on no-match killed the script silently under `set -euo pipefail`. Verifier worked around with manual mutation checks; fixed in `c18977b`.
- **`reality-check.sh` single-commit rollback** (WO-0002): Script used one `LAST_COMMIT` for all files, causing files introduced in earlier commits to be overwritten with identical content (no actual mutation). Filed escalation `20260415-1630-reality-check-per-file-rollback.md`; fixed with per-file rollback logic.
- **WO-0003 harness bugs** (3 rejections): First rejection found F12/F13/F14 fixtures absent (executor hadn't built them yet). Second rejection confirmed same. Third rejection hit two bugs in `reality-check.sh` (Bug A: `--ignored` tests vanish on rollback, Bug B: zero-test false-negative). Fixed in `d3bee43`. Retry 4 passed.
- **WO-0004 broken selectors** (2 rejections): Tests inside `mod tests {}` produced nextest paths with `::tests::` that didn't match frozen `acceptance_tests` selectors. Co-located tests also vanished during mutation-check rollback. Root-cause-finder identified all three issues (broken selectors, co-located tests, missing F06 trailer). Fixed by moving tests to `crates/ucil-cli/tests/init.rs` with correct module nesting. Retry 3 passed.
- **Missing `tokio::time::timeout`** (WO-0004 retry 1): `verify_plugin_health()` had a bare `.await` on IO without timeout. Caught by critic, rejected by verifier. Fixed in `d2af2f9`.

## Risks carried into Phase 1

- **`reality-check.sh` structural limitation**: The `--ignored` + rollback interaction (DEC-0003 / F11) required a manual mutation check. The script should detect `--ignored` acceptance tests and adjust the zero-test guard. Not yet automated.
- **Harness maturity**: 9 escalations in a single phase suggests the build harness itself needs stabilization. Triage, auto-merge, root-cause-finder, and retry loops were all added mid-phase as reactive fixes.
- **CI not exercised**: The `.github/workflows/ci.yml` (F08) was verified structurally but has not run on a real GitHub Actions runner. Phase 1 will be the first real push.
- **No benchmarks yet**: Phase 0 had no performance targets. Phase 1 introduces tree-sitter parsing where latency matters.

## Metrics

- Lines of code added: ~40,991
- Tests added: 24 Rust (`#[test]` / `#[tokio::test]`), 15 vitest (TS fixture), 4 pytest (Python fixture) = **43 tests**
- Benches added: 0

## Decisions made

- **DEC-0001**: Accept oversized commits in WO-0002 (types.rs 368L, schema_migration.rs 215L) — tightly-coupled domain types require atomic commits.
- **DEC-0002**: Add effectiveness, usability, and end-to-end verification gates — 10 new verification dimensions required from Phase 1 onwards.
- **DEC-0003**: Allow `#[ignore]` for legitimately-slow tests via `// SLOW-TEST:` marker comment — exempts `rust_project_loads` without weakening the pre-commit hook.
- **DEC-0004**: Allow skip markers in `tests/fixtures/mixed-project/` — intentional defect fixtures are exempt from the skip-marker ban.

## Next phase prep

- Planner should prioritize tree-sitter parsing crate (`ucil-treesitter`) and the LMDB/sled tag cache — these are Phase 1 Week 1–2 deliverables per the master plan.
- Executor should verify GitHub Actions CI runs green on the first Phase 1 push.
- The `scripts/reality-check.sh` `--ignored` test detection should be fixed early to avoid repeated manual mutation checks.
- Effectiveness gate scaffolding (DEC-0002) needs initial scenario implementations before Phase 1 gate can pass.
