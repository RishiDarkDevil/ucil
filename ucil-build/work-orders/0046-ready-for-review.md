---
work_order: WO-0046
feature: P2-W6-F08
branch: feat/WO-0046-plugin-lifecycle-integration-suite
final_commit: e6b7b48af89772cdcacd1b257f13d865a7ebcd59
ready_at: 2026-05-05T03:50:00Z
ready_by: executor
---

# WO-0046 — Ready for review

## Final commit
`e6b7b48af89772cdcacd1b257f13d865a7ebcd59`
(`build(lock): regenerate Cargo.lock for ucil-tests-integration ucil-daemon dep`)

## Branch
`feat/WO-0046-plugin-lifecycle-integration-suite`

## Commit chain (chronological)
1. `3b99574` `build(tests-integration): wire test_plugin_lifecycle [[test]] + ucil-daemon dev-dep`
2. `78d1945` `test(integration): add test_plugin_hot_cold_round_trip end-to-end`
3. `47e7733` `test(integration): add test_plugin_crash_recovery_via_circuit_breaker`
4. `d12cce2` `test(integration): add test_plugin_independent_lifecycle_two_runtimes`
5. `ae15540` `build(verify): add scripts/verify/P2-W6-F08.sh`
6. `e6b7b48` `build(lock): regenerate Cargo.lock for ucil-tests-integration ucil-daemon dep`

(One commit per test per the WO scope_in commit-size guidance + DEC-0005 module-coherence; helpers `mock_mcp_plugin_path` + `healthy_manifest` land in commit 2 with their first caller; `failing_manifest` lands in commit 3 with its first caller.)

## Files changed (4 only — all in the WO allow-list)
```
$ git diff --name-only main..HEAD
Cargo.lock
scripts/verify/P2-W6-F08.sh
tests/integration/Cargo.toml
tests/integration/test_plugin_lifecycle.rs
```
- ZERO files under `crates/*/src/**` (forbidden_paths invariant honoured).
- ZERO files under `plugins/**` (forbidden_paths invariant honoured).
- ZERO files under `tests/fixtures/**` (forbidden_paths invariant honoured).

## What I verified locally

### Existence + grep gates (all pass)
- `test -f tests/integration/test_plugin_lifecycle.rs` ✓
- `test -x scripts/verify/P2-W6-F08.sh` ✓
- `grep -q 'name = "test_plugin_lifecycle"' tests/integration/Cargo.toml` ✓
- `grep -q 'ucil-daemon' tests/integration/Cargo.toml` ✓
- `! grep -q 'mod tests {' tests/integration/test_plugin_lifecycle.rs` ✓ (DEC-0007 module-root)
- `[ "$(grep -cE '^#\[(tokio::)?test\]' tests/integration/test_plugin_lifecycle.rs)" -ge 3 ]` ✓ (exactly 3)
- `grep -q 'Mocking' tests/integration/test_plugin_lifecycle.rs` ✓ (prohibition docstring)
- `grep -q 'mock-mcp-plugin' tests/integration/test_plugin_lifecycle.rs` ✓
- `grep -q '__ucil_test_nonexistent' tests/integration/test_plugin_lifecycle.rs` ✓
- `grep -q 'with_circuit_breaker_base' tests/integration/test_plugin_lifecycle.rs` ✓
- `grep -q 'idle_timeout' tests/integration/test_plugin_lifecycle.rs` ✓
- `! grep -qE 'MockCommand|spawn_mock|mock_command' tests/integration/test_plugin_lifecycle.rs` ✓
- `! grep -qE '#\[ignore\]|#\[cfg\(ignore\)\]|\.skip\(' tests/integration/test_plugin_lifecycle.rs` ✓

### Cargo gates (all pass)
- `cargo build -p ucil-daemon --bin mock-mcp-plugin` → exit 0
- `cargo test --test test_plugin_lifecycle -- --test-threads=1`
  → `test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s`
  - `test_plugin_hot_cold_round_trip ... ok`
  - `test_plugin_crash_recovery_via_circuit_breaker ... ok`
  - `test_plugin_independent_lifecycle_two_runtimes ... ok`
- `cargo clippy -p ucil-tests-integration --all-targets -- -D warnings` → exit 0
- `cargo test --workspace --no-fail-fast` → 342 passed, 0 failed across 35 test suites

