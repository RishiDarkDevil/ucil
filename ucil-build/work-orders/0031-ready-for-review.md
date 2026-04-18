# WO-0031 — ready for review

**Branch**: `feat/WO-0031-symbol-resolution`
**Final commit**: `1905ffaee77a56ff3cc54e3ac8844516eec8d8f1`
**Feature**: `P1-W4-F03` — symbol resolution
**Executor**: `executor` subagent (session wrote commits `ac9d07c`, `906783e`, `6e93542`, `1905ffa`)

## What I verified locally

- [x] `cargo fmt --check` — clean (exit 0)
- [x] `cargo clippy --workspace -- -D warnings` — clean (exit 0, all 10 crates)
- [x] `cargo nextest run -p ucil-core knowledge_graph::test_symbol_resolution` — **PASS** (frozen F03 selector)
- [x] `cargo nextest run -p ucil-core knowledge_graph::` — **9/9 PASS** (prior F01/F02/F08 tests regression-clean)
- [x] `cargo doc -p ucil-core --no-deps` — no `^warning` / `^error` lines
- [x] Branch on origin has ≥4 commits (1905ffa, 6e93542, 906783e, ac9d07c)
- [x] `SymbolResolution` struct defined with the spec's field order
      (`file_path`, `start_line`, `signature`, `doc_comment`, `parent_module`)
      and derive set (`Debug, Clone, PartialEq, Eq, Serialize, Deserialize`)
- [x] `KnowledgeGraph::resolve_symbol(&self, name, file_scope)` — read-only,
      uses the SQL the WO specified verbatim
      (`name = ?1 OR qualified_name = ?1 OR qualified_name LIKE '%::' || ?1`,
      optional `AND file_path = ?2`, `ORDER BY t_ingested_at DESC LIMIT 1`)
- [x] `#[tracing::instrument(level="debug", skip(self), fields(name = %name, scoped = file_scope.is_some()))]` attached per master-plan §15.2
- [x] `test_symbol_resolution` lives at **module root** (not inside `mod tests { }`)
      per DEC-0005
- [x] `SymbolResolution` re-exported from `crates/ucil-core/src/lib.rs`
      alongside `Entity`, `Relation`, etc.
- [x] Both `SymbolResolution` and `resolve_symbol` carry rustdoc with
      master-plan citations (§12.1 and §18 Phase 1 Week 4 line 1749)
- [x] No forbidden-path touched:
      `ucil-build/feature-list.json`, `ucil-master-plan-v2.1-final.md`,
      `tests/fixtures/**`, `scripts/gate/**`, `scripts/flip-feature.sh`,
      `crates/ucil-core/src/incremental.rs` — all untouched

## Implementation notes

- `parent_module` is **derived** in Rust at read time via
  `qualified_name.rsplit_once("::")` — no schema column added
  (master-plan §12.1 schema is frozen; adding a column would require an ADR).
  A `qualified_name` of `NULL` or one without any `::` separator yields
  `parent_module = None`.
- Tie-breaking determinism: SQLite's `datetime('now')` schema default has
  second precision, so two back-to-back upserts can share a `t_ingested_at`
  value, and `ORDER BY … DESC LIMIT 1` stability under `LIMIT` is
  implementation-defined. The test inserts a `>1s sleep` between the two
  `"parse"`-named rows to force a strictly-later `t_ingested_at` on the
  second insert so the "newest ingest wins" contract is deterministic.
- A new `resolution_from_row` decoder sits alongside `entity_from_row` /
  `relation_from_row` at the module tail and handles the
  `qualified_name → parent_module` derivation in one place.
- Commits split into four conventional-commit chunks per the WO's
  suggested cadence:
    1. `ac9d07c feat(core): add SymbolResolution struct`
    2. `906783e feat(core): add KnowledgeGraph::resolve_symbol`
    3. `6e93542 test(core): cover symbol resolution`
    4. `1905ffa docs(core): re-export SymbolResolution from lib`

## Test runtime

`knowledge_graph::test_symbol_resolution` takes ~1.1 seconds (the
deliberate sleep; everything else is microsecond-scale on a tempfile
SQLite).
