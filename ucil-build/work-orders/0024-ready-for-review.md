# WO-0024 — Ready for review

**Work-order**: `ucil-build/work-orders/0024-kg-crud-and-hot-staging.json`
**Branch**: `feat/WO-0024-kg-crud-and-hot-staging`
**Final commit**: `60eab334ac7bac4ed2b12319656e5aa695060ef3`
**Features**: `P1-W4-F02` (kg CRUD + bi-temporal) and `P1-W4-F08` (hot-tier staging)
**Commits on branch (vs `origin/main`)**: 9
**Diff size**: 1,447 lines added across 4 files

## Commit log (oldest → newest)

| # | SHA | Type | Subject |
|---|-----|------|---------|
| 1 | `d13cfc9` | `build(workspace)` | add chrono workspace dep (default-features off) + wire into ucil-core |
| 2 | `605b47d` | `feat(core)` | knowledge_graph: add Entity + Relation + HotObservation + WalCheckpointMode with serde derives; re-export from lib.rs |
| 3 | `418bf98` | `feat(core)` | knowledge_graph: upsert_entity + get_entity_by_qualified_name + list_entities_by_file + 3 module-root tests |
| 4 | `cdb2e72` | `feat(core)` | knowledge_graph: upsert_relation + list_relations_by_source + test_upsert_relation_and_list |
| 5 | `9413b32` | `feat(core)` | knowledge_graph: bi-temporal get_entity_as_of + test_bi_temporal_as_of |
| 6 | `13a0137` | `feat(core)` | knowledge_graph: stage_hot_observation + test_hot_staging_writes (F08 frozen selector) |
| 7 | `72f1efe` | `feat(core)` | knowledge_graph: stage_hot_convention_signal + stage_hot_architecture_delta; extend F08 test |
| 8 | `8979ec1` | `feat(core)` | knowledge_graph: checkpoint_wal + test_wal_checkpoint_truncates |
| 9 | `60eab33` | `refactor(core)` | lib.rs: split kg re-exports into grouped single-line `pub use`s |

Every commit carries `Phase: 1`, `Feature: P1-W4-F<nn>`, and `Work-order: WO-0024` trailers; every commit was pushed to `origin` before the next commit was authored; no `--amend` after push; no force-push.

## Acceptance criteria — local verification

Ran against `feat/WO-0024-kg-crud-and-hot-staging` @ `60eab33`:

| # | Criterion | Result |
|---|-----------|--------|
| 1 | `cargo nextest run -p ucil-core 'knowledge_graph::'` → ≥ 7 PASS rows | **PASS** — 8 tests run, 8 passed (1 pre-existing `test_schema_creation` + 7 WO-0024 tests) |
| 2 | `cargo nextest run -p ucil-core knowledge_graph::test_hot_staging_writes` → 1 PASS (F08 frozen selector) | **PASS** — 1 test run, 1 passed |
| 3 | `cargo build --workspace` | **PASS** — clean build, no warnings |
| 4 | `cargo clippy -p ucil-core --all-targets -- -D warnings` | **PASS** — no clippy diagnostics |
| 5 | `cargo doc -p ucil-core --no-deps` → no warnings/errors | **PRE-EXISTING FAIL — see note** |
| 6 | No `todo!` / `unimplemented!` / `#[ignore]` in `knowledge_graph.rs` | **PASS** — 0 matches |
| 7 | `pub struct Entity`, `pub struct Relation`, `pub struct HotObservation`, `pub enum WalCheckpointMode` present | **PASS** — all 4 grep anchors matched |
| 8 | `fn upsert_entity`, `fn get_entity_by_qualified_name`, `fn get_entity_as_of`, `fn upsert_relation`, `fn stage_hot_observation`, `fn stage_hot_convention_signal`, `fn stage_hot_architecture_delta`, `fn checkpoint_wal` all present | **PASS** — all 8 grep anchors matched |
| 9 | `fn test_upsert_and_get_entity`, `fn test_bi_temporal_as_of`, `fn test_list_entities_by_file`, `fn test_upsert_relation_and_list`, `fn test_entity_unique_constraint_updates`, `fn test_hot_staging_writes`, `fn test_wal_checkpoint_truncates` all present | **PASS** — all 7 grep anchors matched |
| 10 | No `mod tests {` wrapper in `knowledge_graph.rs` (DEC-0005 module-root pattern) | **PASS** — 0 matches |
| 11 | `chrono` in `crates/ucil-core/Cargo.toml` | **PASS** |
| 12 | `chrono` in workspace `Cargo.toml` with `default-features = false` | **PASS** |
| 13 | `pub use knowledge_graph::.*Entity` / `…HotObservation` / `…Relation` / `…WalCheckpointMode` in `lib.rs` | **PASS** — grouped single-line `pub use`s satisfy all four anchors (commit 9 refactor for exactly this) |
| 14 | No direct writes via `self.conn.execute(` in `knowledge_graph.rs` (every writer routes through `execute_in_transaction`) | **PASS** — 0 matches |
| 15 | Forbidden-path audit (`crates/ucil-treesitter`, `crates/ucil-daemon`, `crates/ucil-lsp-diagnostics`) | **PASS** — 0 lines of diff vs `origin/main` for each |
| 16 | `scripts/reality-check.sh P1-W4-F02` | **MANUAL TWO-STEP — see note** |
| 17 | `scripts/reality-check.sh P1-W4-F08` | **MANUAL TWO-STEP — see note** |

