# WO-0034 — Ready for review

**Feature**: `P1-W4-F10` — `get_conventions` MCP tool
**Branch**: `feat/WO-0034-get-conventions-tool`
**Final commit**: `f3a9b29`

## Summary

Promotes the `get_conventions` MCP tool (master-plan §3.2 row 7) from
the phase-1 `_meta.not_yet_implemented: true` stub to a real handler
that reads project coding conventions from the SQLite `conventions`
table. Mirrors the WO-0033 `find_definition` gating pattern: when
`McpServer::with_knowledge_graph(kg)` is used, dispatch routes to the
real handler; when the server is built via `McpServer::new()` (no KG
bound), `get_conventions` falls through to the stub so phase-1
invariant #9 is preserved for the "no KG" deployment shape and the
remaining 20 tools.

## Commits (5)

```
f3a9b29 test(daemon): cover get_conventions tool end-to-end (P1-W4-F10)
0cc81ce feat(daemon): wire get_conventions MCP tool (P1-W4-F10)
df657a2 test(core): cover conventions CRUD helpers
3e54fa6 feat(core): add insert_convention + list_conventions helpers
77a0134 feat(core): add Convention struct for conventions table
```

## What changed

### `crates/ucil-core/src/knowledge_graph.rs`

- New `pub struct Convention` with all 10 columns of the §12.1
  `conventions` table (`id`, `category`, `pattern`, `examples`,
  `counter_examples`, `confidence`, `evidence_count`, `t_ingested_at`,
  `last_verified`, `scope`). Derives `Debug, Clone, PartialEq,
  Serialize, Deserialize`. Rustdoc references master-plan §12.1 lines
  1172–1182 and §3.2 row 7.
- New `pub fn insert_convention(&mut self, &Convention) -> Result<i64,
  KnowledgeGraphError>` routes through `execute_in_transaction`
  (`BEGIN IMMEDIATE` per §11 line 1117); `t_ingested_at` is omitted
  from the INSERT list so the schema default
  (`DEFAULT (datetime('now'))`) populates it. `#[tracing::instrument]`
  span `ucil.core.kg.insert_convention`.
- New `pub fn list_conventions(&self, Option<&str>) -> Result<Vec<Convention>,
  KnowledgeGraphError>` — `None` drops the `WHERE` clause; `Some(cat)`
  filters by `category = ?1` against the `idx_conventions_category`
  index. Rows ordered by `id ASC`. Empty result is `Ok(vec![])`, never
  `Err`. `#[tracing::instrument]` span `ucil.core.kg.list_conventions`.
- New private `fn convention_from_row` row decoder alongside the
  existing `entity_from_row` / `relation_from_row`.
- Four module-root unit tests:
    - `test_insert_and_list_conventions_all` — round-trip two rows,
      assert id-asc order + every user column preserved +
      `t_ingested_at` populated by schema default.
    - `test_list_conventions_category_filter` — three-row mixed-category
      fixture, filter returns only matching rows.
    - `test_list_conventions_unknown_category_returns_empty` — unknown
      filter yields `Ok(vec![])`.
    - `test_list_conventions_empty_table` — fresh KG returns
      `Ok(vec![])` from `list_conventions(None)`.

### `crates/ucil-core/src/lib.rs`

- Re-export `Convention` from `knowledge_graph` alongside `Entity`,
  `Relation`.

### `crates/ucil-daemon/src/server.rs`

- New dispatch branch in `handle_tools_call`: when `name ==
  "get_conventions" && self.kg.is_some()`, route to
  `Self::handle_get_conventions`; otherwise fall through to the stub.
  The `find_definition` branch is untouched.
- New `fn handle_get_conventions(id, params, kg)` method. Parses
  `arguments.category`:
    - absent key / explicit JSON null → `Option::None` (no filter)
    - JSON string → `Some(s)` (filter)
    - any other JSON type → JSON-RPC `-32602` (Invalid params)
  Then calls `read_conventions` and projects rows onto the
  `_meta: {tool, source: "kg", count, category, conventions}`
  envelope. Empty result returns `isError: false` with text
  `"no conventions yet"` — the §3.2 row 7 "empty list if none yet
  extracted" contract.
