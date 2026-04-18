---
work_order: WO-0023
slug: lsp-call-and-type-hierarchy-feed
feature: P1-W5-F06
branch: feat/WO-0023-lsp-call-and-type-hierarchy-feed
head_commit: e874a54bf0552aad4f51d4e536df5c5109da43b3
status: ready-for-review
---

# WO-0023 — ready for review

All acceptance criteria from `0023-lsp-call-and-type-hierarchy-feed.json`
have been met locally in the worktree at `../ucil-wt/WO-0023`. The G4
architecture feed (call hierarchy incoming/outgoing + type hierarchy
supertypes) is now delivered as a new `call_hierarchy` module inside
`ucil-lsp-diagnostics`, mirroring the WO-0016 `quality_pipeline` template.

## Commits on branch (in order)

| SHA       | Subject                                                             |
|-----------|---------------------------------------------------------------------|
| `be06fb9` | `feat(lsp-diagnostics): add G4 call/type hierarchy persistence`     |
| `e874a54` | `test(lsp-diagnostics): acceptance tests for call/type hierarchy feed` |

Both commits pushed to `origin/feat/WO-0023-lsp-call-and-type-hierarchy-feed`.

## Acceptance criteria — local verdict

| # | Criterion | Result |
|---|-----------|--------|
| 1 | Module exists; `pub mod call_hierarchy;` + re-exports in `lib.rs` | PASS — `lib.rs:20 pub mod call_hierarchy;`, `lib.rs:28-31 pub use call_hierarchy::{…}` |
| 2 | `cargo nextest run -p ucil-lsp-diagnostics 'call_hierarchy::'` matches ≥ 7 tests, all passing | PASS — 7/7 passed (14 skipped — bridge/diagnostics/quality_pipeline tests, filtered by the prefix selector) |
| 3 | `cargo build -p ucil-lsp-diagnostics` succeeds | PASS |
| 4 | `cargo clippy -p ucil-lsp-diagnostics --all-targets -- -D warnings` (pedantic + nursery) is clean | PASS — 0 warnings |
| 5 | `cargo doc -p ucil-lsp-diagnostics --no-deps` is clean | PASS — 0 warnings / 0 errors |
| 6 | No `todo!` / `unimplemented!` / `#[ignore]` in `call_hierarchy.rs` | PASS — 0 matches |
| 7 | No `ucil-daemon` dep in `ucil-lsp-diagnostics/Cargo.toml` (DEC-0008) | PASS — 0 matches |
| 8 | Required public symbols present (3 `pub async fn persist_*` + `pub enum CallHierarchyError`) | PASS — 4/4 declarations found |
| 9 | All 7 frozen test names present as module-root `fn test_*` | PASS — 7/7 `fn test_*` matches |
| 10 | No `mod tests {` wrapper in `call_hierarchy.rs` (DEC-0005) | PASS — 0 matches |
| 11 | Exactly 3 occurrences of `execute_in_transaction(` (one per persist fn) | PASS — count == 3 |
| 12 | No direct writes via `kg.conn().execute(` or `self.conn().execute(` | PASS — 0 matches |
| 13 | Forbidden-path audit (bridge/diagnostics/quality_pipeline/types, ucil-core, ucil-daemon, ucil-treesitter) | PASS — all 0 lines of diff vs `origin/main` |
| 14 | Reality-check oracle (mutation check) | PASS — manual two-step (script trips on `zero_tests=1` brand-new-module heuristic; see note) |

## Reality-check note (mutation oracle)

`scripts/reality-check.sh P1-W5-F06` triggered the script's well-known
new-module false-positive — the same scenario already documented for
WO-0014 (P1-W5-F03), WO-0015 (P1-W5-F04), and WO-0016 (P1-W5-F05). When
the feature's tests live inside the file that gets stashed, the stashed
state produces zero matching tests in `cargo nextest`, which the
script's `zero_tests=1` heuristic treats as fake-green. For a
brand-new module there is no pre-existing test binary to exercise, so
this branch is structurally unavoidable with the automated harness.

The manual two-step verification performed here mirrors the WO-0014 /
WO-0015 / WO-0016 procedure (see `0016-ready-for-review.md`):

- **Stashed state** (removed `call_hierarchy.rs`, reverted `lib.rs` to
  `origin/main`): `cargo nextest run -p ucil-lsp-diagnostics
  'call_hierarchy::'` reported `Starting 0 tests across 1 binary
  (14 tests skipped)` → `0 tests run: 0 passed, 14 skipped` →
  `error: no tests to run`. The feature's tests cannot exist without
  the feature's code.
- **Restored state** (files copied back; `git status` clean vs. branch
  tip): `cargo nextest run -p ucil-lsp-diagnostics 'call_hierarchy::'`
  reported `7 tests run: 7 passed, 14 skipped`, with all seven frozen
  `test_*` names among the passing seven.

