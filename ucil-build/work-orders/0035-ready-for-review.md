# WO-0035 — ready for review

**Feature**: `P1-W5-F09` — `search_code` MCP tool (hybrid symbol + text).
**Branch**: `feat/WO-0035-search-code-tool`
**Head commit**: `f9199cb`
**ADR**: `DEC-0009-search-code-in-process-ripgrep` (pre-authored by planner).
**Master-plan anchors**: §3.2 row 4; §18 Phase 1 Week 5 line 1765; §12.1
  (entities schema consumed by the new KG read helper).
**Frozen acceptance selector**:
  `cargo nextest run -p ucil-daemon server::test_search_code_basic`.

## Diff summary (from branch point `cf55900..f9199cb`)

```
 Cargo.lock                              |  108 +++
 Cargo.toml                              |   21 +
 crates/ucil-core/src/knowledge_graph.rs |  254 +++
 crates/ucil-daemon/Cargo.toml           |    7 +
 crates/ucil-daemon/src/lib.rs           |    1 +
 crates/ucil-daemon/src/server.rs        | 1170 +++
 crates/ucil-daemon/src/text_search.rs   |  370 +++
 7 files changed, 1919 insertions(+), 12 deletions(-)
```

No files under `tests/fixtures/**`, `scripts/gate/**`,
`scripts/flip-feature.sh`, `ucil-build/feature-list.json`,
`ucil-build/feature-list.schema.json`, or the master plan were touched.

## Commits (8, all pushed)

| SHA | Subject |
|-----|---------|
| `166cd01` | build(workspace): add ignore + grep-* deps for search_code text scan |
| `1e4a466` | feat(core): add KnowledgeGraph::search_entities_by_name read helper |
| `e84984b` | test(core): cover search_entities_by_name read helper |
| `1aef49a` | feat(daemon): wire search_code MCP tool (P1-W5-F09, DEC-0009) |
| `6dd1b1a` | test(daemon): cover merge_search_results pure function |
| `c687694` | refactor(daemon): align SearchCodeResult with WO-0035 preview/signature spec |
| `5e17349` | test(daemon): add search_code acceptance + 5 negative tests (WO-0035) |
| `f9199cb` | docs(daemon): backtick SQLite / scope_in in search_code rustdoc |

## Acceptance checklist

| Criterion | Status |
|---|---|
| `cargo nextest run -p ucil-daemon server::test_search_code_basic` | PASS |
| `cargo nextest run -p ucil-daemon server::test_all_22_tools_registered` | PASS |
| `cargo nextest run -p ucil-daemon server::test_find_definition_tool` | PASS |
| `cargo nextest run -p ucil-daemon server::test_get_conventions_tool` | PASS |
| `cargo nextest run -p ucil-daemon server::` | PASS (all server-scope tests green) |
| `cargo nextest run -p ucil-daemon text_search::` | PASS (6 tests) |
| `cargo nextest run -p ucil-core knowledge_graph::` | PASS (21 tests) |
| `cargo nextest run --workspace` | PASS (249 tests, 1 skipped) |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo fmt --all --check` | PASS |
| `cargo doc -p ucil-daemon --no-deps` zero warnings | PASS |
| `cargo doc -p ucil-core --no-deps` zero warnings | PASS |
| `ucil-build/work-orders/0035-ready-for-review.md` present | PASS (this file) |
| `ucil-build/decisions/DEC-0009-search-code-in-process-ripgrep.md` present | PASS |

## What shipped

### Workspace (`Cargo.toml`)
- Added `ignore = "0.4"`, `grep-searcher = "0.1"`, `grep-regex = "0.1"`,
  `grep-matcher = "0.1"` under `[workspace.dependencies]`.  Each carries
  an inline comment citing `DEC-0009` and feature `P1-W5-F09`.
- `crates/ucil-daemon/Cargo.toml` pulls the four crates.

### `ucil-core` (`knowledge_graph.rs`)
- New public read helper
  `KnowledgeGraph::search_entities_by_name(&self, query: &str, limit: usize)
   -> Result<Vec<Entity>, KnowledgeGraphError>`.
- SQL: `WHERE name LIKE ?1 OR qualified_name LIKE ?1
        ORDER BY t_ingested_at DESC LIMIT ?2` — `query` wrapped as
  `%query%`.  `#[tracing::instrument]` span
  `ucil.core.kg.search_entities_by_name`.
- 6 `#[cfg(test)]` tests: exact match, substring, qualified-name match,
  `limit` respected, `t_ingested_at DESC` ordering, empty-table.

### `ucil-daemon` (`text_search.rs`, new module)
- `pub fn text_search(root: &Path, query: &str, max_results: usize)
   -> Result<Vec<TextMatch>, TextSearchError>`.
