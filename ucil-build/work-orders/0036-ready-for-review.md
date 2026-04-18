# WO-0036 — ready for review

**Feature**: `P1-W4-F09` — `understand_code` MCP tool (tree-sitter AST + KG fusion).
**Branch**: `feat/WO-0036-understand-code-tool`
**Head commit**: `3d2a0dd`
**Master-plan anchors**: §3.2 row 1 (`understand_code` — explain a file/function/
  module); §18 Phase 1 Week 4 line 1751 (Phase 1 deliverable); §12.1 (entities
  schema); §12.2 (relations schema).
**Frozen acceptance selector**:
  `cargo nextest run -p ucil-daemon server::test_understand_code_tool`.

## Diff summary (from branch point `main..HEAD`)

```
 Cargo.lock                                |    2 +
 crates/ucil-daemon/Cargo.toml             |    8 +
 crates/ucil-daemon/src/lib.rs             |    1 +
 crates/ucil-daemon/src/server.rs          |  596 +++++++++++-
 crates/ucil-daemon/src/understand_code.rs | 1224 +++++++++++++++++++++++++++++
 crates/ucil-treesitter/src/parser.rs      |   93 +++
 6 files changed, 1923 insertions(+), 1 deletion(-)
```

No files under `tests/fixtures/**`, `scripts/gate/**`,
`scripts/flip-feature.sh`, `ucil-build/feature-list.json`,
`ucil-build/feature-list.schema.json`, or the master plan were touched.

## Commits (5, all pushed)

| SHA | Subject |
|-----|---------|
| `5712d52` | feat(treesitter): add Language::from_extension helper |
| `c568eea` | feat(daemon): wire understand_code MCP tool (KG+tree-sitter) |
| `fbe3238` | test(daemon): add frozen acceptance test for understand_code (file mode) |
| `aefd46d` | test(daemon): add supplementary understand_code tests (7 variants) |
| `3d2a0dd` | refactor(daemon): satisfy clippy pedantic/nursery on understand_code |

## Acceptance checklist

| Criterion | Status |
|---|---|
| `cargo nextest run -p ucil-daemon server::test_understand_code_tool` | PASS |
| `cargo nextest run -p ucil-daemon server::test_understand_code_tool_symbol_mode` | PASS |
| `cargo nextest run -p ucil-daemon server::test_understand_code_tool_auto_detect_file` | PASS |
| `cargo nextest run -p ucil-daemon server::test_understand_code_tool_missing_target` | PASS |
| `cargo nextest run -p ucil-daemon server::test_understand_code_tool_empty_target` | PASS |
| `cargo nextest run -p ucil-daemon server::test_understand_code_tool_unknown_symbol` | PASS |
| `cargo nextest run -p ucil-daemon server::test_understand_code_tool_no_kg_returns_stub` | PASS |
| `cargo nextest run -p ucil-daemon server::test_understand_code_tool_invalid_kind` | PASS |
| `cargo nextest run -p ucil-daemon server::test_all_22_tools_registered` | PASS |
| `cargo nextest run -p ucil-daemon server::test_find_definition_tool` | PASS |
| `cargo nextest run -p ucil-daemon server::test_get_conventions_tool` | PASS |
| `cargo nextest run -p ucil-daemon server::test_search_code_basic` | PASS |
| `cargo nextest run -p ucil-daemon` | PASS (114 tests) |
| `cargo nextest run --workspace` | PASS (270 tests, 1 skipped) |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo fmt --all --check` | PASS |
| `cargo doc -p ucil-daemon --no-deps` zero warnings | PASS |
| `ucil-build/work-orders/0036-ready-for-review.md` present | PASS (this file) |

## What shipped

### `ucil-treesitter` (`parser.rs`)
- New `pub fn Language::from_extension(ext: &str) -> Option<Language>`
  inherent method on the existing `Language` enum.  Maps `.rs` →
  `Rust`, `.py` → `Python`, `.ts`/`.tsx` → `TypeScript`, `.js`/`.jsx` →
  `JavaScript`, `.go` → `Go`, `.java` → `Java`, `.c` → `C`, `.cpp`/
  `.cc`/`.cxx`/`.hpp`/`.h` → `Cpp`, `.rb` → `Ruby`, `.sh`/`.bash` →
  `Bash`, `.json` → `Json`; case-insensitive. Returns `None` for
  unknown extensions.
- `#[cfg(test)]` unit tests (6 cases: `rs`, `py`, `tsx`, `go`,
  uppercase `PY`, unknown `xyz`).

