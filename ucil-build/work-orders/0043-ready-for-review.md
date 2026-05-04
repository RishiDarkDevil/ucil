---
work_order: WO-0043
slug: plugin-hot-reload-and-circuit-breakers
phase: 2
week: 6
features: [P2-W6-F03, P2-W6-F04]
branch: feat/WO-0043-plugin-hot-reload-and-circuit-breakers
final_commit: 257a1e1
ready_at: 2026-05-04T19:05:00Z
---

# WO-0043 — Ready for review

## Summary

Implements **P2-W6-F03 (plugin hot-reload)** and **P2-W6-F04 (circuit
breakers)** entirely inside `crates/ucil-daemon/src/plugin_manager.rs`,
plus a 2-symbol re-export tweak in `crates/ucil-daemon/src/lib.rs`.
Single-file blast radius preserved (WO-0042 lesson — same template).

Final commit: `257a1e1` on branch
`feat/WO-0043-plugin-hot-reload-and-circuit-breakers`.

## Commits (10 total)

```
b83ce15 feat(daemon): extend PluginRuntime with in_flight gate + restart_attempts counter
188850c feat(daemon): add MAX_RESTARTS + CIRCUIT_BREAKER_BASE_BACKOFF_MS constants and PluginManager backoff config
4d6756e feat(daemon): add PluginManager::add to register pre-built PluginRuntimes
e44d6d9 feat(daemon): add PluginError::NotFound + CircuitBreakerOpen variants
067cec4 feat(daemon): add PluginManager::reload with in-flight drain + tracing
b60217b feat(daemon): add restart_with_backoff + circuit-breaker trip semantics
8e206ec feat(daemon): re-export MAX_RESTARTS + CIRCUIT_BREAKER_BASE_BACKOFF_MS from lib.rs
528eb6a test(daemon): add plugin_manager::test_hot_reload acceptance test
934243b test(daemon): add plugin_manager::test_circuit_breaker acceptance test
257a1e1 docs(daemon): backtick MAX_RESTARTS in test_circuit_breaker rustdoc
```

The two test commits exceed the ~50 LOC soft target (test_hot_reload
+117 LOC, test_circuit_breaker +91 LOC) — covered by DEC-0005 spirit
for module-root acceptance tests.

## What I verified locally

All 28 acceptance criteria from `0043-plugin-hot-reload-and-circuit-breakers.json`
were run inside the WO-0043 worktree and pass:

