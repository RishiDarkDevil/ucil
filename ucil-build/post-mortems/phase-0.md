# Phase 0 Post-Mortem

**Phase**: 0
**Dates**: 2026-04-14 → 2026-04-15
**Features completed**: 14 / 14
**Commits**: 113 total from `edde83f` Initial commit through `3b9ff72` WO-0004 merge (39 feature-tagged via `Feature: P0-W1-F*` trailer; remainder harness/chore/meta)
**Work-orders**: 4 (WO-0001, WO-0002, WO-0003, WO-0004)
**Rejections**: 5 total (WO-0001: 0 · WO-0002: 1 · WO-0003: 3 · WO-0004: 2)
**Escalations**: 9 (all resolved)

## What was built

- **Workspace skeleton** (F01, F10): Cargo workspace with 7 crates (`ucil-core`, `ucil-cli`, `ucil-daemon`, `ucil-treesitter`, `ucil-embeddings`, `ucil-lsp-diagnostics`, `ucil-agents`), pnpm TypeScript workspace under `adapters/`, and uv-managed Python package under `ml/`. Full §17 directory layout verified.
- **Core type system** (F02): Seven domain types in `ucil-core/src/types.rs` — `QueryPlan`, `Symbol`, `Diagnostic`, `KnowledgeEntry`, `ToolGroup`, `CeqpParams`, `ResponseEnvelope` — with serde roundtrip, Clone, and PartialEq tests.
- **Schema migration** (F07): `stamp_version()` / `check_version()` in `schema_migration.rs` with real SQLite (rusqlite + tempfile), including downgrade guard. Schema version pinned at `1.0.0`.
- **OpenTelemetry skeleton** (F09): `init_tracer()` in `ucil-core/src/otel.rs` with stdout exporter, span creation, and shutdown test — no Jaeger/OTLP wiring (deferred to Phase 6).
- **`ucil init` command** (F03, F04, F05, F06): CLI entry point that creates `.ucil/`, detects project languages, supports `--llm-provider` selection (F04), runs plugin health verification with `PLUGIN_PROBE_TIMEOUT` guard (F05), and writes `.ucil/init_report.json` (F06). `--no-install-plugins` flag ships for the CI smoke path.
- **CI pipeline** (F08): `.github/workflows/ci.yml` with Rust (`cargo test` + `clippy -D warnings`), TypeScript (`biome check` + `pnpm build`), and Python (`ruff` + `mypy --strict` + `pytest`) jobs. Verified structurally via `scripts/verify/P0-W1-F08.sh`; has not yet run on a real GitHub Actions runner.
- **Test fixtures** (F11–F14):
  - `rust-project` (~5.8K LOC) with `#[ignore]`-gated cargo-check integration test (F11)
  - `python-project` (~4.8K LOC expression interpreter) with 4 pytest tests (F12)
  - `typescript-project` (~4.2K LOC task-query engine) with 15 vitest tests (F13)
  - `mixed-project` with intentional Rust/TS/Python lint + type + security + test defects (F14) — oracle for later G7/G8 fusion validation

## What broke