### `ucil-daemon` (`Cargo.toml`)
- Added `tree-sitter.workspace = true` + `streaming-iterator.workspace
  = true` (rationale comment cites WO-0036 + P1-W4-F09).

### `ucil-daemon` (`understand_code.rs`, new module — 1224 lines)
- **Constants**: `UNDERSTAND_CODE_MAX_EDGES = 50`,
  `UNDERSTAND_CODE_MAX_TOP_LEVEL_SYMBOLS = 100` — both emit
  `tracing::warn!` when truncation fires.
- **Error enum** `UnderstandCodeError` (`thiserror::Error +
  #[non_exhaustive]`): `Io`, `Parse`, `KnowledgeGraph`,
  `UnsupportedLanguage(String)`, `TargetNotInRoot(String)`,
  `NotFound(String)`, `Poisoned(String)`.
- **Summary structs**: `UnderstandCodeFileSummary`, `FileSymbolRow`,
  `EntitySummary`, `RelationEdge`, `UnderstandCodeSymbolSummary`. All
  derive `Serialize + Debug + Clone + PartialEq + Eq`.
- **Pure helpers**:
  - `count_imports(source, lang) -> usize` — tree-sitter `Query`
    based (NOT string `.contains`), so `//` comments and string
    literals don't inflate the count. Supports Rust `use`, Python
    `import` / `from`, TypeScript / JavaScript `import` + `require()`
    calls, Go `import`.
  - `cap_and_sort_edges` — sorts by
    `(relation_type, peer_qualified_name)` then caps at
    `UNDERSTAND_CODE_MAX_EDGES`.
  - `project_relative`, `language_tag`, `symbol_kind_tag` — all
    `const fn` where possible.
- **Dispatch logic**:
  - `parse_target`, `parse_kind`, `parse_root` — argument extraction
    helpers; each returns `Result<..., Value>` so the caller can
    propagate JSON-RPC `-32602` envelopes.
  - `dispatch_file` — file-mode router (and `kind == "module"` falls
    through to this).
  - `explain_file(target_path, root, kg)` — reads file, detects
    language, parses, extracts symbols, counts imports + lines,
    queries KG via three path-key forms (canonical, raw, relative
    under `root`) to handle canonicalisation drift between ingest
    and lookup, zips on `(name, start_line)`.
  - `explain_symbol(target, kg, root)` — `get_entity_by_
    qualified_name` → `list_relations_by_source` +
    `list_relations_by_target` → peer resolution via
    `get_entity_by_id` (missing peers dropped at
    `tracing::debug`) → `containing_file` via `explain_file`.
  - `collect_edges` — extracted from `explain_symbol` with an
    explicit `drop(guard)` so the KG mutex is released before the
    `UnderstandCodeSymbolSummary` is assembled.
  - `handle_understand_code(id, params, kg) -> Value` — top-level
    handler, < 40 lines (dispatcher-only shape).

### `ucil-daemon` (`server.rs`)
- New dispatch branch inside `handle_tools_call`: when `name ==
  "understand_code"` and a KG is attached, route to
  `understand_code::handle_understand_code`.  Otherwise the existing
  phase-1 stub path runs — preserving invariant #9 for the 18 tools
  still stubbed (`find_definition`, `get_conventions`, `search_code`,
  and now `understand_code` are the four real handlers).
- Promoted `jsonrpc_error` to `pub(crate)` so
  `understand_code::handle_understand_code` can use it.
- New test-only fixture `build_understand_code_fixture` builds a
  `tempfile::TempDir` project root (copied from
  `tests/fixtures/rust-project`), opens a real `KnowledgeGraph`,
  runs the `IngestPipeline` against `src/util.rs`, seeds a
  synthetic `calls` relation (caller →`evaluate`) so symbol-mode
  tests observe at least one inbound edge, and returns
  `(server, kg, tmp, fixture_root, util_rs_canonical, evaluate_qn,
   caller_qn)`.
- Extracted `assert_understand_code_file_response(response,
  expected_target)` helper so the frozen acceptance test
  `test_understand_code_tool` stays under the
  `clippy::too_many_lines` threshold.

### Tests