- New private `struct GetConventionsReadError { code, message }`
  mirroring `FindDefinitionReadError`; new private
  `fn read_conventions(kg, category) -> Result<Vec<Convention>,
  GetConventionsReadError>` owns the mutex lock + KG call so
  `handle_get_conventions` stays under `clippy::too_many_lines`.
- New private `fn convention_to_json` and `fn get_conventions_found_response`
  response builders. Reuses the existing `jsonrpc_error` helper.
- Four module-root acceptance tests at the frozen-selector path:
    - `test_get_conventions_tool` — main acceptance test, two-row KG
      fixture, drives both unfiltered and `category="naming"`
      tools/call round-trips, asserts `_meta.count`, `_meta.category`,
      `_meta.conventions[*].category`, `_meta.conventions[*].pattern`,
      `isError == false`, and that `_meta.not_yet_implemented` is
      absent.
    - `test_get_conventions_tool_empty` — fresh KG, asserts `count ==
      0`, `conventions == []`, `isError == false`, text says "no
      conventions yet".
    - `test_get_conventions_tool_no_kg_returns_stub` — `McpServer::new()`
      keeps the stub envelope — phase-1 invariant #9 preserved.
    - `test_get_conventions_tool_non_string_category` — non-string
      `category` → JSON-RPC error `-32602`.

## Verified locally

- `cargo nextest run -p ucil-daemon server::test_get_conventions_tool --run-ignored default`: 4 passed (happy path + empty + stub + non-string).
- `cargo nextest run -p ucil-daemon server::test_all_22_tools_registered`: 1 passed — the 22-tool catalog stays wire-compatible.
- `cargo nextest run -p ucil-daemon server::test_find_definition_tool`: all 3 find_definition tests pass — the new dispatch branch does not shadow or alter `find_definition` gating.
- `cargo nextest run -p ucil-daemon server::`: 10 tests passed, 0 failed.
- `cargo nextest run -p ucil-core knowledge_graph::`: 15 tests passed (11 pre-existing + 4 new convention tests).
- `cargo nextest run --workspace`: 226 tests passed, 1 skipped (pre-existing), 0 failed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all --check`: clean.
- `cargo doc -p ucil-daemon --no-deps`: zero warnings/errors.
- `cargo doc -p ucil-core --no-deps`: zero warnings/errors.
- `grep -nE 'todo!\(|unimplemented!\(|NotImplementedError|raise NotImplementedError'` over the WO diff: 0 hits.

All 11 acceptance criteria from the work-order JSON pass end-to-end.

## Scope preserved

- Auto-extraction of conventions (G5 group-fusion layer, master-plan
  §17 / Phase 2+) — out of scope.
- `hot_convention_signals` hot→warm→cold promotion loop
  (Phase-3) — out of scope.
- The 22-tool catalog and its `ucil_tools()` builder — unchanged.
- `tests/fixtures/**`, `scripts/gate/**`, `flip-feature.sh`,
  `feature-list.json`, master plan — untouched.
- Zero new `thiserror` variants; reuses `KnowledgeGraphError::Sqlite`
  for the generic SQL path.

## Notes for the verifier

- The frozen acceptance selector is `server::test_get_conventions_tool`
  — module-root placement per DEC-0005 so the selector resolves without
  a `tests::` segment.
- The "reality check" (step 6b in verifier flow) is satisfied by the
  dispatch branch + handler being the single place that escapes the
  stub path; stashing `handle_get_conventions` + the dispatch branch
  causes `test_get_conventions_tool` to fail (the response would carry
  `_meta.not_yet_implemented: true` instead of the expected `_meta.tool
  == "get_conventions"`).