### Regression sentinels (all pass)
| Sentinel | Result |
|----------|--------|
| `cargo test -p ucil-daemon plugin_manager::test_hot_cold_lifecycle` | 1 passed |
| `cargo test -p ucil-daemon plugin_manager::test_circuit_breaker` | 1 passed |
| `cargo test -p ucil-daemon plugin_manager::test_lifecycle_state_machine` | 1 passed |
| `cargo test -p ucil-daemon plugin_manager::test_manifest_parser` | 1 passed |
| `cargo test -p ucil-daemon plugin_manager::test_hot_reload` | 1 passed |
| `cargo test -p ucil-daemon --test plugin_manager` | 3 passed |
| `cargo test -p ucil-daemon --test plugin_manifests` | 2 passed |
| `cargo test -p ucil-daemon --test e2e_mcp_stdio` | 1 passed |
| `cargo test -p ucil-daemon --test e2e_mcp_with_kg` | 1 passed |
| `cargo test -p ucil-cli commands::plugin::` | 15 passed |
| `cargo test --test test_lsp_bridge` | 5 passed |
| `bash scripts/verify/P2-W6-F08.sh` | exits 0 with `[OK] P2-W6-F08` banner |

### Pre-baked mutation checks (both confirm tests are load-bearing)

#### Mutation A — `with_idle_timeout` body no-op
The work-order's prescribed sed
`sed -i 's|self.idle_timeout = idle_timeout;|/* mutation: drop assignment */|' crates/ucil-daemon/src/plugin_manager.rs`
produces a **compile-failure cascade** because the daemon's `#![deny(warnings)]` (`crates/ucil-daemon/src/lib.rs:75`) flags the now-unused `mut self` and `idle_timeout` parameter — `cargo test --test test_plugin_lifecycle test_plugin_hot_cold_round_trip` exits non-zero before the assertion fires. This is a strictly stronger failure than the spec's runtime-assertion failure, so the mutation invariant holds: any drop of the assignment breaks the test target.

To confirm the **assertion itself** is load-bearing (not just relying on the cascade), I re-ran with a runtime-only variant that keeps compilation clean:

```diff
-    pub const fn with_idle_timeout(mut self, idle_timeout: Duration) -> Self {
-        self.idle_timeout = idle_timeout;
+    pub const fn with_idle_timeout(self, idle_timeout: Duration) -> Self {
+        let _ = idle_timeout;
         self
     }
```

`cargo test --test test_plugin_lifecycle test_plugin_hot_cold_round_trip` then panics on
**`tests/integration/test_plugin_lifecycle.rs:207`** with:

> `assertion `left == right` failed: tick must demote Active → Idle once the idle window has elapsed (got None); runtime=PluginRuntime { ..., state: Active, ..., idle_timeout: 600s, ... }`
>
> `left: None    right: Some(Idle)`

Note `idle_timeout: 600s` in the runtime debug print — the production 10-min default leaked through, exactly as the WO predicts. Restored cleanly via `git checkout -- crates/ucil-daemon/src/plugin_manager.rs`.

**Verifier-extension hint** (per WO-0045 lessons line 199): if the verifier wants a third symmetric mutation, the cleanest is on `PluginRuntime::tick` body — replacing the inner `self.state = PluginState::Idle; return Some(PluginState::Idle);` with `return None;` makes test #1 fail at the same `Some(Idle)` assertion.

#### Mutation B — `with_circuit_breaker_base` body no-op
Same compile-failure cascade with the work-order's prescribed `sed -i 's|self.circuit_breaker_base = base;|let _ = base;|'` — the daemon's `#![deny(warnings)]` now flags `mut self`. Cargo test exits non-zero before assertions fire.

Runtime-only variant (drop `mut`, keep `let _ = base;`) compiles cleanly:

```diff
-    pub const fn with_circuit_breaker_base(mut self, base: Duration) -> Self {
-        self.circuit_breaker_base = base;
+    pub const fn with_circuit_breaker_base(self, base: Duration) -> Self {
+        let _ = base;
         self
     }
```

`cargo test --test test_plugin_lifecycle test_plugin_crash_recovery_via_circuit_breaker` then panics on **`tests/integration/test_plugin_lifecycle.rs:364`** with:

