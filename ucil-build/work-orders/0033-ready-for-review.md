# WO-0033 — ready for review

**Branch**: `feat/WO-0033-find-definition-tool`
**Final commit**: `e3cb5e4a2f80bfabae3c3ef9be4d0a8b9163a479`
**Feature**: `P1-W4-F05` — `find_definition` MCP tool returns definition location, signature, hover doc, and immediate callers from tree-sitter + KG
**Executor**: `executor` subagent (session wrote 8 commits: `45c4066`, `5a001c7`, `bc7420d`, `ec98a07`, `ae8aadb`, `6a3ab37`, `272ffd8`, `e3cb5e4`)

## What I verified locally

- [x] `cargo fmt --all --check` — clean (exit 0)
- [x] `cargo clippy --workspace --all-targets -- -D warnings` — clean (exit 0, all 10 crates)
- [x] `cargo nextest run -p ucil-daemon 'server::test_find_definition_tool' --run-ignored default` — **3/3 PASS** (frozen F05 selector + two negative-path tests, 0.057 s)
- [x] `cargo nextest run -p ucil-daemon server::test_all_22_tools_registered` — **PASS** (22-tool catalog invariant preserved)
- [x] `cargo nextest run -p ucil-daemon server::` — **all PASS** (find_definition + stub-catalog tests + progressive-startup test)
- [x] `cargo nextest run -p ucil-daemon` — **76/76 PASS** (no regression in prior daemon features)
- [x] `cargo nextest run -p ucil-core knowledge_graph::` — **all PASS** (new `get_entity_by_id` + `list_relations_by_target` unit tests green; existing F02/F03/F08 tests unchanged)
- [x] `cargo nextest run --workspace` — **218/218 PASS** (1 skipped in another crate, unrelated)
- [x] `cargo doc -p ucil-daemon --no-deps` — zero `warning` / `error` lines (post-filter for `thiserror`/`Documenting`/`Finished`)
- [x] `cargo doc -p ucil-core --no-deps` — zero `warning` / `error` lines
- [x] `ucil-build/work-orders/0033-ready-for-review.md` — present (this file)
- [x] Zero new stubs: `git diff main..HEAD -- 'crates/**/*.rs' | grep -E 'todo!\(|unimplemented!\(|NotImplementedError|raise NotImplementedError'` yields 0 hits.

## Implementation summary

### `crates/ucil-core/src/knowledge_graph.rs`

- New **read-only** helper `pub fn get_entity_by_id(&self, id: i64) -> Result<Option<Entity>, KnowledgeGraphError>` with `#[tracing::instrument(name = "ucil.core.kg.get_entity_by_id", …)]`. Mirrors `get_entity_by_qualified_name`; no transaction. Returns `Ok(None)` for missing rowids (dangling-FK tolerance).
- New **read-only** helper `pub fn list_relations_by_target(&self, target_id: i64) -> Result<Vec<Relation>, KnowledgeGraphError>` with `#[tracing::instrument(name = "ucil.core.kg.list_relations_by_target", …)]`. Mirrors `list_relations_by_source`; rows returned in `id ASC` order.
- `pub struct SymbolResolution` extended with `pub id: Option<i64>` and `pub qualified_name: Option<String>`. SQL for `resolve_symbol` already SELECTed these columns; the struct now projects them so the server handler can pivot on them. Existing public fields (`file_path`, `start_line`, `signature`, `doc_comment`, `parent_module`) are unchanged — additive change.
- Two module-level `#[cfg(test)]` unit tests: `knowledge_graph::test_get_entity_by_id` (roundtrip: upsert an entity → look up by returned id → confirm fields; missing id → `Ok(None)`) and `knowledge_graph::test_list_relations_by_target` (insert 2 caller relations + 1 outbound → filter by target → exactly 2 inbound rows returned; missing target → empty vec).

### `crates/ucil-daemon/src/server.rs`