- [x] `cargo build -p ucil-daemon --quiet` — clean
- [x] `cargo test -p ucil-daemon plugin_manager::test_hot_reload` — 1 passed
- [x] `cargo test -p ucil-daemon plugin_manager::test_circuit_breaker` — 1 passed
- [x] `cargo test -p ucil-daemon plugin_manager::test_hot_cold_lifecycle` — 1 passed (P1-W3-F06 regression guard)
- [x] `cargo test -p ucil-daemon plugin_manager::test_manifest_parser` — 1 passed (P2-W6-F01 regression guard)
- [x] `cargo test -p ucil-daemon plugin_manager::test_lifecycle_state_machine` — 1 passed (P2-W6-F02 regression guard)
- [x] `cargo test -p ucil-daemon --lib` — 123 passed; 0 failed
- [x] `cargo test -p ucil-daemon --test plugin_manager` — 3 passed; 0 failed
- [x] `cargo test -p ucil-daemon --test e2e_mcp_stdio` — 1 passed; 0 failed
- [x] `cargo test -p ucil-daemon --test e2e_mcp_with_kg` — 1 passed; 0 failed
- [x] `cargo test --workspace --no-fail-fast` — no FAILED lines
- [x] `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — clean
- [x] `cargo doc -p ucil-daemon --no-deps` — no `^error`, no `warning: unresolved`
- [x] `cargo fmt --check` — clean
- [x] `grep -q 'fn test_hot_reload' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'fn test_circuit_breaker' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'pub async fn reload' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'pub async fn restart_with_backoff' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'MAX_RESTARTS' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'CIRCUIT_BREAKER_BASE_BACKOFF_MS' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'CircuitBreakerOpen' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'NotFound' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'restart_attempts' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'in_flight' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'with_circuit_breaker_base' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'pub fn add' crates/ucil-daemon/src/plugin_manager.rs` — yes
- [x] `grep -q 'MAX_RESTARTS' crates/ucil-daemon/src/lib.rs` — yes
- [x] `grep -q 'CIRCUIT_BREAKER_BASE_BACKOFF_MS' crates/ucil-daemon/src/lib.rs` — yes

## Mutation checks (pre-baked, for the verifier)

The work-order's `acceptance` block names two function-body mutations:

1. **Stash the body of `PluginManager::reload`** so it returns `Ok(())`
   without acquiring the writer lock or calling `health_check`.
   Expected effect: `test_hot_reload` fails on the
   `elapsed >= Dur::from_millis(100)` assertion (`plugin_manager.rs`
   inside `test_hot_reload`) — the in-flight reader is never drained.

2. **Stash the body of `PluginManager::restart_with_backoff`** so it
   returns `Ok(())` immediately. Expected effect: `test_circuit_breaker`
   fails on the `Err(PluginError::CircuitBreakerOpen { .. })` match —
   the runtime never trips.

Both mutations restore green when the function bodies are restored.

## Implementation notes / deviations

- **`pub fn add`**, not `pub async fn add` — chosen for the
  acceptance-criterion grep `grep -q 'pub fn add'` and consistent with
  the WO suggestion of `try_write` shape. On lock contention the
  function emits a `tracing::warn!` and skips the push (rust-style.md
  forbids `unwrap`/`expect` outside `#[cfg(test)]`); contention is
  unexpected during setup so this is a programmer-error path.

- **Lock-guard tightening (clippy `significant_drop_tightening`)** —
  inside `reload` and `restart_with_backoff` every `runtimes.read()` /
  `runtimes.write()` is consumed inline (no `let` binding) so the
  guard drops at end-of-expression. Confirmed `cargo clippy --
  -D warnings` clean.

- **Single-arm `if`-flow over `match` (clippy `single_match_else`)** —
  inside `restart_with_backoff` I replaced the original `match
  Self::health_check(..) { Ok => return Ok(()) , Err => sleep }` with
  an `if Self::health_check(..).is_ok() { return Ok(()) } ; sleep` to
  satisfy the lint while keeping the loop body readable.

- **No `PluginRuntime { ... }` struct literals exist anywhere in the
  file** outside the type definition. The WO scope_in note about
  updating literals at lines 1188-1205 referred to a `PluginManifest`
  literal in `test_hot_cold_lifecycle`; that does NOT carry the new
  fields and was not modified. Constructor calls (`PluginRuntime::new`)
  pick up the additive fields automatically.

- **Test wall-time budgets honoured**:
  - `test_hot_reload` ~110 ms (100 ms reader hold + ~10 ms reload
    overhead) — well below the 2 s sanity bound.
  - `test_circuit_breaker` ~40 ms (5+10+20 = 35 ms backoff + ~5 ms of
    spawn-ENOENT overhead) — well below the 2 s sanity bound.

- **No global tracing-subscriber installed** in either test (WO-0042
  lesson). Both tests assert state via the public
  `state` / `error_message` / `restart_attempts` / `last_call` fields.

## Files changed

- `crates/ucil-daemon/src/plugin_manager.rs` — additive only.
- `crates/ucil-daemon/src/lib.rs` — extended one `pub use` block by
  two symbols (`MAX_RESTARTS`, `CIRCUIT_BREAKER_BASE_BACKOFF_MS`).

No other crate, no manifest changes, no fixtures touched. Forbidden
paths discipline preserved.