| Test | Purpose |
|---|---|
| `server::test_understand_code_tool` | File-mode happy path — frozen selector |
| `server::test_understand_code_tool_symbol_mode` | Symbol-mode happy path w/ inbound edge |
| `server::test_understand_code_tool_auto_detect_file` | `kind` omitted + target-is-file → file mode |
| `server::test_understand_code_tool_missing_target` | `-32602` on missing target |
| `server::test_understand_code_tool_empty_target` | `-32602` on empty target |
| `server::test_understand_code_tool_invalid_kind` | `-32602` on `kind: "galaxy"` |
| `server::test_understand_code_tool_unknown_symbol` | `_meta.found == false`, `isError == false` |
| `server::test_understand_code_tool_no_kg_returns_stub` | `_meta.not_yet_implemented == true` w/o KG |
| `understand_code::tests::count_imports_rust_counts_use_declarations` | Rust `use` count |
| `understand_code::tests::count_imports_rust_ignores_comments_and_strings` | Tree-sitter query (not string scan) |
| `understand_code::tests::count_imports_python_counts_both_forms` | `import X` + `from X import Y` |
| `understand_code::tests::count_imports_typescript_counts_import_and_require` | ESM `import` + CJS `require()` |
| `understand_code::tests::count_imports_go_counts_declarations` | Go `import ( ... )` |
| `understand_code::tests::count_imports_returns_zero_for_unsupported_language` | JSON has no imports |
| `understand_code::tests::cap_and_sort_sorts_deterministically_and_caps` | Deterministic edge ordering |
| `understand_code::tests::language_tag_roundtrip` | `language_tag` is total |
| `understand_code::tests::relation_edge_falls_back_to_name_when_no_qualified_name` | Peer fallback |

## Stash-based reality-check

Per CLAUDE.md "What 'done' looks like" and WO acceptance:

1. Mutated `handle_understand_code` so `_meta.source` became
   `"MUTATION-TEST-BROKEN"` (everything else intact so the compile
   stayed clean).
2. Re-ran `cargo nextest run -p ucil-daemon
   server::test_understand_code_tool`.
3. Test **FAILED** at
   `_meta.source must be "tree-sitter+kg"` — panic at
   `server.rs:3355:5`.
4. Restored `"tree-sitter+kg"`; the test PASSED again.

This proves the test is driven by the real `ucil_treesitter::Parser`
+ real `KnowledgeGraph::list_entities_by_file` +
`get_entity_by_qualified_name` + `list_relations_by_source` /
`_by_target` pipeline, not by any mocked shortcut.

## Workspace test snapshot

```
Summary [ 5.103s] 270 tests run: 270 passed, 1 skipped
```

## Phase-1 invariant #9 preserved

`server::test_all_22_tools_registered` stays GREEN.  The 18 tools
other than `find_definition` / `get_conventions` / `search_code` /
`understand_code` still return `_meta.not_yet_implemented: true`.

## Notes for the verifier

- The only new public surface on `ucil-treesitter` is
  `Language::from_extension` (one inherent method on the existing
  `Language` enum).  All `understand_code` helpers in `ucil-daemon`
  are `pub(crate)` or private.
- `understand_code.rs` is declared `pub mod` in
  `crates/ucil-daemon/src/lib.rs` so the frozen nextest selector
  `server::test_understand_code_tool` resolves under the mainline
  executor profile (DEC-0007 + WO-0006 test-selector rule).  All
  `understand_code` module-local tests also live under
  `understand_code::tests`.
- `explain_file` intentionally tries **three** KG lookup keys
  (canonical path, raw path, relative-under-root) because ingest
  and handler calls may disagree on canonicalisation.  The
  `kg_entity_count == 36` observed in the acceptance test proves the
  canonical key hit.
- `handle_understand_code` is invoked through the `tools/call` router
  (private handler in `McpServer`), not exported as a standalone
  function — the rustdoc on the module prose documents this rather
  than an intra-doc link, to satisfy
  `rustdoc::private_intra_doc_links`.
- The symbol-mode happy path assertion is parameterised on the
  synthetic `caller_qn` relation seeded by
  `build_understand_code_fixture`, so `inbound_edges` is guaranteed
  to contain a `calls` edge regardless of upstream extractor
  changes.
- Tree-sitter 0.25's `QueryMatches` returns captures via
  `streaming_iterator::StreamingIterator`, not `Iterator`; the new
  `streaming-iterator` workspace dep is the reason.