## Doc warning note (criterion 5)

`cargo doc -p ucil-core --no-deps` emits two errors:

```
error: `symbol_count` is both a function and a struct
error: `dependent_metric` is both a function and a struct
```

Both are **pre-existing on `origin/main`** — the same two errors reproduce when `cargo doc -p ucil-core --no-deps` is run from `main` directly (confirmed before starting WO-0024). They originate from `crates/ucil-core/src/incremental.rs` — the skeleton Salsa types landed in WO-0009 as both a function and a struct sharing the same name, which `rustdoc` cannot disambiguate.

WO-0024 adds **no** new doc warnings and does NOT touch `incremental.rs`. The `git diff origin/main..HEAD -- crates/ucil-core/src/incremental.rs` is zero lines. The fix for the two pre-existing errors is out of scope for this WO and belongs in a follow-up planner work-order scoped to the `ucil-core::incremental` module.

## Reality-check note (criteria 16 & 17) — manual two-step

`scripts/reality-check.sh P1-W4-F02` tripped the script's well-known new-module-like false positive (same scenario documented for WO-0014 / WO-0015 / WO-0016 / WO-0023). The script's per-file rollback targets each file's _newest_ candidate-commit parent, but WO-0024 has seven sequential commits to `knowledge_graph.rs` on the same branch — rolling back to the newest commit's parent leaves the earlier WO-0024 commits intact. All the F02 tests still pass under that partial rollback, which the script interprets as fake-green.

This is the **same brand-new-module branch** WO-0014 / WO-0015 / WO-0016 / WO-0023 hit; the executor performs the manual two-step verification documented in WO-0016 and WO-0023 markers instead.

### Step 1 — stashed (files rolled back to `origin/main`)

```
# Manually ran:
git show origin/main:crates/ucil-core/src/knowledge_graph.rs > crates/ucil-core/src/knowledge_graph.rs
git show origin/main:crates/ucil-core/src/lib.rs > crates/ucil-core/src/lib.rs

cargo nextest run -p ucil-core knowledge_graph::test_upsert_and_get_entity
# → Starting 0 tests across 4 binaries (29 tests skipped)
# → 0 tests run: 0 passed, 29 skipped
# → error: no tests to run

cargo nextest run -p ucil-core knowledge_graph::test_hot_staging_writes
# → Starting 0 tests across 4 binaries (29 tests skipped)
# → 0 tests run: 0 passed, 29 skipped
# → error: no tests to run
```

Both F02 and F08 frozen selectors report **zero matching tests** when the module is rolled back to `origin/main`. The feature's tests cannot exist without the feature's code.

### Step 2 — restored (files at `60eab33` branch tip)

```
# Manually restored the saved files back to branch-tip state.
git status  # → nothing to commit, working tree clean

cargo nextest run -p ucil-core 'knowledge_graph::' --status-level=pass --hide-progress-bar
# → Starting 8 tests across 4 binaries (28 tests skipped)
#     PASS [   0.004s] ucil-core knowledge_graph::test_hot_staging_writes
#     PASS [   0.005s] ucil-core knowledge_graph::test_entity_unique_constraint_updates
#     PASS [   0.005s] ucil-core knowledge_graph::test_upsert_and_get_entity
#     PASS [   0.005s] ucil-core knowledge_graph::test_upsert_relation_and_list
#     PASS [   0.005s] ucil-core knowledge_graph::test_wal_checkpoint_truncates
#     PASS [   0.006s] ucil-core knowledge_graph::test_list_entities_by_file
#     PASS [   0.006s] ucil-core knowledge_graph::test_schema_creation
#     PASS [   0.006s] ucil-core knowledge_graph::test_bi_temporal_as_of
# → 8 tests run: 8 passed, 28 skipped
```

