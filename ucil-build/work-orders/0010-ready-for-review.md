---
wo_id: WO-0010
slug: plugin-lifecycle-and-mcp-server
branch: feat/WO-0010-plugin-lifecycle-and-mcp-server
final_commit: 98b755e8f53692d151d714b58b21180f0fbd4149
features:
  - P1-W3-F06
  - P1-W3-F07
---

# WO-0010 — ready for review

## Final commit
`98b755e8f53692d151d714b58b21180f0fbd4149` on branch
`feat/WO-0010-plugin-lifecycle-and-mcp-server` (pushed to `origin`).

## Commits on this branch (newest first)

| sha       | message                                                            |
|-----------|--------------------------------------------------------------------|
| `98b755e` | feat(daemon): MCP server stdio loop with 22 UCIL tool descriptors  |
| `f2dda95` | test(daemon): add module-level `test_hot_cold_lifecycle`           |
| `7cf2df3` | feat(daemon): add HOT/COLD lifecycle types to `plugin_manager`     |

## What was implemented

### P1-W3-F06 — HOT/COLD plugin lifecycle with idle timeout
- `crates/ucil-daemon/src/plugin_manager.rs`
  - New `LifecycleSection` with `hot_cold: bool` and
    `idle_timeout_minutes: Option<u64>` (+ `idle_timeout()` helper
    defaulting to 30 min per §14.2).
  - `LifecycleSection` field added to `PluginManifest`.
  - `PluginState` enum: `Discovered → Registered → Loading → Active →
    Idle → Stopped → Error` with `Display`.
  - `PluginRuntime { manifest, state, last_call, idle_timeout }` with:
    - `new` / `with_idle_timeout` constructors
    - `mark_call(now)` — refreshes the last-call timestamp and pulls
      the runtime back from `Idle` to `Loading` (on-demand restart
      signal).
    - `tick(now) -> Option<PluginState>` — the idle sweep; flips
      `Active -> Idle` once `now - last_call >= idle_timeout`.
  - `PluginManager` is now `Arc<RwLock<Vec<PluginRuntime>>>`-backed
    (was zero-sized):
    - `activate(manifest)` — drives `Registered -> Loading -> Active`
      via the real `health_check()`, records the runtime in the
      shared vector.
    - `wake(runtime)` — drives `Loading -> Active` via real health
      check (no-op if already `Active`, error if already `Stopped` /
      `Error`).
    - `run_idle_monitor(tick_every)` — spawns a background
      `tokio::task` that sweeps all runtimes on a cadence.
    - `registered_runtimes()` — a cheap read snapshot for
      diagnostics.
- Acceptance test `plugin_manager::test_hot_cold_lifecycle` is a
  module-level item (peer of `mod tests`, NOT nested inside it) so
  the frozen `feature-list.json` test selector resolves.  The test
  uses the real `mock-mcp-plugin` binary (resolved via
  `std::env::current_exe`) and drives the full lifecycle:
  `activate -> Active`, short-idle `tick -> Idle`, `mark_call ->
  Loading`, `wake -> Active`.

### P1-W3-F07 — MCP server over stdio
- `crates/ucil-daemon/src/server.rs` (new)
  - Constants: `JSONRPC_VERSION = "2.0"`,
    `MCP_PROTOCOL_VERSION = "2024-11-05"`,
    `READ_TIMEOUT_MS = 10_000`, `WRITE_TIMEOUT_MS = 5_000`,
    `TOOL_COUNT = 22`.
  - `McpError` — `thiserror` enum covering Io, ReadTimeout,
    WriteTimeout, Encode.
  - `ceqp_input_schema()` — JSON Schema for the master-plan §8.2
    CEQP universal params (`reason`, `current_task`,
    `files_in_context`, `token_budget`).
  - `ucil_tools()` — all 22 §3.2 tools in canonical order, each
    `ToolDescriptor` carrying the CEQP schema.
  - `McpServer::serve<R, W>()` — newline-delimited JSON-RPC 2.0
    over any `AsyncRead + AsyncWrite` pair; every read/write is
    wrapped in a named-const `tokio::time::timeout`.
  - Dispatch for `initialize`, `tools/list`, `tools/call`.
  - `tools/call` returns a stub envelope with
    `_meta.not_yet_implemented: true` per §3.2.5 — real JSON,
    no `todo!` / `unimplemented!`.
