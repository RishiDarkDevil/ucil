---
work_order: WO-0030
slug: kg-crud-hot-staging-retry
branch: feat/WO-0030-kg-crud-hot-staging-retry
final_commit: 6a394e2038b6462fac6f03abb4a00e6082359cb7
features: [P1-W4-F02, P1-W4-F08]
supersedes: [WO-0012, WO-0020, WO-0024]
created_at: 2026-04-18
executor: executor
---

# WO-0030 — Ready for review

Re-land of the knowledge_graph CRUD + bi-temporal queries + hot-staging writes onto a branch cut from current main (which now includes the rustdoc disambiguation fix `f6ec86e`).

## Method

Took the **preferred path** per the work-order: cherry-picked the 9 implementation commits `d13cfc9..60eab33` from the stale `feat/WO-0024-kg-crud-and-hot-staging` branch onto a fresh branch from `main` (`bafebd5`).

One conflict encountered and resolved mechanically:
- `Cargo.lock` during the first cherry-pick (`d13cfc9 build(workspace): add chrono workspace dep`). Resolved by taking the incoming (`--theirs`) lockfile and then running `cargo check --offline` to regenerate it against current `main`'s deps. `Cargo.toml` and `crates/ucil-core/Cargo.toml` merged cleanly. The remaining 8 commits applied without conflict.

## Commits on branch (oldest → newest)

| sha | summary |
|-----|---------|
| `8dc79ac` | build(workspace): add chrono workspace dep (default-features off) + wire into ucil-core |
| `81f012c` | feat(core): knowledge_graph: add Entity + Relation + HotObservation + WalCheckpointMode with serde derives; re-export from lib.rs |
| `46bb793` | feat(core): knowledge_graph: upsert_entity + get_entity_by_qualified_name + list_entities_by_file + 3 module-root tests |
| `6b91c90` | feat(core): knowledge_graph: upsert_relation + list_relations_by_source + test_upsert_relation_and_list |
| `3a83f04` | feat(core): knowledge_graph: bi-temporal get_entity_as_of + test_bi_temporal_as_of |
| `46ecb2a` | feat(core): knowledge_graph: stage_hot_observation + test_hot_staging_writes (F08 frozen selector) |
| `d24b944` | feat(core): knowledge_graph: stage_hot_convention_signal + stage_hot_architecture_delta; extend F08 test |
| `1192423` | feat(core): knowledge_graph: checkpoint_wal + test_wal_checkpoint_truncates |
| `6a394e2` | refactor(core): lib.rs: split kg re-exports into grouped single-line `pub use`s |

Final HEAD: `6a394e2038b6462fac6f03abb4a00e6082359cb7`
Commit count on branch (main..HEAD): **9**.

## What I verified locally

All acceptance criteria from the work-order were run on the worktree `../ucil-wt/WO-0030` and exited clean:

- `cargo fmt --check` → exit 0
- `cargo clippy --workspace -- -D warnings` → exit 0 (clean across all 8 workspace crates)
- `cargo nextest run -p ucil-core knowledge_graph::` → **8 tests run, 8 passed**:
  - `test_upsert_and_get_entity`
  - `test_entity_unique_constraint_updates`
  - `test_list_entities_by_file`
  - `test_upsert_relation_and_list`
  - `test_bi_temporal_as_of`
  - `test_hot_staging_writes` ← **P1-W4-F08 frozen selector**
  - `test_wal_checkpoint_truncates`
  - `test_schema_creation` (pre-existing)
- `cargo nextest run -p ucil-core knowledge_graph::test_hot_staging_writes` → 1/1 passed
- `cargo doc -p ucil-core --no-deps 2>&1 | { ! grep -qE '^(warning|error)'; }` → exit 0 (WO-0024's sole blocker — pre-existing rustdoc ambiguity in `incremental.rs` — is resolved on main at `f6ec86e`)
- Branch pushed to `origin/feat/WO-0030-kg-crud-hot-staging-retry` with 9 commits
- `git log origin/feat/WO-0030-kg-crud-hot-staging-retry --format=%H main..HEAD | wc -l` → 9 (≥ 9)

## Scope adherence

- No touches to `crates/ucil-core/src/incremental.rs` (scope_out).
- No touches to `ucil-build/feature-list.json`, master plan, `tests/fixtures/**`, `scripts/gate/**`, `scripts/flip-feature.sh`.
- No modifications to `crates/ucil-daemon/` or `crates/ucil-lsp-diagnostics/` (left alone per scope_out).
- Diff scope: `Cargo.toml`, `Cargo.lock`, `crates/ucil-core/Cargo.toml`, `crates/ucil-core/src/knowledge_graph.rs`, `crates/ucil-core/src/lib.rs` — as specified.

Ready for critic + verifier.