- Wraps `ignore::WalkBuilder` (with `.require_git(false)` so
  `.gitignore` applies outside a Git checkout),
  `grep_regex::RegexMatcherBuilder` (`.case_smart(true)`),
  `grep_searcher::SearcherBuilder` (`.line_number(true)`,
  `.binary_detection(BinaryDetection::quit(b'\0'))`).  Custom
  `CollectSink` halts via `Ok(false)` once the cap is reached.
- Public error enum `TextSearchError` with `thiserror::Error` —
  variants `BuildMatcher(#[from] grep_regex::Error)`,
  `Io(#[from] std::io::Error)`, `Walk(String)`.
- 6 `#[cfg(test)]` tests: `.gitignore` respect, 1-indexed line
  numbers, `max_results` cap, empty-result-is-empty-vec, zero-cap
  early exit, invalid-regex error.

### `ucil-daemon` (`server.rs`)
- New constants `SEARCH_CODE_DEFAULT_MAX_RESULTS = 50` and
  `SEARCH_CODE_MAX_RESULTS = 500`.
- New dispatch branch inside `handle_tools_call`:
  when `name == "search_code"` and a KG is attached, route to
  `Self::handle_search_code`.  Otherwise the existing phase-1 stub
  path runs — preserving invariant #9 for the 19 other tools.
- New `handle_search_code(id, params, kg)` method, plus helpers:
  - `parse_search_code_args(args) -> Result<SearchCodeArgs,
     SearchCodeReadError>` — validates `query` (required non-empty
     string), `root` (optional, must exist and be a directory),
     `max_results` (non-negative integer, default 50, clamped to
     500 with `tracing::warn!` on clamp).
  - `read_symbol_matches(kg, query, limit)` — KG mutex acquire →
    `search_entities_by_name` → mutex release.
  - `merge_search_results(symbols, texts, max_results) ->
     Vec<SearchCodeResult>` — dedup key is
     `(file_path, line_number)`; collision flips `source` to
     `"both"`, preserves `qualified_name`/`signature`, and
     overwrites `preview` with the `ripgrep` line.
  - `search_code_response(id, query, root, symbol_count, text_count,
     results) -> Value` — emits the MCP `tools/call` envelope with
    `_meta: {tool, source: "tree-sitter+ripgrep", count, query, root,
     symbol_match_count, text_match_count, results}`.
- `SearchCodeResult` struct serialises to
  `{source, file_path, line_number, preview, qualified_name?,
   signature?}` — matches WO-0035 scope_in bullet 7.
- 4 `#[cfg(test)]` unit tests on `merge_search_results`: no overlap,
  full overlap, partial overlap with collision, `max_results`
  truncation.
- 1 `#[tokio::test]` acceptance test + 5 negative tests:
  `test_search_code_basic` (merged symbol+text hit producing a
  `source == "both"` row), `_empty_query`, `_no_kg_returns_stub`,
  `_non_string_query`, `_nonexistent_root`, `_max_results_clamp`,
  `_only_text_no_symbol`.

## Stash-based reality-check

Per CLAUDE.md "What 'done' looks like" and WO acceptance #11, I
verified the acceptance test exercises real code:

1. Replaced the body of `handle_search_code` so the merged result
   vec was forced empty (keeping the symbol + text reads intact so
   the compile stays clean).
2. Re-ran `cargo nextest run -p ucil-daemon server::test_search_code_basic`.
3. Test **FAILED** at `_meta.count must be >= 1 (symbol + text
   merged): got 0` — panic at `server.rs:2867:5`, response body
   confirmed `"count": 0, "results": []`.
4. Restored the merge call; the test PASSED again.

This proves the test is driven by the real
`KnowledgeGraph::search_entities_by_name` + in-process `ripgrep`
pipeline, not by any mocked shortcut.

## Workspace test snapshot

```
Summary [ 5.075s] 249 tests run: 249 passed, 1 skipped
```

## Phase-1 invariant #9 preserved

`server::test_all_22_tools_registered` stays GREEN.  The 19 tools
other than `find_definition` / `get_conventions` / `search_code`
still return `_meta.not_yet_implemented: true`.

## Notes for the verifier

- The `ucil-core::KnowledgeGraph::search_entities_by_name` helper is
  the only new public surface on `ucil-core`; all `search_code`
  helpers in `ucil-daemon::server` are private or module-scoped.
- The `text_search` module is declared `pub(crate)` in
  `crates/ucil-daemon/src/lib.rs`; its public items are named with
  `pub` (the module's outer `pub(crate)` scopes them to the crate —
  `clippy::redundant_pub_crate` would otherwise fire on per-item
  `pub(crate)`).
- `ignore::WalkBuilder` defaults to `require_git(true)`; I pass
  `.require_git(false)` so `.gitignore` applies outside a Git
  checkout (exercised by the
  `text_search::tests::respects_gitignore_and_finds_hits` test
  that uses a fresh `tempfile::TempDir`).
