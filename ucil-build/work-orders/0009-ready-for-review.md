# WO-0009 — Ready for Review

**Work-order:** `ucil-build/work-orders/0009-salsa-and-plugin-manager.json`
**Branch:** `feat/WO-0009-salsa-and-plugin-manager`
**Final commit SHA:** `46abfaf4a9db02d19fa128da2c993948e9c6ccd7`
**Worktree:** `/home/rishidarkdevil/Desktop/ucil-wt/WO-0009`
**Date:** 2026-04-17
**Features:** `P1-W3-F04` (ucil-core Salsa skeleton), `P1-W3-F05` (ucil-daemon plugin-manager skeleton)

## Commits on this branch (main → HEAD)

```
46abfaf feat(daemon): plugin-manager skeleton (manifest, discover, spawn, health)
5c2739a feat(core): salsa incremental engine skeleton with early-cutoff DAG
45d3d41 build: add salsa 0.22, toml/serde_json deps, and mock-mcp-plugin bin
```

## What I verified locally

Each acceptance criterion from the work-order was run explicitly. AC1–AC8 PASS. AC9 is a
known harness limitation (see final bullet).

- **AC1 — `cargo build -p ucil-core -p ucil-daemon`**: clean build, no warnings.
- **AC2 — `cargo nextest run -p ucil-core incremental::`**: 5 tests pass
  (`memoise_skips_second_call_with_identical_input`,
  `invalidate_on_mtime_change_forces_recompute`,
  `early_cutoff_skips_downstream_recompute`,
  `contents_change_propagates_through_downstream`,
  `concrete_database_default_constructs`).
  Confirmed via `salsa::EventKind::WillExecute` event-hook logging — not a
  self-reported boolean. Early-cutoff test observes `symbol_count` re-executing
  and `dependent_metric` being skipped, as required.
- **AC3 — `cargo nextest run -p ucil-daemon plugin_manager::`**: 13 tests pass
  (10 unit tests in `src/plugin_manager.rs` + 3 integration tests in
  `tests/plugin_manager.rs` wrapped in `mod plugin_manager { ... }` so nextest
  picks them up under the `plugin_manager::` selector).
- **AC4 — `cargo clippy -p ucil-core -p ucil-daemon -- -D warnings`**: clean.
  Also fixed a pre-existing `clippy::unnecessary_map_or` in
  `crates/ucil-core/tests/smoke.rs` (not authored by this WO but blocked
  `-D warnings` on newer rustc); documented in the commit body of 5c2739a.
- **AC5 — `cargo fmt --check` on both crates**: clean.
- **AC6 — No forbidden stubs in shipped code**: grepped for `todo!`,
  `unimplemented!`, `NotImplementedError`, `.unwrap()` (outside `#[cfg(test)]`)
  across both crates. All `.unwrap`/`.expect` usage is gated to tests. No
  `#[ignore]`, `.skip`, or `xfail` attributes added.
- **AC7 — `tokio::time::timeout` wraps every health-check `.await`**:
  verified by inspecting `PluginManager::health_check` — the entire stdio
  read/write sequence is enclosed in a single `tokio::time::timeout` call
  keyed to the named const `HEALTH_CHECK_TIMEOUT_MS = 5_000`. The timeout
  path is itself exercised by the unit test
  `health_check_times_out_when_plugin_does_not_respond` which spawns
  `/usr/bin/sleep 30` as the "plugin" and asserts `PluginError::HealthCheckTimeout`.
- **AC8 — Mock MCP plugin is a real subprocess**: the integration test
  `spawn_and_health_check_returns_mock_tools` uses
  `env!("CARGO_BIN_EXE_mock-mcp-plugin")` to invoke the compiled
  `crates/ucil-daemon/tests/support/mock_mcp_plugin.rs` binary (declared
  as `[[bin]]` in `Cargo.toml`). No mocking of `tokio::process::Command`,
  no stubbing of child stdio.
- **AC9 — `scripts/reality-check.sh`**: KNOWN HARNESS LIMITATION.
  The script takes a feature-id, not a WO-id; `scripts/reality-check.sh WO-0009`
  exits 2 with "Feature WO-0009 not found" as expected. When invoked with
  the real feature-ids (`P1-W3-F04`, `P1-W3-F05`) it fails with
  "ZERO tests run with code stashed — module was removed". This is the
  identical harness behaviour observed for already-verified features
  `P1-W2-F01` and `P1-W2-F05`, which are both `passes=true` in the current
  feature-list despite the same reality-check outcome. Per the pattern
  established by prior verifications (and consistent with the DEC-0007
  scope discussion around per-WO verifier gate composition), this is not
  a per-WO fix — it is a harness-script issue that needs a separate ADR
  or triage ticket. Flagged here for the verifier's attention rather
  than silently stubbed around.

## Files touched on this branch

```
crates/ucil-core/Cargo.toml                           (dep: salsa = "0.22")
crates/ucil-core/src/lib.rs                           (pub mod incremental + re-exports)
crates/ucil-core/src/incremental.rs                   (NEW — ~300 LOC + 5 tests)
crates/ucil-core/tests/smoke.rs                       (clippy fix: is_some_and)
crates/ucil-daemon/Cargo.toml                         (deps: toml, serde_json, tempfile;
                                                        bin: mock-mcp-plugin)
crates/ucil-daemon/src/lib.rs                         (pub mod plugin_manager + re-exports)
crates/ucil-daemon/src/plugin_manager.rs              (NEW — ~580 LOC + 10 unit tests)
crates/ucil-daemon/tests/plugin_manager.rs            (NEW — 3 integration tests)
crates/ucil-daemon/tests/support/mock_mcp_plugin.rs   (NEW — real JSON-RPC subprocess bin)
```

## Forbidden-pattern scan (explicitly re-checked before handoff)

- `todo!()` / `unimplemented!()` / `NotImplementedError` / `pass`-only bodies: none.
- `#[ignore]` / `.skip()` / `xfail` / commented-out assertions: none.
- Mocks of `tokio::process::Command`, salsa internals, or child stdio: none.
  The plugin integration tests spawn the real compiled mock-mcp-plugin binary.
- Edits under `tests/fixtures/**`: none.
- Edits to `feature-list.json`: none.
- `git commit --no-verify`, `--amend`-after-push, `push --force`: none.

## Handoff notes for the verifier

1. Run from a clean slate as required by the verifier contract:
   `cargo clean && cargo nextest run -p ucil-core incremental::` then
   `cargo nextest run -p ucil-daemon plugin_manager::`.
2. Early-cutoff behaviour is observed via `salsa::EventKind::WillExecute` —
   not a proxy for "tests passed". Log contents are asserted directly.
3. The timeout test uses `/usr/bin/sleep 30` (which reads stdin but does
   not respond); on CI images that lack `/usr/bin/sleep` this test will
   fail — if that happens, reproduce locally before flagging.
4. AC9 (reality-check.sh) is a harness-level issue predating this WO.
   Recommend recording it as a separate escalation / ADR rather than
   blocking WO-0009 on a script that fails identically for at least
   two already-`passes=true` features.
