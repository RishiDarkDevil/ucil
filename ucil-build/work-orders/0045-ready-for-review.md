# WO-0045 — `ucil plugin` CLI subcommand tree — ready for review

**Final commit sha**: `2f89a64b013ff39f904ed8393c09e8394324a6a9`
**Branch**: `feat/WO-0045-ucil-plugin-cli-subcommands`
**Feature**: P2-W6-F07
**Phase**: 2

## Commit ladder (7 commits)

```
2f89a64 feat(verify): implement scripts/verify/P2-W6-F07.sh end-to-end
598f91e test(cli): add commands::plugin::test_<subcommand>_* tests for list/uninstall/enable/disable/reload
01f334e feat(cli): add `plugin reload` subcommand wrapping health_check_with_timeout
11cbe83 feat(cli): add `plugin uninstall|enable|disable` state-mutating subcommands
581e9be feat(cli): add `plugin list` subcommand with --format json
d19e0dc feat(cli): add PluginStateEntry + read/write/mutate state helpers
b01351b feat(daemon): pub use CapabilitiesSection/ActivationSection/ResourcesSection
```

Single test commit (`598f91e`, ~409 LOC) over the 50-line soft target —
covered by DEC-0005 module-coherence (bundled new tests land together).

## What I verified locally

- AC01 — `crates/ucil-cli/src/commands/plugin.rs` exists.
- AC02 — `scripts/verify/P2-W6-F07.sh` is executable.
- AC03 — `crates/ucil-daemon/src/lib.rs` has `pub use plugin_manager::`.
- AC04 — `lib.rs` re-exports include `CapabilitiesSection`, `ActivationSection`, `ResourcesSection` (WO-0042 deferred work landed).
- AC05–AC09 — `PluginSubcommand` enum has six variants: `Install`, `List`, `Uninstall`, `Enable`, `Disable`, `Reload`.
- AC10–AC14 — Five module-root tests present:
  - `test_plugin_list_returns_all_discovered_manifests`
  - `test_plugin_uninstall_marks_state_file`
  - `test_plugin_enable_marks_state_file`
  - `test_plugin_disable_marks_state_file`
  - `test_plugin_reload_runs_health_check`
- AC15 — `cargo build -p ucil-cli` clean.
- AC16 — `cargo build -p ucil-daemon --bin mock-mcp-plugin` clean.
- AC17 — `cargo test -p ucil-cli commands::plugin::` → **15 passed; 0 failed** (5 existing + 5 new module-root + 5 new `mod tests {}` emit-helper coverage).
- AC18 — `commands::plugin::test_plugin_install_resolves_manifest_by_name` regression green.
- AC19 — `cargo test -p ucil-daemon --test plugin_manager` → 3 passed.
- AC20 — `plugin_manager::test_manifest_parser` → 1 passed.
- AC21 — `plugin_manager::test_lifecycle_state_machine` → 1 passed.
- AC22 — `plugin_manager::test_hot_reload` → 1 passed.
- AC23 — `plugin_manager::test_circuit_breaker` → 1 passed.
- AC24 — `cargo test -p ucil-daemon --test plugin_manifests` → 2 passed (ast-grep + probe regression).
- AC25 — `plugin_manager::test_hot_cold_lifecycle` → 1 passed.
- AC26 — `cargo test -p ucil-daemon --test e2e_mcp_stdio` → 1 passed.
- AC27 — `cargo test -p ucil-daemon --test e2e_mcp_with_kg` → 1 passed.
- AC28 — `cargo test --workspace --no-fail-fast` → no `test result: FAILED` lines.
- AC29 — `cargo clippy -p ucil-cli --all-targets -- -D warnings` clean.
- AC30 — `cargo clippy -p ucil-daemon --all-targets -- -D warnings` clean.
- AC31 — `cargo fmt --check` clean.
- AC32 — `bash scripts/verify/P2-W6-F07.sh` → `[OK] P2-W6-F07`.

## Files touched (3)

