# WO-0042 — ready for review

**Branch**: `feat/WO-0042-plugin-manifest-and-lifecycle-statemachine`
**Final commit sha**: `5cc34d012ec86d32c749eb966d923e3b77b52697`
**Features**: P2-W6-F01 (manifest parser), P2-W6-F02 (lifecycle state machine)
**Touched files**: `crates/ucil-daemon/src/plugin_manager.rs` (only)

## Commit ladder

1. `2ae808d feat(daemon): add CapabilitiesSection + ResourcesSection to plugin manifest`
2. `c0de35f feat(daemon): add PluginManifest::validate + activates_for helpers`
3. `7c272a4 feat(daemon): add full plugin lifecycle transition methods + tracing`
4. `3415865 test(daemon): add plugin_manager::test_manifest_parser acceptance test`
5. `bdd9709 test(daemon): add plugin_manager::test_lifecycle_state_machine acceptance test`
6. `5cc34d0 refactor(daemon): split test_manifest_parser into helpers`

(Six commits total — five planned plus one follow-up refactor to keep
clippy `too_many_lines` happy on the §14.1-complete fixture test.)

## What I verified locally

All 25 `acceptance_criteria` from the work-order pass:

- [x] `cargo build -p ucil-daemon` clean.
- [x] `cargo test -p ucil-daemon plugin_manager::test_manifest_parser` →
      `1 passed; 0 failed`.
- [x] `cargo test -p ucil-daemon plugin_manager::test_lifecycle_state_machine` →
      `1 passed; 0 failed`.
- [x] `cargo test -p ucil-daemon plugin_manager::test_hot_cold_lifecycle` →
      `1 passed; 0 failed` (P1-W3-F06 regression guard).
- [x] `cargo test -p ucil-daemon --lib` → `121 passed; 0 failed`.
- [x] `cargo test -p ucil-daemon --test plugin_manager` → `3 passed; 0 failed`
      (existing minimal-manifest integration suite — backward-compat
      preserved by `#[serde(default)]` on the new sections).
- [x] `cargo test -p ucil-daemon --test e2e_mcp_stdio` → `1 passed; 0 failed`
      (Phase-1 stub-path regression guard).
- [x] `cargo test -p ucil-daemon --test e2e_mcp_with_kg` → `1 passed; 0 failed`
      (Phase-1 KG-bootstrap regression guard).
- [x] `cargo test --workspace --no-fail-fast` — no `test result: FAILED`
      anywhere (every workspace member green).
- [x] `cargo clippy -p ucil-daemon --all-targets -- -D warnings` clean
      (no `^error` lines).
- [x] `cargo doc -p ucil-daemon --no-deps` — no `^error` and no
      `^warning: unresolved` lines.
- [x] `cargo fmt --check` clean.
- [x] All 13 `grep -q ...` text-presence checks (`fn test_manifest_parser`,
      `fn test_lifecycle_state_machine`, `CapabilitiesSection`,
      `ResourcesSection`, `fn validate`, `fn activates_for_language`,
      `fn activates_for_tool`, `fn register`, `fn mark_loading`,
      `fn mark_active`, `fn stop`, `fn mark_error`, `error_message`,
      `ucil.plugin.lifecycle`).

## Notes for the verifier

### Frozen-selector placement (DEC-0007)

Both new acceptance tests live at the **module root** of
`crates/ucil-daemon/src/plugin_manager.rs` — NOT inside the `mod tests
{ }` wrapper. Their nextest paths are exactly the selectors frozen in
`feature-list.json`:

```
plugin_manager::test_manifest_parser
plugin_manager::test_lifecycle_state_machine
```

Same pattern as the existing `plugin_manager::test_hot_cold_lifecycle`
(P1-W3-F06).

### Backward-compat for existing manifests

`CapabilitiesSection`, `ResourcesSection`, and the `[lifecycle]`
section all carry `#[serde(default)]` on the `PluginManifest` field
declarations, so the existing minimal manifests in
`crates/ucil-daemon/tests/plugin_manager.rs` (which only set `[plugin]`
and `[transport]`) continue to parse without edits. Verified by AC6
(`--test plugin_manager` integration suite green).

### lib.rs gap (cited per scope_out exception)

The work-order's `scope_out` allowed adding explicit `pub use` lines to
`crates/ucil-daemon/src/lib.rs` IF the existing re-export glob did not
cover the new symbols. The actual re-export at lib.rs:105–109 is a
**named list, not a glob** — so the new types
(`CapabilitiesSection`, `ActivationSection`, `ResourcesSection`) are
not re-exported at lib root.

I deliberately did **not** add `pub use` lines because:

1. No consumer in this WO needs them — both new tests live at the
   module root of `plugin_manager.rs` and access the types directly.
2. The existing integration test (`tests/plugin_manager.rs`) only
   imports `HealthStatus`, `PluginError`, `PluginManager`,
   `PluginManifest` — none of which changed.
3. Avoiding `lib.rs` keeps the WO blast radius tight (single source
   file edited).

The new types are still reachable via the fully qualified path
`ucil_daemon::plugin_manager::CapabilitiesSection` etc. for any
downstream consumer that wants them.

### Logging discipline

Every successful state transition emits exactly one `tracing::info!`
event under target `ucil.plugin.lifecycle` (master-plan §15.2
`ucil.<layer>.<op>` naming). `mark_error` uses `tracing::warn!` and
adds the captured error message. No `println!` / `eprintln!` anywhere.
Centralised through a private `log_transition` helper so every method
records the same `plugin` / `from` / `to` field set.

### Mutation checks (verifier's job)

Two mutation checks are pre-baked in `acceptance` so the verifier
doesn't have to invent the mutation patch:

1. **`PluginRuntime::register`** — stash the body so it becomes a no-op
   (`Ok(())` without changing state). Expected outcome:
   `test_lifecycle_state_machine` fails at the first
   `assert_eq!(runtime.state, PluginState::Registered)` after
   `register()`.
2. **`PluginManifest::validate`** — stash the body so it returns
   `Ok(())` unconditionally. Expected outcome: `test_manifest_parser`
   fails at `assert_empty_plugin_name_rejected` (the empty-name
   sad-path block).

### Hot/Cold preservation

The existing `tick` / `mark_call` / `with_idle_timeout` surface and
`PluginManager::activate` / `wake` paths are untouched, so
`test_hot_cold_lifecycle` (P1-W3-F06) still passes. Only the
`PluginRuntime` struct gained `error_message: Option<String>` (default
`None`) and the new transition methods.