- **`scripts/reality-check.sh` pipefail bug** (WO-0001 verification): `CHANGED_FILES=$(... | grep ...)` pipeline exited non-zero under `set -euo pipefail` when grep found no matches, silently killing the script. Verifier ran manual mutation checks instead; fixed in `c18977b` via union-across-feature-commits. Non-blocking for WO-0001 flip.
- **`reality-check.sh` single-commit rollback** (WO-0002 retry 1): Script used one `LAST_COMMIT` for all files, so files introduced in earlier commits (e.g., `types.rs` at `ea983dd`) were "rolled back" to themselves — zero mutation. Fixed with per-file rollback logic in `7e6bb26`; escalation `20260415-1630-reality-check-per-file-rollback.md` resolved.
- **WO-0003 three-rejection loop** (F03, F11–F14):
  - Retry 1: F12/F13/F14 fixtures absent (executor hadn't built them).
  - Retry 2: same.
  - Retry 3: two `reality-check.sh` bugs surfaced (Bug A: `--ignored` acceptance tests disappear after per-file rollback; Bug B: zero-test false-negative). Fixed in `d3bee43`. Retry 4 passed.
- **WO-0004 missing `tokio::time::timeout`** (retry 1): `verify_plugin_health()` had a bare `.await` on IO. Caught by critic, rejected by verifier (`9f25131`). Fixed in `d2af2f9`.
- **WO-0004 broken nextest selectors** (retry 2): Tests inside `mod tests {}` produced nextest paths with `::tests::` that didn't match frozen `acceptance_tests` selectors; co-located tests also vanished during rollback; `Feature: P0-W1-F06` trailer was missing. Root-cause-finder (`798bed1`, `798bed1` RCA) identified all three. Fixed by moving tests to `crates/ucil-cli/tests/init.rs` with `mod commands { mod init { ... } }` nesting. Retry 3 passed.

## Risks carried into Phase 1

- **`reality-check.sh` `--ignored` blind spot**: DEC-0003 allow-listed one slow test, but the mutation script still can't verify `--ignored` acceptance tests without the manual fixture delete/restore dance. Phase 1 features that depend on slow integration tests need a harness fix, not another manual workaround.
- **Harness maturity**: 9 escalations in a single phase is high. Triage (Buckets A–D), auto-merge, root-cause-finder, and retry loops were all added reactively during Phase 0. Phase 1 is the first phase with the full harness in place.
- **CI not yet exercised on GitHub Actions**: `.github/workflows/ci.yml` passed structural verification only. Phase 1's first push will validate the runner config.
- **No benchmarks, no coverage, no mutation gates**: Phase 0 predated `scripts/mutation-gate.sh` and coverage gates (wired in `5a8422d` for Phase 1+). Phase 0's mutation checks were manual; Phase 1 raises the bar.

## Metrics

- Lines of code added: ~40,991 (dominated by F11–F14 fixtures)
- Tests added: 24 Rust (`#[test]` / `#[tokio::test]`), 15 vitest (TS fixture), 4 pytest (Python fixture) = **43 tests**
- Benches added: 0 (no perf targets in Phase 0)
- Coverage targets: not yet defined (Phase 1+)
- Mutation targets: not yet defined (Phase 1+ per `scripts/mutation-gate.sh` added in `134ab8d`)
- P95 query latency: N/A (no query path built yet)

## Verification reports

- `ucil-build/verification-reports/WO-0001.md` — F01, F10 (PASS, verifier `6c4f652d`)
- `ucil-build/verification-reports/WO-0002.md` — F02, F07, F09 (PASS after 1 rejection, verifier `266e9762`)
- `ucil-build/verification-reports/WO-0003.md` — F03, F11, F12, F13, F14 (PASS after 3 rejections, verifier `11d36c68`)
- `ucil-build/verification-reports/WO-0004.md` — F04, F05, F06, F08 (PASS after 2 rejections, verifier `e632b721`)
- Rejections preserved: `ucil-build/rejections/WO-0002.md`, `WO-0003.md`, `WO-0003-retry-2.md`, `WO-0003-retry-3.md`, `WO-0004.md`

Four distinct verifier session IDs across four work-orders — session independence held.

## Decisions made

- **DEC-0001** — Accept oversized commits in WO-0002 (`types.rs` 368L, `schema_migration.rs` 215L): tightly-coupled domain types require atomic commits.
- **DEC-0002** — Add effectiveness, usability, and end-to-end verification gates: 10 new verification dimensions required from Phase 1 onwards (codified in the harness via `306f424`).
- **DEC-0003** — Allow `#[ignore]` for legitimately-slow tests via a `// SLOW-TEST:` marker comment: exempts `rust_project_loads` (F11) without weakening the anti-laziness pre-commit hook.
- **DEC-0004** — Allow skip markers in `tests/fixtures/mixed-project/`: intentional defect fixtures are exempt from the skip-marker ban — they are the oracle for G7/G8.

## Next phase prep

- **Planner priority**: tree-sitter parsing crate (`ucil-treesitter`) and the LMDB/sled tag cache — Phase 1 Week 1–2 deliverables per master plan §18.
- **Executor / CI**: verify `.github/workflows/ci.yml` runs green on the first real Phase 1 push; fix any host-runner-only breakage before the gate.
- **Harness**: fix `scripts/reality-check.sh` `--ignored`-test detection early (one-line triage-bucket-B fix) so Phase 1 WOs with slow tests don't repeat the manual dance.
- **Gates**: effectiveness-gate scenarios (DEC-0002), mutation-gate floor, and coverage-gate floor all need initial scenario implementations before Phase 1 can ship.