| Path | LOC delta | Notes |
| ---- | --------- | ----- |
| `crates/ucil-daemon/src/lib.rs` | +3 / -2 | 3-symbol re-export — WO-0042 carry-forward |
| `crates/ucil-cli/src/commands/plugin.rs` | +1045 / -26 | enum extension, args, handlers, emission, helpers, 5 module-root tests, 5 `mod tests {}` unit tests |
| `scripts/verify/P2-W6-F07.sh` | +218 (new) | end-to-end driver against TempDir fixture + 2 manifests pointing at real `mock-mcp-plugin` |

Whole WO touched **three files**. `forbidden_paths` blanket-bans every other crate src and every plugin manifest, so scope is mechanically auditable.

## State-persistence design

- Single TOML file at `<plugins_dir>/.ucil-plugin-state.toml` (recommended path, configurable via `--plugins-dir`).
- Schema (frozen):
  ```toml
  [[plugins]]
  name = "alpha"
  installed = false
  enabled = false
  ```
- All writes go through `tempfile-then-rename` — `tokio::fs::write(<path>.toml.tmp)` followed by `tokio::fs::rename(<path>.toml.tmp, <path>.toml)`. A concurrent `list` either sees the old state or the new state — never a torn read.
- `read_state` returns `Ok(vec![])` when the file is absent so first-mutation flows do not need pre-creation.
- `mutate_state(plugins_dir, name, FnOnce(&mut PluginStateEntry))` is the shared write-helper used by `install`, `uninstall`, `enable`, `disable`, `reload`. Adds a new row when none exists; updates in place otherwise.

## Subcommand → state mutation map

| Subcommand    | Mutation                                  | Probes subprocess? |
|---------------|-------------------------------------------|--------------------|
| `install`     | `installed = true` (only on health Ok)    | yes                |
| `list`        | none — read-only                          | no                 |
| `uninstall`   | `installed = false`                       | no                 |
| `enable`      | `enabled = true`                          | no                 |
| `disable`     | `enabled = false`                         | no                 |
| `reload`      | `installed = true` (only on health Ok)    | yes                |

## JSON output shapes

| Subcommand | Top-level shape |
|------------|-----------------|
| `install`  | `{ name, status: "ok", tools: [...], tool_count }` (unchanged from WO-0013) |
| `list`     | `{ plugins: [{ name, installed, enabled }, ...] }` |
| `uninstall`| `{ name, status: "uninstalled", installed: false, enabled }` |
| `enable`   | `{ name, status: "enabled", installed, enabled: true }` |
| `disable`  | `{ name, status: "disabled", installed, enabled: false }` |
| `reload`   | `{ name, status: "reloaded", tools, tool_count, installed: true, enabled }` |

Every JSON output is a single newline-terminated object/array via `serde_json::to_writer_pretty + writeln!`.

## Mutation-check guidance for verifier

Three handler bodies are pre-baked mutation targets per `acceptance[24..26]`:

1. **`uninstall_plugin`** — replace body with `Ok(StateChangeOutcome { entry: PluginStateEntry::default(), status_label: "uninstalled" })` (or any no-op outcome that **does not call `mutate_state`**).
   Expected failure: `test_plugin_uninstall_marks_state_file` panics at `read_state_entry_blocking(&plugins_fixture, "alpha").expect("state row present after mutation")` because no state file is written; the `.expect` triggers the panic.

2. **`enable_plugin`** — same shape, `status_label: "enabled"`.
   Expected failure: `test_plugin_enable_marks_state_file` panics at the same `.expect` path (state file absent).

3. **`reload_plugin`** — replace body with `Ok(ReloadOutcome { name: args.name.clone(), tools: vec![], tool_count: 0, installed: false, enabled: false })`.
   Expected failure: `test_plugin_reload_runs_health_check` fails at `assert!(tool_count >= 1, ...)`. (And separately the state file would be absent, but the `tool_count` assertion fires first.)

