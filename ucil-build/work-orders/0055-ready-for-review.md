# WO-0055 — Ready for Review

**Feature:** `P2-W7-F08` (SCIP P1 install + G1Source)
**Branch:** `feat/WO-0055-scip-p1-install-and-g1-source`
**Final commit sha:** `2ff938b` (full sha redacted by pre-commit secret scanner; obtain via `git log -1 --format=%H feat/WO-0055-scip-p1-install-and-g1-source` in this branch)
**ADR:** [DEC-0014](../decisions/DEC-0014-scip-cli-to-sqlite-pipeline.md)

## What I verified locally

External binaries on PATH (verifier prerequisite per `scripts/verify/P2-W7-F08.sh`):

* `scip-rust v0.0.5` — installed as a UCIL-local shim at `~/.local/bin/scip-rust` that translates `scip-rust index --output <path>` into the actual `rust-analyzer scip <repo> --output <path>` invocation. Reason: upstream `scip-rust` v0.0.5 is a 1-line `rust-analyzer scip . > dump.scip` shell wrapper that does NOT support the `--output` flag the work-order's CLI shape uses; the shim provides the canonical interface on top of the real `rust-analyzer scip` subcommand. **The verifier MUST install `scip-rust` on PATH** — `scripts/devtools/install-scip-rust.sh` documents the install path; expect the verifier to either run that or install upstream (potentially with a similar shim).
* `scip CLI v0.7.1` — installed via the upstream Linux release tarball at `https://github.com/sourcegraph/scip/releases/download/v0.7.1/scip-linux-amd64.tar.gz`.

### Static checks (every acceptance_criteria grep)

```
[PASS] plugin.toml exists
[PASS] install-scip-rust.sh executable
[PASS] install-scip.sh executable
[PASS] scip.rs exists
[PASS] verify script executable
[PASS] manifest name (`name = "scip"`)
[PASS] [capabilities] table
[PASS] [indexer] table
[PASS] [ingest] table
[PASS] no [transport] table
[PASS] no [lifecycle] table
[PASS] no `"main"` ref
[PASS] DEC-0014 cite (`CLI-pipeline plugin per DEC-0014`)
[PASS] pub enum ScipError
[PASS] pub struct ScipReference
[PASS] pub struct ScipG1Source
[PASS] pub async fn index_repo
[PASS] pub async fn load_index_to_sqlite
[PASS] pub async fn query_symbol
[PASS] #[non_exhaustive]
[PASS] pub mod scip; in lib.rs
[PASS] re-export ScipError in lib.rs
[PASS] G1ToolKind::Scip
[PASS] G1ToolKind::Scip => 4 arm in authority_rank
```

### Cargo tests

* `cargo build -p ucil-daemon --tests` — clean.
* `cargo test -p ucil-daemon --lib scip::test_scip_p1_install` — `1 passed; 0 failed` in 2.60 s. The frozen selector at MODULE ROOT of `scip.rs` resolves cleanly per DEC-0007.  All 6 sub-assertions exercise the real path:
  - SA1 (`index_repo` round-trip): real `scip-rust` subprocess produces a non-empty `index.scip`.
  - SA2 (`load_index_to_sqlite`): real `scip` Rust crate decodes the protobuf, writes rows to a real `tempfile::TempDir`-managed SQLite store; `scip_symbols` table created; row count > 0.
  - SA3 (`query_symbol` against `evaluate`): returns ≥1 ref with `file_path` ending in `util.rs` (matches the load-bearing fixture-anchor `pub fn evaluate` at `tests/fixtures/rust-project/src/util.rs:128`).
  - SA4 (`ScipG1Source` standalone): `kind == G1ToolKind::Scip`, `status == Available`, entries non-empty.
  - SA5 (fan into `execute_g1`): orchestrator outcome includes `Scip`-kind result with `status == Available`.
  - SA6 (`authority_rank` regression sentinel): `authority_rank(G1ToolKind::Scip) == 4`.
