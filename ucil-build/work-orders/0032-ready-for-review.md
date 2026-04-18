# WO-0032 — ready for review

**Branch**: `feat/WO-0032-treesitter-to-kg-pipeline`
**Final commit**: `8307fbfa4ddf1f7e44eb41e625e4f80cfa08aa94`
**Feature**: `P1-W4-F04` — wire tree-sitter extraction → knowledge graph population
**Executor**: `executor` subagent (session wrote commits `46103ae`, `91a408d`, `8307fbf`)

## What I verified locally

- [x] `cargo fmt --all --check` — clean (exit 0)
- [x] `cargo clippy --workspace --all-targets -- -D warnings` — clean (exit 0, all 10 crates)
- [x] `cargo nextest run -p ucil-daemon executor::test_treesitter_to_kg_pipeline` — **PASS** (frozen F04 selector, 0.100s)
- [x] `cargo nextest run -p ucil-daemon executor::` — **8/8 PASS** (1 frozen + 7 supplementary)
- [x] `cargo nextest run -p ucil-daemon` — **73/73 PASS** (no regression in prior daemon features)
- [x] `cargo nextest run -p ucil-core knowledge_graph::` — **9/9 PASS** (no regression in F02/F03/F08)
- [x] `cargo nextest run -p ucil-treesitter` — **53/53 PASS** (no regression in W2 features)
- [x] `cargo nextest run --workspace` — **213/213 PASS** (1 skipped in another crate, unrelated)
- [x] `cargo doc -p ucil-daemon --no-deps` — no `warning` / `error` lines
- [x] `test -f ucil-build/work-orders/0032-ready-for-review.md` — present (this file)

## Implementation summary

- **`crates/ucil-daemon/src/executor.rs`** (new module, 877 lines):
  - `pub struct IngestPipeline { parser: Parser, extractor: SymbolExtractor }` with
    `new()` + `Default` impl.
  - `pub fn ingest_file(&mut self, kg: &mut KnowledgeGraph, path: &Path) -> Result<usize, ExecutorError>`
    reads the file, infers language from extension, parses it with
    `ucil_treesitter::Parser`, extracts symbols with `SymbolExtractor`,
    and upserts the entire batch inside **ONE** `kg.execute_in_transaction(|tx| …)`
    per file (WAL-batching invariant, master-plan §11 line 1117 + §18 line 1759).
  - Raw `INSERT ... ON CONFLICT(qualified_name, file_path, t_valid_from) DO UPDATE
    SET t_last_verified = datetime('now'), access_count = access_count + 1`
    SQL is inlined inside the closure rather than calling
    `KnowledgeGraph::upsert_entity` — the latter opens its own transaction,
    which would defeat the "one transaction per file" contract.
  - Idempotency: synthesised non-NULL `qualified_name` of shape
    `{file_path}::{name}@{line}:{col}` + constant
    `t_valid_from = "1970-01-01T00:00:00+00:00"` so the
    `UNIQUE(qualified_name, file_path, t_valid_from)` index drives the
    `ON CONFLICT` path. (SQLite treats NULL as distinct in UNIQUE
    constraints, so NULLs would defeat the upsert.)
  - `pub enum ExecutorError` with `#[non_exhaustive]` +
    `#[derive(thiserror::Error, Debug)]` — variants `Io`,
    `UnsupportedExtension`, `Parse(#[from] ParseError)`,
    `KnowledgeGraph(#[from] KnowledgeGraphError)`.
  - Constants: `pub const SOURCE_TOOL: &str = "tree-sitter"` and
    `pub const TREE_SITTER_VALID_FROM: &str = "1970-01-01T00:00:00+00:00"`
    — both re-exported from `lib.rs` so downstream callers and tests
    reference the same value.
  - Helpers (private):
    `language_from_extension`, `kind_tag`, `language_tag`,
    `build_qualified_name`, `compute_source_hash`, `symbol_to_row`.
    Both `kind_tag` and `language_tag` carry wildcard `_ => "unknown"`
    arms because `SymbolKind` and `Language` are `#[non_exhaustive]`
    in `ucil_treesitter` — the arms are unreachable through
    `language_from_extension`'s allow-list but required for the
    compiler.
- **`crates/ucil-daemon/src/lib.rs`**: added `pub mod executor;` and
  `pub use executor::{ExecutorError, IngestPipeline, SOURCE_TOOL,
  TREE_SITTER_VALID_FROM};` + a paragraph on the module in the crate
  docstring.
- **`crates/ucil-daemon/Cargo.toml`**: added `rusqlite`,
  `ucil-core` (path dep), `ucil-treesitter` (path dep). Comment
  explains `rusqlite` is needed so the closure body in
  `execute_in_transaction` can build `rusqlite::params!` bindings.

## Tests (8 total, all at module root per DEC-0005)

1. `executor::test_treesitter_to_kg_pipeline` (frozen acceptance
   selector) — ingests `tests/fixtures/rust-project/src/util.rs` on
   a temp KG, asserts entities present, `source_tool = "tree-sitter"`,
   `language = "rust"`, `t_valid_from` matches constant, `start_line
   >= 1`, `qualified_name` non-null. Re-runs and asserts row count is
   stable (idempotent) AND `SUM(access_count) >= row_count` (the
   `ON CONFLICT DO UPDATE` path fired).
2. `executor::test_ingest_multi_file_isolation` — ingests both
   `util.rs` and `parser.rs`; asserts each file's rows carry that
   file's `file_path` and `source_tool = "tree-sitter"`.
3. `executor::test_ingest_rejects_unsupported_extension` — `.xyz`
   path (not on disk) returns `ExecutorError::UnsupportedExtension`
   without opening the file.
4. `executor::test_language_from_extension_table` — pins every
   documented extension mapping + case-insensitivity + `None` for
   unknown/extensionless.
5. `executor::test_kind_tag_covers_all_variants` — every known
   `SymbolKind` → stable lowercase tag.
6. `executor::test_build_qualified_name_shape_and_stability` — pins
   `{file}::{name}@{line}:{col}` shape, stable across calls, distinct
   `start_line` → distinct `qualified_name`.
7. `executor::test_compute_source_hash_deterministic_and_hex16` —
   16 hex chars, deterministic across calls. Shape-only assertion on
   the second call with distinct inputs (we tolerate SipHash-1-3
   collisions on 16 hex chars).
8. `executor::test_ingest_pipeline_default_available` — `Default`
   impl exists.

## Commit split (3 commits, DEC-0005 applies to commit #2)

1. `46103ae build(daemon): add ucil-treesitter + rusqlite deps …` (1 file, 3 lines)
2. `91a408d feat(daemon): add executor module — tree-sitter → KG ingest pipeline`
   (3 files, 667 lines — DEC-0005 module-introduction exception)
3. `8307fbf test(daemon): add supplementary unit coverage for executor helpers`
   (1 file, 221 lines)

## No forbidden-path touched

Verified: `ucil-build/feature-list.json`, `ucil-master-plan-v2.1-final.md`,
`tests/fixtures/**`, `scripts/gate/**`, `scripts/flip-feature.sh` — all untouched.
Only the files in "Implementation summary" were modified/created.

## Test runtime

- `executor::test_treesitter_to_kg_pipeline` — 0.100s (parses `util.rs`
  twice, two transactions)
- `executor::test_ingest_multi_file_isolation` — 0.164s (ingests
  `util.rs` + `parser.rs`)
- Other 6 tests — < 5ms each (pure helper tests, no fs/parse)