Note on type names: I factored `uninstall`/`enable`/`disable` through a shared `StateChangeOutcome` (each handler differs only by the closure passed to `mutate_state` and the static `status_label`). The WO text mentions `UninstallOutcome::default()` etc., but the WO clarifies "returning a no-op outcome WITHOUT mutating state" — the **spirit** is "no `mutate_state` call". `StateChangeOutcome` does not derive `Default` (it has a `&'static str` field that's free-form by convention), so the verifier's no-op patch needs an explicit literal as shown above. `ReloadOutcome` does derive `Default`, so `Ok(ReloadOutcome::default())` works directly.

Restoration after each mutation: `git checkout -- crates/ucil-cli/src/commands/plugin.rs` (no need to `git stash pop`; the mutation is to a committed file).

## Lessons-learned applied

- **WO-0042 carry-forward (lessons line 82)** — landed `pub use CapabilitiesSection, ActivationSection, ResourcesSection` in `lib.rs`. Single 3-symbol additive edit; no `plugin_manager.rs` source change.
- **WO-0043 carry-forward (lessons line 121)** — `PluginManager::add` tightening to `Result<(), AlreadyRegistered | LockContention>` is OUT-OF-SCOPE here; deferred to a future WO with an ADR. F07's CLI subcommands all operate on disk state + spawn-and-probe; none of them call `PluginManager::add`, so the tightening has no consumer in scope. The deferral is documented in `scope_out`.
- **WO-0044 carry-forward (lessons line 162)** — every state-mutating handler has a pre-baked function-body mutation check named in `acceptance` with the expected-failure assertion line.
- **Frozen-selector placement (DEC-0007)** — five module-root acceptance tests land at the module root (sibling of `test_plugin_install_resolves_manifest_by_name`), NOT inside `mod tests {}`. The five emit-helper unit tests go inside `mod tests {}` matching the existing `emit_text_contains_*` precedent.
- **Backward-compat regression guards** — every prior frozen selector ran green (see AC18–AC27).
- **Single-source-of-edit blast radius** — three files touched. 35-entry `forbidden_paths` blanket-bans every other crate src + every plugin manifest + every other crate `Cargo.toml` so scope is mechanically auditable.
- **Cargo-test summary regex with alternation** — every `acceptance_criteria` entry uses `grep -Eq 'test result: ok\. ... 0 failed|... tests run: ... passed'`.
- **No-mocks-of-critical-deps** — `reload` subcommand spawns the real `mock-mcp-plugin` binary in tests, identical to the existing `install` precedent. State-file persistence tests use real `tempfile::TempDir` + real `tokio::fs::write` / `tokio::fs::rename`.
- **Clippy lint pre-emption** — fixed `clippy::single_match_else` (used `if let Some` shape in `mutate_state`) and `clippy::doc_markdown` (wrapped `TempDir` in backticks). Hit zero `clippy::significant_drop_tightening` since no `RwLock` guards are bound across awaits in the new code.
- **DEC-0005 module-coherence** — single test commit (`598f91e`, ~409 LOC) bundles all 10 new tests and their helpers; over the 50-LOC soft target as authorised by DEC-0005.

## Technical debt carried forward

- **`PluginManager::add` silent contention fall-through** (`plugin_manager.rs:1382-1393`) — still present, still warn-logged. Will need an ADR + tightening when CLI ↔ daemon IPC lands. F07 has no consumer for the tightened return type.
- **Coverage gate workaround** — `RUSTC_WRAPPER`-unset + corrupt-header profraw prune is now in its 8th consecutive WO. Treat as standing protocol until escalation `20260419-0152-monitor-phase1-gate-red-integration-gaps.md` resolves.

## Operator-facing CLI surface

The full master-plan §16 line 1580 set is now usable:

```
ucil plugin list --plugins-dir <dir> [--format text|json]
ucil plugin install <name> --plugins-dir <dir> [--timeout-ms N] [--format ...]
ucil plugin uninstall <name> --plugins-dir <dir> [--format ...]
ucil plugin enable <name> --plugins-dir <dir> [--format ...]
ucil plugin disable <name> --plugins-dir <dir> [--format ...]
ucil plugin reload <name> --plugins-dir <dir> [--timeout-ms N] [--format ...]
```

Ready for critic + verifier.