* `cargo test -p ucil-daemon --test plugin_manager` — `3 passed; 0 failed`.
* `cargo test -p ucil-daemon --test plugin_manifests` — `3 passed; 0 failed`.
* `cargo test -p ucil-daemon plugin_manager::test_manifest_parser` — `1 passed; 0 failed`.
* `cargo test -p ucil-daemon plugin_manager::test_lifecycle_state_machine` — passes (test exists per filter; returned 1 passed under the lib selector).
* `cargo test -p ucil-daemon plugin_manager::test_hot_reload` — passes.
* `cargo test -p ucil-daemon plugin_manager::test_circuit_breaker` — passes.
* `cargo test -p ucil-daemon plugin_manager::test_hot_cold_lifecycle` — passes.
* `cargo test -p ucil-daemon executor::test_g1_parallel_execution` — `1 passed; 0 failed`.
* `cargo test -p ucil-daemon executor::test_g1_result_fusion` — passes.
* `cargo test -p ucil-daemon --test e2e_mcp_stdio` — `1 passed; 0 failed`.
* `cargo test -p ucil-daemon --test e2e_mcp_with_kg` — `1 passed; 0 failed`.
* `cargo test --workspace --no-fail-fast` — no `test result: FAILED` in any crate; all integration suites including `tests/integration/test_plugin_lifecycle.rs` (the test file in `forbidden_paths`) compile + pass cleanly.
* `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — clean (no `error` lines).
* `cargo fmt --check` — clean.

### Verify script

`bash scripts/verify/P2-W7-F08.sh` — exits 0 with `[OK] P2-W7-F08`.  Five-step shape:

1. detect `scip-rust` + `scip` on PATH (operator-actionable hint on absence).
2. print versions.
3. `cargo test -p ucil-daemon --lib scip::test_scip_p1_install` + alternation regex check.
4. forensic `scip print --json` over a fresh fixture index, parsed via `python3 -c 'import json,sys; json.load(...)'`.
5. `[OK] P2-W7-F08`; exit 0.

### Path guard

`git diff --name-only main...HEAD`:

```
Cargo.lock
Cargo.toml
crates/ucil-daemon/Cargo.toml
crates/ucil-daemon/src/executor.rs
crates/ucil-daemon/src/lib.rs
crates/ucil-daemon/src/plugin_manager.rs
crates/ucil-daemon/src/scip.rs
plugins/structural/scip/plugin.toml
scripts/devtools/install-scip-rust.sh
scripts/devtools/install-scip.sh
scripts/verify/P2-W7-F08.sh
```

Plus this file (`ucil-build/work-orders/0055-ready-for-review.md`).  Every entry is on the WO acceptance allow-list.

### Commit ladder (10 commits)

```
2ff938b fix(plugin_manager): remove indexer + ingest fields from PluginManifest, keep IndexerSection / IngestSection types
a2c12e3 feat(verify): implement scripts/verify/P2-W7-F08.sh end-to-end
b7aa5e5 test(scip): add test_scip_p1_install module-root acceptance test
ea89007 feat(scip): implement query_symbol + ScipG1Source G1Source impl
4295e84 feat(executor): extend G1ToolKind + authority_rank for Scip variant
c14c9a1 feat(scip): implement load_index_to_sqlite protobuf decode + sqlite ingest
0ee1711 feat(scip): implement index_repo subprocess wrapper
9b14bf2 feat(scip): add scip module skeleton with ScipError + ScipReference types
50d9555 feat(plugin_manager): make transport optional, add IndexerSection + IngestSection for CLI-pipeline plugins
8cc00cd chore(devtools): add install-scip-rust.sh + install-scip.sh helpers
c53ce8e feat(plugins): add scip plugin manifest under plugins/structural/scip/
2b58790 build(workspace): add scip + protobuf workspace deps for cross-repo symbol indexing
```

### Notable judgment calls (executor research per WO `plan_summary`)

1. **`scip` crate uses `protobuf` (rust-protobuf 3.7.2), not `prost`.**  DEC-0014's "transitive prost" hedge resolved to `protobuf` per the canonical `scip` 0.7.1 dependency list (verified on crates.io API on 2026-05-06).  `Cargo.toml` pins `protobuf = "=3.7.2"` to align exactly with `scip = "0.7.1"`'s exact-match requirement.  `ScipError::ProtobufDecode` carries `protobuf::Error`.  The decoder entry point is `Index::parse_from_bytes(&bytes)`.

2. **`tests/integration/test_plugin_lifecycle.rs` is in `forbidden_paths`.**  My initial extension added `pub indexer: Option<IndexerSection>` and `pub ingest: Option<IngestSection>` fields to `PluginManifest` per the WO scope_in.  Rust's struct-literal syntax then required all `PluginManifest { ... }` sites in that forbidden integration test to be updated, which I cannot do.  Fixed in the final commit by **dropping the indexer/ingest fields from `PluginManifest`** while keeping the `IndexerSection` + `IngestSection` types as standalone documentation.  Serde silently ignores unknown top-level tables (no `#[serde(deny_unknown_fields)]`), so `[indexer]` and `[ingest]` in the scip manifest parse cleanly via `PluginManifest::from_path`; the daemon's `crate::scip` module never reads those fields off the manifest (it uses hardcoded `scip-rust` + the `SCIP_SCHEMA` constant directly).  The acceptance criterion "`plugins/structural/scip/plugin.toml` parses cleanly via `PluginManifest::from_path`" is satisfied.

3. **`authority_rank` promoted from module-private to `pub(crate)`** so SA6 in `test_scip_p1_install` can call it directly.  Minimal change (just visibility); does not introduce new public API.

4. **`scip-rust` upstream binary is broken at v0.0.5** (1-line `rust-analyzer scip . > dump.scip` wrapper without `--output` support).  Local install uses a UCIL shim at `~/.local/bin/scip-rust` that translates the canonical `index --output <path>` CLI shape into `rust-analyzer scip <cwd> --output <path>`.  This is environmental — the verifier needs an equivalent shim or a hypothetical fixed upstream.  The plugin manifest declares `binary = "scip-rust"` per the work-order intent so a future upstream fix is forward-compatible.

5. **Cumulative re-export discipline** advanced to **10 consecutive WOs cleared on inline re-export**.  All 6 new public symbols (`index_repo`, `load_index_to_sqlite`, `query_symbol`, `ScipError`, `ScipG1Source`, `ScipReference`) plus the constants (`SCIP_INDEX_DEADLINE_SECS`, `SCIP_SCHEMA`) are re-exported in `lib.rs` in alphabetical order under the `pub use scip::{ ... }` block.

### Mutation checks (verifier-runs)

The WO `acceptance` list pre-bakes three mutations.  I have NOT run them locally (per the WO they are "verifier runs"); the test is set up so each one fails the corresponding sub-assertion when the targeted body is neutered:

* **M1**: neuter `index_repo` body to `Ok(PathBuf::new())` → SA1 panics on the path-not-empty assertion.
* **M2**: neuter `load_index_to_sqlite` body to `Ok(0)` → SA2 panics on the row-count > 0 assertion.
* **M3**: mutate `authority_rank` arm `G1ToolKind::Scip => 4` to `=> 0` → SA6 panics with the value mismatch.

Restore via `git checkout -- crates/ucil-daemon/src/scip.rs` (M1+M2) or `crates/ucil-daemon/src/executor.rs` (M3).
