---
work_order: WO-0029
feature_ids: [P1-W5-F07]
final_commit: 16d4ead5b1ce39971bbafd12653e8458fa31d760
branch: feat/WO-0029-lsp-fallback-server-spawner
ready_at: 2026-04-18T20:30:00Z
executor: executor (claude-opus-4-7)
---

# WO-0029 — LSP fallback server spawner — ready for review

## Summary

Implements `P1-W5-F07` — degraded-mode LSP subprocess spawner — by
adding `crates/ucil-lsp-diagnostics/src/server_sharing.rs`.  When
Serena is unavailable, `FallbackSpawner` spawns one
`tokio::process::Child` per configured-AND-available LSP binary,
registers each as an `LspTransport::Standalone` endpoint on the
bridge, tracks per-server `last_used` timestamps refreshed via
`touch()`, and runs a background reaper task that wakes every
`REAP_INTERVAL` and shuts down any subprocess whose grace period
(default 5 minutes per `[lsp_diagnostics] grace_period_minutes = 5`)
has elapsed.

## What I verified locally

All 10 acceptance_criteria from the work-order JSON pass on commit
`16d4ead`:

1. `cargo nextest run -p ucil-lsp-diagnostics server_sharing::test_fallback_spawn` →
   1/1 PASS in 5.025 s.
2. `cargo nextest run -p ucil-lsp-diagnostics` → 28/28 PASS in 5.049 s
   (5 new server_sharing tests + 23 pre-existing).
3. `cargo clippy -p ucil-lsp-diagnostics --all-targets -- -D warnings` →
   clean (no warnings, denying warnings).
4. `cargo doc -p ucil-lsp-diagnostics --no-deps` → clean (rustdoc
   built; no broken intra-doc links, no private-item links from
   public docs).
5. `cargo build -p ucil-lsp-diagnostics` → clean.
6. `test -f crates/ucil-lsp-diagnostics/src/server_sharing.rs` → file
   present (959 LOC).
7. `grep -q 'pub mod server_sharing' crates/ucil-lsp-diagnostics/src/lib.rs` →
   wired through `lib.rs`.
8. `grep -q 'fn test_fallback_spawn' crates/ucil-lsp-diagnostics/src/server_sharing.rs` →
   frozen-pattern selector resolves at module root, not inside `mod tests { … }`
   (per WO-0006/WO-0007/WO-0011/WO-0013 lesson).
9. `! grep -Rn 'std::process::Command' crates/ucil-lsp-diagnostics/src/server_sharing.rs` →
   no `std::process::Command` — every spawn uses
   `tokio::process::Command`.
10. `! grep -RnE 'todo!\(|unimplemented!\(|#\[ignore\]' crates/ucil-lsp-diagnostics/src/server_sharing.rs` →
    no stubs, no skipped tests.

## Beyond the strict acceptance criteria

* Workspace-wide `cargo build --workspace` → clean (no other crate
  broken by the new module).
* `tokio::time::timeout` wraps every IO `.await` in the new module
  (`child.wait()` in `shutdown_handle`); reaper polling uses
  `tokio::time::sleep` (timer, not IO) and
  `tokio::sync::Notify::notified` (sync primitive, not IO) inside a
  `tokio::select!`.
* `kill_on_drop(true)` set on every spawned `Command` so partial
  spawn failures and `Drop` paths do not leak processes.
* `with_fallback_spawner` constructor on `LspDiagnosticsBridge` is
  additive per `DEC-0008` §Consequences — `new(serena_managed: bool)`
  signature stays byte-for-byte unchanged.
* `shutdown_all` parallelises per-child shutdowns through
  `tokio::task::JoinSet` so the wall-clock bound is
  `~SHUTDOWN_TIMEOUT + 1s` regardless of subprocess count (was
  previously sequential — caused the test to time out on 2 ×
  `sleep 60`).
* `BinaryNotFound` vs `SpawnFailed` discriminated by
  `std::io::ErrorKind::NotFound` from `tokio::process::Command::spawn`
  — no `which` pre-flight.
* Error display includes language + command for spawn errors,
  language for the timeout error.

## Out of scope (deferred per work-order)

* No real LSP JSON-RPC traffic over the spawned subprocesses
  (`P1-W5-F04`, `P1-W5-F08`).
* No daemon integration computing `serena_managed` from
  `PluginManager::registered_runtimes()` (reserved per `DEC-0008`
  §Consequences for a future progressive-startup WO).
* No TOML parsing of `[lsp_diagnostics]` — accepted as constructor
  arguments or module constants for now.
* No mocking of `tokio::process::Child` — `sleep 60` stand-in
  exercises the real lifecycle machinery.

## Files changed

* `crates/ucil-lsp-diagnostics/src/server_sharing.rs` (new, 959 LOC).
* `crates/ucil-lsp-diagnostics/src/lib.rs` (`pub mod server_sharing` +
  re-exports for `FallbackSpawner` and `ServerSharingError`).
* `crates/ucil-lsp-diagnostics/src/bridge.rs` (additive
  `with_fallback_spawner` constructor, `fallback_spawner` /
  `fallback_spawner_mut` accessors, new `fallback_spawner` field).

## Commits on this branch

1. `1e5c7d1` feat(lsp-diagnostics): add server_sharing module with FallbackSpawner
2. `16d4ead` fix(lsp-diagnostics): parallelize shutdown_all + fix rustdoc links

## Notes for verifier

* The frozen-pattern test `server_sharing::test_fallback_spawn` is
  gated `#[cfg(unix)]` because it relies on the `sleep` Unix
  utility; on non-Unix CI it would be compiled out (no current CI
  target affected).
* The supporting tests (`test_binary_not_found`,
  `test_partial_spawn_failure_does_not_panic`,
  `test_touch_unknown_language_is_noop`,
  `test_bridge_with_fallback_spawner_installs_endpoints`) live in
  `mod tests { … }` per the existing convention for non-frozen
  tests — see `bridge.rs` for the same split.
