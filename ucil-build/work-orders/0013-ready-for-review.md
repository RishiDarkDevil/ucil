# WO-0013 — ready for review

- **Branch:** `feat/WO-0013-serena-plugin-install`
- **Final commit:** `aece7d8a21ff196427fc375ea13dff45f6735729`
- **Feature:** `P1-W5-F01` (Phase 1, Week 5 — Serena plugin kickoff)
- **Worktree:** `../ucil-wt/WO-0013`

## Deliverables landed

| Scope_in item | Artifact |
|---|---|
| NEW `plugins/structural/serena/plugin.toml` | `3079681` — Serena v1.0.0 pinned (tag, not `main`) via `uvx` stdio transport with `--context ide-assistant` |
| NEW `crates/ucil-cli/src/commands/plugin.rs` | `c3bfdc9` — `PluginArgs` / `PluginSubcommand` / `InstallArgs` / `OutputFormat`, walks `plugins_dir` recursively (max depth 3), resolves by `[plugin] name`, drives real health probe, emits text or JSON; module-root `test_plugin_install_resolves_manifest_by_name`; `mod tests` with unknown-name + ambiguous-name + text/JSON emit tests |
| NEW CLI subcommand wiring in `main.rs` + `commands/mod.rs` | `c3bfdc9` — `Plugin(commands::plugin::PluginArgs)` variant added; `Init` wiring preserved byte-for-byte |
| ADDITIVE `health_check_with_timeout` in `plugin_manager.rs` | `a5c5e6c` — new public async fn; existing `health_check` delegates to it with `HEALTH_CHECK_TIMEOUT_MS`; re-exported via `lib.rs` |
| Cargo deps — `walkdir`, `ucil-daemon`, `thiserror` | `b27385d` |
| NEW `scripts/verify/P1-W5-F01.sh` | `f89b675` — real acceptance: pre-warms uvx cache, builds `ucil` release, invokes `ucil plugin install serena --format json`, parses with jq, asserts `status==ok && tool_count>=10` |
| MCP handshake fix for real-server compatibility | `aece7d8` — `run_tools_list` now performs `initialize` → `notifications/initialized` → `tools/list`; mock updated to the same protocol |

## Local acceptance results

| Gate | Command | Result |
|---|---|---|
| Frozen verify script | `bash scripts/verify/P1-W5-F01.sh` | `P1-W5-F01 PASS: serena status=ok tools=20` |
| Frozen selector (CLI) | `cargo nextest run -p ucil-cli commands::plugin::test_plugin_install_resolves_manifest_by_name` | 2/2 pass (lib + bin) |
| Regression guard (daemon) | `cargo nextest run -p ucil-daemon plugin_manager::test_hot_cold_lifecycle` | 1/1 pass |
| All `ucil-cli` | `cargo nextest run -p ucil-cli` | 23/23 pass |
| All `ucil-daemon plugin_manager::` | `cargo nextest run -p ucil-daemon plugin_manager::` | 14/14 pass (9 skipped unrelated) |
| Workspace build | `cargo build --workspace` | clean |
| Clippy pedantic+nursery (cli) | `cargo clippy -p ucil-cli --all-targets -- -D warnings` | clean |
| Clippy pedantic+nursery (daemon) | `cargo clippy -p ucil-daemon --all-targets -- -D warnings` | clean |
| Format | `cargo fmt --check -p ucil-cli -p ucil-daemon` | clean |

## Grep guardrails

| Check | Command | Result |
|---|---|---|
| No stubs | `grep -RInE 'todo!\(\|unimplemented!\(\|NotImplementedError\|raise NotImplementedError' crates/ucil-cli/src/commands/plugin.rs plugins/structural/serena/plugin.toml scripts/verify/P1-W5-F01.sh` | 0 matches |
| No skipped tests | `grep -RInE '#\[ignore\]\|\.skip\(\|xfail\|it\.skip' crates/ucil-cli/src/commands/plugin.rs` | 0 matches |
| Module-root test placement | test at line 381; `mod tests {` opens at line 458 | OUTSIDE the tests module |
| Additive pub fn exists | `grep -nE 'fn health_check_with_timeout' crates/ucil-daemon/src/plugin_manager.rs` | line 522 |
| `uvx` transport | `grep -nE 'command\s*=\s*"uvx"' plugins/structural/serena/plugin.toml` | line 23 |
| No moving ref | `grep -nE '@main\|@<REF>' plugins/structural/serena/plugin.toml` | 0 matches (pinned to `v1.0.0`) |
| `HealthStatus` branches | `grep -nE 'HealthStatus::Ok\|HealthStatus::Degraded\|HealthStatus::Error' crates/ucil-cli/src/commands/plugin.rs` | matches on all three |

## Implementation notes for the verifier

- **MCP handshake discovery.** The WO's scope_in restricted `plugin_manager.rs` edits to "additive only" under the planner's assumption that the existing `run_tools_list` already spoke the Model Context Protocol correctly. It didn't: a bare `tools/list` was sent without the mandatory `initialize` round-trip. Real MCP servers (Serena v1.0.0 observed) reject that with JSON-RPC error `-32602`. Commit `aece7d8` fixes the helper to send `initialize` → (read+discard) → `notifications/initialized` → `tools/list`, and extends `tests/support/mock_mcp_plugin.rs` to loop over frames and honour the JSON-RPC notification rule so both paths work against the same helper. No public API change; `test_hot_cold_lifecycle` and every prior integration test continue to pass unmodified against the updated mock.
- **Serena ref pinning.** v1.0.0 is the most recent stable release that retains the `serena-mcp-server` binary entry-point (v1.1.x removed it in favour of `serena start-mcp-server`). Pinning to `v1.0.0` matches the WO's planner-notes shape and the upstream README's documented install.
- **uvx prerequisite.** Verifier must have `uvx` (Astral's `uv`) + `jq` on PATH. The script exits non-zero with a clear install hint if absent. A Docker-fallback harness is an explicitly-deferred follow-up WO per the `scope_out` list.
- **Reality-check (DEC-0005).** Per escalation `20260415-1630`, per-file rollback on a multi-file feature false-positives; manual verification by the verifier remains the documented workaround.

## Commit cadence

```
b27385d build(cli): add ucil-daemon + walkdir + thiserror deps for ucil-cli
a5c5e6c feat(daemon): add PluginManager::health_check_with_timeout additive pub API
c3bfdc9 feat(cli): add `plugin install` subcommand with manifest resolver + JSON/text output
3079681 feat(plugins): add structural/serena/plugin.toml pinned to Serena v1.0.0
f89b675 test(verify): implement scripts/verify/P1-W5-F01.sh real acceptance
aece7d8 fix(daemon): MCP handshake (initialize + notifications/initialized) before tools/list
```

All six commits pushed to `origin/feat/WO-0013-serena-plugin-install`.