> `fast-test budget must hold (got 7.003877135s)`

7.0 s = 1 s × {1, 2, 4} of leaked production base — exactly as the WO predicts. The dual-bound elapsed assertion is the regression sentinel. Restored cleanly via `git checkout -- crates/ucil-daemon/src/plugin_manager.rs`.

**Verifier-extension hint**: a third symmetric mutation on `PluginManager::restart_with_backoff` body — replacing the entire `for attempt in 0..MAX_RESTARTS { ... }` loop with `return Ok(());` — makes test #2 fail at the `expect_err("...")` line because the breaker never trips.

### Mutation cleanup (per WO-0044 lessons line 178 c–d)
- Mutation revert via `git checkout -- crates/ucil-daemon/src/plugin_manager.rs` (NOT `git stash pop`).
- `git status --short` reports a clean working tree between mutation steps and at the end of verification.
- Restored file is byte-identical to HEAD.
- All 3 tests pass again post-restoration.

## Implementation notes for the verifier

### Path resolution
`mock_mcp_plugin_path()` anchors via `env!("CARGO_MANIFEST_DIR")` (which compiles to `tests/integration/`) and joins `../../target/<profile>/mock-mcp-plugin`. Profile is chosen from `cfg!(debug_assertions)`. `CARGO_TARGET_DIR` is honoured when set so the existing shared-build-cache pattern from `.cargo/config.toml` (set up by `scripts/setup-build-cache.sh`) keeps working.

### Manifest construction
Both helpers construct `PluginManifest` as a struct literal (`PluginManifest { plugin: PluginSection { ... }, transport: TransportSection { ... }, capabilities: CapabilitiesSection::default(), resources: None, lifecycle: None }`) per the WO scope_out rule "any test manifest is constructed in-memory inside the test fn — NOT loaded from disk". `plugins/**/plugin.toml` is untouched.

### Why the local runtime is unaffected by `restart_with_backoff` in test #1
`PluginManager::activate` returns an OWNED clone of the runtime; the manager keeps a parallel clone behind its `Arc<RwLock<Vec<PluginRuntime>>>`. `restart_with_backoff` mutates the manager-internal copy only. The local runtime in test #1 stays in `PluginState::Idle` after the restart returns — but the `mgr.registered_runtimes()` snapshot shows `Active`, which is what the test asserts. This matches the existing `test_circuit_breaker` precedent (line 1777 of `crates/ucil-daemon/src/plugin_manager.rs`).

### Test commit-size split
Per the WO scope_in: "One commit per test is acceptable; one combined test commit is acceptable up to the 200-LOC threshold per DEC-0005 module-coherence." The combined file is ~320 LOC of code (~490 LOC including doc comments), so I split into one commit per test. Helpers (`mock_mcp_plugin_path` + `healthy_manifest`) land with their first caller in commit 2 to avoid the dead-code intermediate state that DEC-0005 flagged as deceptive. `failing_manifest` lands with its first caller in commit 3.

## Known follow-ups (out of scope for this WO)
- The carried-forward `PluginManager::add` return-type tightening (Result<(), AlreadyRegistered | LockContention>) from WO-0043 lessons line 139 is still deferred. F08 doesn't need it.
- The doc-rot hint from WO-0045 lessons line 230 is still pending docs-writer Phase-2 close-out.

## Lessons learned
1. The exact pre-baked mutation patterns from WO-0042 lessons (verbatim sed commands) interact with `#![deny(warnings)]` on the production library: dropping an assignment without also dropping `mut self` produces a compile-failure cascade rather than the spec's runtime-assertion failure. Both modes prove the test target catches the mutation, but a verifier expecting the runtime-assertion shape should be ready to apply the runtime-only variant (`fn name(self, ...)` + `let _ = arg;`) shown above. Both variants are now documented in the verification report.
2. `restart_with_backoff` mutates the manager-internal runtime list, NOT the caller's local clone — test #1's final assertion reads `mgr.registered_runtimes()` rather than the local `runtime` variable. This matches the existing `test_circuit_breaker` precedent and is documented inline in the test body.
3. With `with_idle_timeout` as a builder rather than a direct field assignment, the test surface is the builder body — exactly what mutation A targets. The WO-0046 design choice of "use the builder, NOT direct field assignment" is what makes mutation A meaningful.

— executor