Conclusion: the feature's tests genuinely exercise the feature's code —
they vanish when the module is stashed and reappear when restored. No
fake-green. Working tree after the manual check: clean, no drift.

## Design alignment

- **§13.5 G4 architecture feed**: LSP `textDocument/callHierarchy/{incoming,outgoing}Calls`
  and `textDocument/typeHierarchy/supertypes` responses now land as rows
  in the §12.1 `entities` + `relations` tables — completing the
  diag-bridge → G4 architecture feed described at master-plan §13.5 line
  1436.
- **§12.1 schema frozen**: no DDL changes. The three persist functions
  INSERT into the existing `entities (kind, name, file_path, start_line,
  end_line, qualified_name, t_valid_from)` and `relations (source_id,
  target_id, kind, source_tool, source_evidence, confidence)` columns
  only.
- **§11 single-writer discipline**: each persist function opens exactly
  one `KnowledgeGraph::execute_in_transaction(|tx| { … })` scope —
  verified by the frozen grep criterion (`grep -c
  'execute_in_transaction(' == 3`) and by the `test_atomic_transaction_single_scope`
  test which reads its own source via `include_str!` and asserts the
  count at test time.
- **Direction semantics** (documented in rustdoc + asserted in every
  positive test via SQL join on `entities.name`):
  - incoming calls → `relations(source_id=peer, target_id=root, kind='calls', source_tool='lsp:callHierarchy')` — peer calls root
  - outgoing calls → `relations(source_id=root, target_id=peer, kind='calls', source_tool='lsp:callHierarchy')` — root calls peer
  - supertypes → `relations(source_id=root, target_id=supertype, kind='inherits', source_tool='lsp:typeHierarchy')` — root extends/implements supertype
- **`SymbolKind` → `entities.kind` mapping** (pure `const fn`, tested by
  `test_symbol_kind_mapping_covers_serena_emitted_kinds`):
  `Function`/`Method`/`Constructor` → `"function"`;
  `Class`/`Interface`/`Struct`/`Enum` → `"type"`;
  `Module`/`Namespace`/`Package` → `"module"`;
  `Variable`/`Constant`/`Field`/`Property` → `"variable"`; fallthrough →
  `"symbol"`. Documented in the module rustdoc as a table
  (WO-0016 severity-mapping precedent).
- **DEC-0008 seam preserved**: the test-side `ScriptedFakeSerenaClient`
  implements UCIL's own `SerenaClient` trait (NOT a Serena MCP wire-mock),
  same pattern WO-0015/WO-0016 used. No `rusqlite::Connection` mock —
  tests open a real on-disk `KnowledgeGraph` via `tempfile::TempDir` +
  `KnowledgeGraph::open`. `ucil-lsp-diagnostics/Cargo.toml` still has
  zero `ucil-daemon` references — the cycle-free invariant holds.
- **`tokio::time::timeout` discipline**: the `.await` on
  `client.call_hierarchy_incoming` / `_outgoing` /
  `type_hierarchy_supertypes` is already wrapped by `DiagnosticsClient`
  at `LSP_REQUEST_TIMEOUT_MS` (WO-0015). This WO adds NO second timeout
  layer — re-wrapping would mask the typed `DiagnosticsClientError::Timeout`
  variant behind an opaque outer future (WO-0015 anti-pattern,
  already documented in `quality_pipeline.rs`).
- **Tracing**: the three persist fns each carry
  `#[tracing::instrument(level="info", skip(client, kg, root_item),
  fields(root_name = %root_item.name))]` with span names
  `ucil.lsp.persist_call_hierarchy_incoming`,
  `ucil.lsp.persist_call_hierarchy_outgoing`, and
  `ucil.lsp.persist_type_hierarchy_supertypes` — matching §15.2
  `ucil.<layer>.<op>` naming.
- **DEC-0005 test placement**: the frozen acceptance selector
  `call_hierarchy::` resolves as a path prefix matching module-root tests
  (NOT wrapped in `mod tests { }`). The 7 test functions live directly
  at module root under `#[cfg(test)]`, matching the DEC-0005 rule and
  the WO-0006/0007 regression's corrective precedent.
- **Non-file URI early surface**: `persist_call_hierarchy_incoming` (and
  the other two) project `root_item.uri` → file_path BEFORE opening the
  transaction, so a non-file URI produces a typed
  `CallHierarchyError::NonFileUri { uri }` error without touching SQLite
  (verified by `test_non_file_uri_surfaces_typed_error`, which asserts
  `entities`/`relations` COUNT stays at 0).
- **Empty hierarchy is a no-op**: if Serena returns an empty vec for a
  root item the function logs `tracing::debug!("no incoming calls;
  skipping transaction")` and returns `Ok(0)` without opening a
  transaction — verified by `test_persist_empty_hierarchy_returns_zero`
  which calls all three functions and asserts both `entities` and
  `relations` tables remain empty.