- Acceptance test `server::test_all_22_tools_registered` is a
  module-level item.  It wires a real `tokio::io::duplex` + `split`
  pair, spawns the serve loop, drives `tools/list` + `tools/call`,
  and asserts:
  - `tools/list` reports exactly 22 tools
  - every §3.2 tool name is present
  - every tool's `inputSchema.properties` carries all four CEQP
    params
  - `tools/call` stub response has `_meta.not_yet_implemented == true`
  The client signals clean EOF via `shutdown()` on its write half
  (required because `tokio::io::split` keeps the duplex alive until
  the write half is explicitly shut down).

### `crates/ucil-daemon/src/lib.rs`
- New `pub mod server;`
- New `pub use server::{ceqp_input_schema, ucil_tools, McpError,
  McpServer, ToolDescriptor, JSONRPC_VERSION, MCP_PROTOCOL_VERSION,
  READ_TIMEOUT_MS, TOOL_COUNT, WRITE_TIMEOUT_MS};`
- New lifecycle re-exports: `LifecycleSection`, `PluginRuntime`,
  `PluginState`, `DEFAULT_IDLE_TIMEOUT_MINUTES`.

## What I verified locally

Commands run from the worktree
`/home/rishidarkdevil/Desktop/ucil-wt/WO-0010/`, all exiting 0:

1. **Acceptance tests (verbatim from the work-order)**
   - `cargo nextest run -p ucil-daemon plugin_manager::test_hot_cold_lifecycle --no-fail-fast`
     → 1 passed / 0 failed
   - `cargo nextest run -p ucil-daemon server::test_all_22_tools_registered --no-fail-fast`
     → 1 passed / 0 failed

2. **Full crate suite**
   - `cargo nextest run -p ucil-daemon --no-fail-fast`
     → 22 passed / 0 failed / 0 skipped (includes 20 pre-existing
     tests plus the 2 new acceptance tests)

3. **Workspace gates**
   - `cargo build --workspace` → exit 0
   - `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
   - `cargo clippy -p ucil-daemon --all-targets -- -D warnings` → exit 0

4. **Anti-laziness greps**
   - `grep -RInE 'todo!\(|unimplemented!\(|NotImplementedError' crates/ucil-daemon/src`
     → no matches
   - `grep -RInE 'std::process::Command' crates/ucil-daemon/src`
     → no matches
   - `grep -c 'mod tests' crates/ucil-daemon/src/server.rs`
     → 1 match — **a comment in the acceptance test explicitly
     documenting that the test is a module-level peer**; no `mod
     tests { }` block wraps the acceptance test.

5. **Forbidden paths untouched**
   - `git diff --name-only main..HEAD` reports only:
     - `crates/ucil-daemon/src/lib.rs`
     - `crates/ucil-daemon/src/plugin_manager.rs`
     - `crates/ucil-daemon/src/server.rs`
   - No edits to `main.rs`, `session_manager.rs`,
     `feature-list.json`, `tests/fixtures/**`, or `scripts/gate/**`.

## No shortcuts taken

- No stubs (`todo!` / `unimplemented!` / `NotImplementedError`).
- No mocks of `tokio::process`, `tokio::io`, `serde_json`, or the
  real subprocess boundary — tests use the real `mock-mcp-plugin`
  binary and real `tokio::io::duplex`.
- No `#[ignore]` / `.skip` / `xfail`.
- No `std::process::Command` in async paths.
- All `.await` on IO wrapped in `tokio::time::timeout` with a named
  const budget.
- `thiserror` in the library; `anyhow` only in binaries.
- `#![deny(warnings)]` + `#![warn(clippy::all, clippy::pedantic,
  clippy::nursery)]` in `lib.rs` — passes `cargo clippy -- -D
  warnings` across the workspace.

Handing off to the critic and verifier.
