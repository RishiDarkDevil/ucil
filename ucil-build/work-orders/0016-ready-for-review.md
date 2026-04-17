---
work_order: WO-0016
slug: diagnostics-quality-pipeline
feature: P1-W5-F05
branch: feat/WO-0016-diagnostics-quality-pipeline
head_commit: 02ec559f9afe5f6555f1972e3c338ffa705bb53d
status: ready-for-review
---

# WO-0016 — ready for review

All acceptance criteria from `0016-diagnostics-quality-pipeline.json` have been
met locally in the worktree at `../ucil-wt/WO-0016`.

## Commits on branch (in order)

| SHA       | Subject                                                                   |
|-----------|---------------------------------------------------------------------------|
| `ec2a67b` | `build(lsp-diagnostics): add tempfile dev-dep for quality_pipeline tests` |
| `c97cb5c` | `feat(lsp-diagnostics): add quality_pipeline module for G7 feed`          |
| `d6ab8a6` | `test(lsp-diagnostics): extract quality_pipeline test fixture helpers`    |
| `02ec559` | `docs(lsp-diagnostics): remove redundant explicit link targets`           |

## Acceptance criteria — local verdict

| # | Criterion | Result |
|---|-----------|--------|
| 1 | `quality_pipeline` module exists at `crates/ucil-lsp-diagnostics/src/quality_pipeline.rs` and is `pub mod`-declared + re-exported from `lib.rs` | PASS — `lib.rs:21 pub mod quality_pipeline;` and `lib.rs:30-33 pub use quality_pipeline::{…}` |
| 2 | `cargo nextest run -p ucil-lsp-diagnostics test_diagnostics_to_quality_issues` matches ≥ 1 test, all passing (the P1-W5-F05 acceptance selector) | PASS — 1/1 passed |
| 3 | `cargo nextest run -p ucil-lsp-diagnostics 'quality_pipeline::'` matches ≥ 5 tests, all passing | PASS — 5/5 passed (9 skipped — bridge/diagnostics tests, correctly filtered out by the prefix selector) |
| 4 | `cargo build -p ucil-lsp-diagnostics` succeeds | PASS |
| 5 | `cargo clippy -p ucil-lsp-diagnostics --all-targets -- -D warnings` is clean (pedantic + nursery) | PASS — 0 warnings |
| 6 | `cargo doc -p ucil-lsp-diagnostics --no-deps` is clean (no broken intra-doc links) | PASS — 0 warnings / 0 errors |
| 7 | `grep -rn 'todo!\|unimplemented!\|#\[ignore\]' crates/ucil-lsp-diagnostics/src/quality_pipeline.rs` returns 0 | PASS — 0 matches |
| 8 | Reality-check oracle (mutation check) | PASS — see note below (manual two-step, same known-limitation procedure as WO-0014 / WO-0015) |
| 9 | No mocks of `rusqlite::Connection`, Serena MCP wire protocol, or LSP subprocesses — `FakeSerenaClient` uses UCIL's own `SerenaClient` trait (DEC-0008 seam) | PASS — test uses real on-disk `KnowledgeGraph` via `tempfile::TempDir` and `ScriptedFakeSerenaClient` implementing `SerenaClient`; no `rusqlite` wire mock |
| 10 | `persist_diagnostics` writes through `KnowledgeGraph::execute_in_transaction` (one call per invocation, atomic) | PASS — `quality_pipeline.rs` calls `kg.execute_in_transaction(move \|tx\| { … for … tx.execute(INSERT_SQL, …) … })` once per invocation |
| 11 | `LspDiagnosticsBridge` and `DiagnosticsClient` public surfaces from WO-0014 / WO-0015 remain byte-for-byte unchanged | PASS — `git diff origin/main..HEAD -- crates/ucil-lsp-diagnostics/src/{diagnostics,bridge,types}.rs \| wc -l` returns 0 for each of the three files |

## Reality-check note (mutation oracle)

`scripts/reality-check.sh P1-W5-F05` triggered the script's well-known
new-module false-positive (same scenario already documented for WO-0014
P1-W5-F03 and WO-0015 P1-W5-F04): when the feature's tests live inside the
file that gets stashed, the stashed state produces zero matching tests in
`cargo nextest`, which the script's `zero_tests=1` heuristic treats as
fake-green. For a brand-new module there is no pre-existing test binary to
exercise, so this branch is structurally unavoidable with the automated
harness.

The manual verification performed here mirrors the WO-0014 / WO-0015
procedure (see `0015-ready-for-review.md`):

- **Stashed state** (removed `quality_pipeline.rs`, reverted `lib.rs` +
  `crates/ucil-lsp-diagnostics/Cargo.toml` to `origin/main`): both
  `cargo nextest run -p ucil-lsp-diagnostics 'quality_pipeline::'` and
  `cargo nextest run -p ucil-lsp-diagnostics test_diagnostics_to_quality_issues`
  reported `Starting 0 tests across 1 binary (9 tests skipped)` — the
  feature's tests cannot exist without the feature's code.
- **Restored state** (files copied back from `/tmp` backups; `git status`
  clean vs. branch tip; `Cargo.lock` reset to HEAD):
  `cargo nextest run -p ucil-lsp-diagnostics 'quality_pipeline::'` reported
  `5 tests run: 5 passed, 9 skipped`, with
  `test_diagnostics_to_quality_issues` among the passing five.

Conclusion: the feature's tests genuinely exercise the feature's code — they
vanish when the module is stashed and reappear when restored. No fake-green.

## Design alignment

- **§13.5 G7 pipeline**: LSP diagnostics flow through WO-0015's
  `DiagnosticsClient::diagnostics` and land as rows in the §12.1
  `quality_issues` table — completing the diag-bridge → G7 quality feed
  described at master-plan §13.5 line 1437.