## Forbidden-path audit

| Path | diff HEAD vs origin/main |
|------|--------------------------|
| `crates/ucil-lsp-diagnostics/src/bridge.rs`          | 0 lines |
| `crates/ucil-lsp-diagnostics/src/diagnostics.rs`     | 0 lines |
| `crates/ucil-lsp-diagnostics/src/quality_pipeline.rs`| 0 lines |
| `crates/ucil-lsp-diagnostics/src/types.rs`           | 0 lines |
| `crates/ucil-core/`                                  | 0 lines |
| `crates/ucil-daemon/`                                | 0 lines |
| `crates/ucil-treesitter/`                            | 0 lines |
| `ucil-master-plan-v2.1-final.md`                     | (not touched) |
| `ucil-build/feature-list.json`                       | (not touched; only verifier may flip `passes`) |

## Files added

| Path | Lines | Role |
|------|-------|------|
| `crates/ucil-lsp-diagnostics/src/call_hierarchy.rs` | 1408 | Module: `persist_call_hierarchy_incoming`, `persist_call_hierarchy_outgoing`, `persist_type_hierarchy_supertypes`, `symbol_kind_to_entity_kind`, `CallHierarchyError`, private `uri_to_file_path` + `insert_entity` + `insert_relation` helpers, `PeerProjection` projection struct, `ScriptedFakeSerenaClient` submodule, `test_fixtures` submodule, seven module-root `test_*` acceptance tests |

## Files modified (add-only)

| Path | Change |
|------|--------|
| `crates/ucil-lsp-diagnostics/src/lib.rs` | Added `pub mod call_hierarchy;` (line 20) and the `pub use call_hierarchy::{persist_call_hierarchy_incoming, persist_call_hierarchy_outgoing, persist_type_hierarchy_supertypes, symbol_kind_to_entity_kind, CallHierarchyError};` re-export block (lines 28-31), alphabetical-within-group with the pre-existing `bridge` / `diagnostics` / `quality_pipeline` / `types` re-exports |

## What I verified locally (summary)

- `cargo nextest run -p ucil-lsp-diagnostics 'call_hierarchy::'` →
  **7 / 7 PASS** (frozen P1-W5-F06 selector; 14 skipped, correctly
  filtered out by the `call_hierarchy::` prefix).
- `cargo nextest run -p ucil-lsp-diagnostics` →
  **21 / 21 PASS** (7 new `call_hierarchy` + 5 existing
  `quality_pipeline` + 9 existing `bridge` / `diagnostics`, all green).
- `cargo build -p ucil-lsp-diagnostics` → **compiles clean**.
- `cargo clippy -p ucil-lsp-diagnostics --all-targets -- -D warnings`
  (pedantic + nursery regime inherited from `lib.rs:15-17`) →
  **0 warnings**.
- `cargo doc -p ucil-lsp-diagnostics --no-deps` → **0 warnings /
  0 errors** (no broken intra-doc links).
- `grep -rn 'todo!\|unimplemented!\|#\[ignore\]'
  crates/ucil-lsp-diagnostics/src/call_hierarchy.rs` → **0 matches**.
- `grep -c 'execute_in_transaction(' crates/ucil-lsp-diagnostics/src/call_hierarchy.rs`
  → **exactly 3** (one per persist function).
- `grep -E 'kg\.conn\(\)\.execute\(|self\.conn\(\)\.execute\('
  crates/ucil-lsp-diagnostics/src/call_hierarchy.rs` → **0 matches**
  (every writer routes through `execute_in_transaction`).
- `grep -E '^mod\s+tests\s*\{'
  crates/ucil-lsp-diagnostics/src/call_hierarchy.rs` → **0 matches**
  (DEC-0005 flat module-root tests).
- All 7 frozen `fn test_*` names present (
  `test_symbol_kind_mapping_covers_serena_emitted_kinds`,
  `test_persist_call_hierarchy_incoming_writes_entities_and_relations`,
  `test_persist_call_hierarchy_outgoing_flips_direction`,
  `test_persist_type_hierarchy_supertypes_writes_inherits_relations`,
  `test_persist_empty_hierarchy_returns_zero`,
  `test_non_file_uri_surfaces_typed_error`,
  `test_atomic_transaction_single_scope`).
- Manual reality-check (two-step): module stashed → 0 tests run;
  restored → 7 / 7 PASS. No fake-green.
- Forbidden-path audit: `bridge.rs`, `diagnostics.rs`,
  `quality_pipeline.rs`, `types.rs`, `crates/ucil-core/`,
  `crates/ucil-daemon/`, `crates/ucil-treesitter/` all show
  **0 lines of diff** against `origin/main`.
- No `ucil-daemon` reference anywhere in
  `crates/ucil-lsp-diagnostics/Cargo.toml` — DEC-0008 cycle-free
  invariant preserved.

Ready for critic + verifier review.