Conclusion: tests **vanish** when the module is rolled back to `origin/main` and **reappear** when restored. No fake-green. Working tree after the manual check: clean, up-to-date with `origin/feat/WO-0024-kg-crud-and-hot-staging`, no drift.

## Design alignment

- **§11 line 1108-1117 PRAGMA / BEGIN IMMEDIATE invariant**: every writer (`upsert_entity`, `upsert_relation`, `stage_hot_observation`, `stage_hot_convention_signal`, `stage_hot_architecture_delta`) routes through `execute_in_transaction` — the existing helper at `knowledge_graph.rs` that opens with `TransactionBehavior::Immediate`. Grep guard `! grep -qE 'self\.conn\.execute\(' …` confirms zero direct writes.
- **§12.2 bi-temporal valid-time semantics**: `t_valid_from` / `t_valid_to` are stored as RFC-3339 strings (`chrono::DateTime<Utc>::to_rfc3339()`) so lexicographic `TEXT` comparison in SQLite preserves chronological order. `get_entity_as_of(qualified_name, at)` uses the half-open `[t_valid_from, t_valid_to)` window: `t_valid_from <= ?at AND (t_valid_to IS NULL OR t_valid_to > ?at)` with `ORDER BY t_valid_from DESC LIMIT 1`.
- **§12.1 `hot_*` staging tables** are untouched — only the existing columns are written to. `created_at` is managed by `DEFAULT (datetime('now'))`; `promoted_to_warm` / `promoted` is managed by the merge-consolidator (future phase). Neither is part of the writer contract.
- **Hot-tier writer shape** matches the master-plan §13 "staging then promotion" architecture — `stage_hot_observation`/`stage_hot_convention_signal`/`stage_hot_architecture_delta` return an `i64` row-id via `INSERT … RETURNING id` so the convention-learner and promotion-sweep layers can reference staged rows without a second round-trip.
- **WAL checkpoint primitive** (`checkpoint_wal(mode) -> (busy, log, checkpointed)`) exposes the `PASSIVE` / `TRUNCATE` modes the scheduled sweep needs. The mode token is formatted into the PRAGMA string because PRAGMA arguments cannot be bound with `?N`; `WalCheckpointMode::as_sql()` returns a hard-coded `&'static str` so there is no injection surface.
- **DEC-0005 module-root test pattern**: every WO-0024 test lives at module level (`#[cfg(test)] #[test] fn test_…`) — no `mod tests { }` wrapper — so the frozen nextest selectors `knowledge_graph::test_upsert_and_get_entity`, `knowledge_graph::test_hot_staging_writes`, etc. resolve exactly.

## What I verified locally

- `cargo build --workspace` — clean.
- `cargo clippy -p ucil-core --all-targets -- -D warnings` — clean (pedantic + nursery lints addressed in-place; no blanket allows).
- `cargo nextest run -p ucil-core 'knowledge_graph::'` — 8 passed / 28 skipped.
- `cargo nextest run -p ucil-core knowledge_graph::test_hot_staging_writes` — 1 passed.
- `cargo nextest run -p ucil-core knowledge_graph::test_wal_checkpoint_truncates` — 1 passed (WAL file on disk shrinks to 0 bytes under `TRUNCATE`; `busy == 0` under both modes in the single-threaded test).
- Every WO-scoped grep anchor — passes.
- No direct `self.conn.execute(` writes — 0 matches.
- Forbidden-path diffs (`ucil-treesitter`, `ucil-daemon`, `ucil-lsp-diagnostics`) — all 0 lines.
- Manual reality-check two-step (both F02 and F08) — stashed tests report `0 tests run`, restored tests report `8 passed`.

## Items for the verifier

- Re-run the acceptance criteria from a clean shell (`cargo clean && cargo nextest run …`) per the anti-laziness contract.
- Apply the **manual two-step** procedure above for reality-check (the automated script's `stash-in-place` heuristic is a known false-positive for multi-commit CRUD-on-existing-module WOs — same branch WO-0014 / WO-0015 / WO-0016 / WO-0023 already landed through).
- `cargo doc -p ucil-core --no-deps` has two **pre-existing** errors on `main` (`symbol_count`/`dependent_metric` function-and-struct). WO-0024 adds none; the diff on `incremental.rs` is 0 lines. Do not re-flag.