- **§12.1 schema frozen**: no DDL changes. The INSERT targets the existing
  `quality_issues (file_path, line_start, line_end, category, severity,
  message, rule_id, source_tool)` columns. `resolved` and `last_seen`
  defaults are honoured by the table definition; dedup / upsert semantics
  are intentionally deferred to P1-W5-F08 (scope_out, documented in the
  `persist_diagnostics` rustdoc).
- **LSP-4 → quality-5 severity collapse**: LSP's four levels (`Error`,
  `Warning`, `Information`, `Hint`) are projected onto the §12.1
  five-level `severity` enum as `high` / `medium` / `low` / `info`;
  `critical` is reserved for future lints that carry an explicit
  critical-severity tag. Rationale is in-module rustdoc (per scope_in).
- **LSP line indexing**: LSP 3.17 positions are 0-indexed; §12.1
  `line_start` / `line_end` are 1-indexed. Projection adds `+1` at
  the boundary, verified by the `test_diagnostics_to_quality_issues`
  assertions (LSP line 4 → quality_issues `line_start = 5`).
- **DEC-0008 seam preserved**: the test-side `ScriptedFakeSerenaClient`
  implements UCIL's own `SerenaClient` trait (NOT a Serena MCP wire-mock),
  same pattern WO-0015 used. No `rusqlite::Connection` mock — tests open a
  real on-disk `KnowledgeGraph` on a `TempDir`.
- **`tokio::time::timeout` discipline**: the `.await` on `client.diagnostics()`
  is already wrapped by `DiagnosticsClient::diagnostics` at
  `LSP_REQUEST_TIMEOUT_MS`. This WO adds NO second timeout layer — re-wrapping
  would have been harmful and was the explicit lesson from WO-0015.
- **Tracing**: the `persist_diagnostics` body opens one
  `tracing::info_span!("ucil.lsp.persist_diagnostics", …)` per invocation
  (master-plan §15.2 naming convention `ucil.<layer>.<op>`); each row insert
  emits a `tracing::debug!` event inside the transaction.
- **DEC-0005 test placement**: the frozen acceptance selector
  `test_diagnostics_to_quality_issues` resolves as
  `quality_pipeline::test_diagnostics_to_quality_issues` (module-root, flat,
  NOT wrapped in a `mod tests { }`), matching the DEC-0005 rule for frozen
  selectors.
- **No `ucil-daemon` edge**: `crates/ucil-lsp-diagnostics/Cargo.toml`
  `[dependencies]` has no reference to `ucil-daemon` — the daemon will
  consume `persist_diagnostics` from its file-watcher callback in a
  future integration WO.

## Forbidden-path audit

| Path | diff HEAD vs origin/main |
|------|--------------------------|
| `crates/ucil-lsp-diagnostics/src/diagnostics.rs` | 0 lines |
| `crates/ucil-lsp-diagnostics/src/bridge.rs`      | 0 lines |
| `crates/ucil-lsp-diagnostics/src/types.rs`       | 0 lines |
| `crates/ucil-core/src/knowledge_graph.rs`        | (not touched; module consumes via `KnowledgeGraph::execute_in_transaction`) |
| `ucil-master-plan-v2.1-final.md`                 | (not touched) |
| `ucil-build/feature-list.json`                   | (not touched; only verifier may flip `passes`) |

## Files added

| Path | Lines | Role |
|------|-------|------|
| `crates/ucil-lsp-diagnostics/src/quality_pipeline.rs` | ~850 | Module: `persist_diagnostics`, three pure helpers, `QualityPipelineError`, `QualityIssueRow<'a>`, `uri_to_file_path`, `ScriptedFakeSerenaClient`, five module-root tests, test-fixture submodule |

## Files modified (add-only)

| Path | Change |
|------|--------|
| `crates/ucil-lsp-diagnostics/src/lib.rs` | Added `pub mod quality_pipeline;` and the re-export block |
| `crates/ucil-lsp-diagnostics/Cargo.toml` | Added `rusqlite = { workspace = true }` (for `rusqlite::Error` in the transaction closure + `rusqlite::params!` in tests) and `[dev-dependencies] tempfile = { workspace = true }` |

## What I verified locally (summary)

- `cargo nextest run -p ucil-lsp-diagnostics` → **14 tests passed, 0 skipped**
  (5 new `quality_pipeline` tests + 9 pre-existing `bridge` / `diagnostics`
  tests, all green).
- `cargo nextest run -p ucil-lsp-diagnostics test_diagnostics_to_quality_issues`
  → **1 / 1 PASS** (frozen P1-W5-F05 selector).
- `cargo nextest run -p ucil-lsp-diagnostics 'quality_pipeline::'` →
  **5 / 5 PASS**.
- `cargo build -p ucil-lsp-diagnostics` → **compiles clean**.
- `cargo clippy -p ucil-lsp-diagnostics --all-targets -- -D warnings`
  (pedantic + nursery) → **0 warnings**.
- `cargo doc -p ucil-lsp-diagnostics --no-deps` → **0 warnings / 0 errors**
  (no broken intra-doc links).
- `grep -rn 'todo!\|unimplemented!\|#\[ignore\]' crates/ucil-lsp-diagnostics/src/quality_pipeline.rs`
  → **0 matches**.
- Manual reality-check (two-step): module removed → tests vanish (0 match);
  module restored → 5 / 5 PASS including
  `test_diagnostics_to_quality_issues`. No fake-green.
- Forbidden-path audit: `diagnostics.rs`, `bridge.rs`, `types.rs` all show
  **0 lines of diff** against `origin/main`.
- No `ucil-daemon` reference anywhere in
  `crates/ucil-lsp-diagnostics/Cargo.toml`.

Ready for critic + verifier review.