- `pub struct McpServer` gained `pub kg: Option<Arc<Mutex<ucil_core::KnowledgeGraph>>>`. The parameterless `McpServer::new()` sets it to `None`, so the frozen `test_all_22_tools_registered` selector + `test_ceqp_params_on_all_tools` + `test_progressive_startup` remain byte-compatible.
- New constructor `pub fn with_knowledge_graph(kg: Arc<Mutex<KnowledgeGraph>>) -> Self`. Used by the new acceptance test + downstream callers (next WO will wire the daemon's real startup path to this).
- `handle_tools_call` now dispatches `find_definition` to `Self::handle_find_definition(id, params, kg)` **only** when `self.kg.is_some()`. When `self.kg.is_none()`, `find_definition` falls through to the existing stub path, so phase-log invariant #9 (the other 21 tools plus default-constructed-server `find_definition`) still holds.
- `handle_find_definition` (associated fn, not `&self`) resolves the symbol, locks the KG mutex once, pivots on the entity id, reads inbound `calls`-kind relations, and projects caller entities to `{qualified_name, file_path, start_line}` JSON. The mutex guard is dropped **before** the JSON envelope is built (`clippy::significant_drop_tightening`). Dangling FKs (source row deleted between queries) are logged at `tracing::warn!` and skipped — the callers list is best-effort because the §12.1 `relations` table has no cascading delete.
- Extracted helpers to stay under the `too_many_lines` cap: `read_find_definition`, `project_callers`, `found_response`, `not_found_response`. New internal struct `FindDefinitionReadError { code: i32, message: String }` threads `(code, message)` out of the locked section so the outer dispatcher builds the JSON-RPC error envelope with the mutex already released.
- Response envelope: `result._meta = { tool: "find_definition", source: "tree-sitter+kg", found: true|false, file_path, start_line, signature, doc_comment, parent_module, qualified_name, callers: [{qualified_name, file_path, start_line}…] }` + `result.content = [{ type: "text", text: "`<name>` defined in <path> at line <n>" }]` + `result.isError = false`. Unknown symbols produce the same envelope with `found: false`, human-readable text, and `isError: false` (not a JSON-RPC error — zero-row results are a successful lookup).

### `crates/ucil-daemon/src/executor.rs`

- Promoted `fn rust_project_fixture()` (under `#[cfg(test)]`) to `pub(crate)` so the new server-module acceptance test can reuse the single cwd-probing fixture locator.

### `crates/ucil-daemon/src/lib.rs`

- Module-doc paragraph describing the WO-0033 promotion of `find_definition`, with cross-refs to master-plan §3.2 row 2, §18 Phase 1 Week 4 line 1751, and phase-log invariant #9.

## Tests (3 new server-module tests at module root per DEC-0005)

1. `server::test_find_definition_tool` (frozen acceptance selector) — opens a temp `KnowledgeGraph`, runs `IngestPipeline::ingest_file` on `tests/fixtures/rust-project/src/util.rs`, upserts a synthetic `calls`-kind relation whose source is a synthetic caller entity and whose target is the real `evaluate` row extracted by tree-sitter, builds `McpServer::with_knowledge_graph(Arc::clone(&kg))`, calls `handle_line` with a `tools/call` request for `find_definition{name:"evaluate"}`, and asserts the JSON-RPC envelope: `jsonrpc == "2.0"`, matching `id`, no `error` field, `_meta.tool == "find_definition"`, `_meta.source == "tree-sitter+kg"`, `_meta.found == true`, `_meta.file_path` matches the ingested fixture path, `_meta.start_line > 0`, `_meta.callers` contains the synthetic caller's `qualified_name`, `result.isError == false`, and — critically — that `_meta.not_yet_implemented` is **ABSENT** (so the dispatcher proved it escaped the phase-1 stub path).

2. `server::test_find_definition_tool_unknown_symbol` — same fixture, but `find_definition{name:"this_symbol_does_not_exist_anywhere"}`. Asserts the response has no `error` field, `_meta.found == false`, and `isError == false` (zero-row result is not a JSON-RPC error).

3. `server::test_find_definition_tool_missing_name_param` — same fixture, but `find_definition{arguments:{}}`. Asserts the response carries an `error` envelope with `code == -32602` (Invalid params), a message mentioning `name`, and no `result` field.

Also added: two module-level unit tests under `ucil-core/src/knowledge_graph.rs` (`test_get_entity_by_id`, `test_list_relations_by_target`) — see above.

## Commit split (8 commits, matches work-order plan step 8)

1. `45c4066` `feat(core): add KnowledgeGraph::get_entity_by_id read helper`
2. `5a001c7` `feat(core): add KnowledgeGraph::list_relations_by_target read helper`
3. `bc7420d` `test(core): unit tests for get_entity_by_id + list_relations_by_target`
4. `ec98a07` `feat(daemon): McpServer::with_knowledge_graph + optional kg field`
5. `ae8aadb` `feat(core): extend SymbolResolution with id + qualified_name` (additive foundation for step 5)
6. `6a3ab37` `feat(daemon): handle_find_definition with KG-backed dispatch` (combined plan steps 5+6: happy path + not-found + missing-name + dispatch wiring in one coherent diff)
7. `272ffd8` `test(daemon): find_definition acceptance + negative tests`
8. `e3cb5e4` `docs(daemon): lib.rs paragraph for find_definition promotion`

## No forbidden-path touched

Verified: `ucil-build/feature-list.json`, `ucil-build/feature-list.schema.json`, `ucil-master-plan-v2.1-final.md`, `tests/fixtures/**`, `scripts/gate/**`, `scripts/flip-feature.sh`, `.githooks/**`, `ucil-build/decisions/DEC-{0001..0008}-*.md` — all untouched by this WO's diff.

## Test runtime (sample)

- `server::test_find_definition_tool` — 0.059 s (real tree-sitter parse of `util.rs` + KG open + relation upsert + handler call)
- `server::test_find_definition_tool_unknown_symbol` — 0.058 s
- `server::test_find_definition_tool_missing_name_param` — 0.059 s
- Full `ucil-daemon` suite — 0.726 s wall
- Full `--workspace` suite — 5.064 s wall (218/218 PASS, 1 skipped in an unrelated crate)

## Master-plan cross-refs

- §3.2 row 2 — `find_definition` — "Go-to-definition with full context"
- §18 Phase 1 Week 4 line 1751 — "Implement first working tool: find_definition"
- §12.1 — `entities` + `relations` schemas (columns the handler projects)
- §12.2 — bi-temporal reads (`resolve_symbol` already threads through the WO-0024 RCA's lexicographic-RFC3339 invariant; this WO adds no new temporal logic)
- DEC-0005 — tests live at module root, no `mod tests { … }` wrapper
- DEC-0007 — `find_definition` is a new handler, not a new module; reality-check.sh automated path applies
- DEC-0008 — Serena enrichment of hover doc deferred to P1-W5-F02; this WO intentionally does not wire Serena
- Phase-log invariant #9 — preserved (other 21 tools keep `_meta.not_yet_implemented: true` stubs; `find_definition` keeps the stub path when `McpServer::new()` is used)
