# WO-0040 — Ready for Review

- **Work-order**: `ucil-build/work-orders/0040-ucil-daemon-mcp-stdio-subcommand.json`
- **Branch**: `feat/WO-0040-ucil-daemon-mcp-stdio-subcommand`
- **Final commit**: `541851390a352c2f1510d8c11d23943f25724b84`
- **Phase**: 1 / Week 3
- **Feature IDs**: none (gate-closure WO)

## What I verified locally (clean worktree at HEAD)

- [x] `cargo build -p ucil-daemon --bin ucil-daemon` — success, `target/debug/ucil-daemon` exists and is executable.
- [x] `scripts/verify/e2e-mcp-smoke.sh` — exits 0; prints `[e2e-mcp-smoke] OK — 22 tools registered, CEQP params on all, daemon spoke MCP cleanly.`
- [x] `cargo test -p ucil-daemon --test e2e_mcp_stdio` — `test result: ok. 1 passed; 0 failed` (the new `e2e_mcp_stdio_handshake_returns_22_tools_with_ceqp` test).
- [x] `cargo test -p ucil-daemon --lib` — `test result: ok. 119 passed; 0 failed` (no regression; WO-0039 DEC-0011-fenced suite still clean).
- [x] `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — clean (no new warnings introduced).
- [x] `cargo doc -p ucil-daemon --no-deps` — generated docs; no `^error` lines and no `^warning: unresolved` lines in stderr.
- [x] `cargo fmt --check` — clean.
- [x] `timeout 3 ./target/debug/ucil-daemon` (no args) — exits cleanly in under 1 s and emits exactly 0 bytes on stdout (backward-compatible with the Phase-0 default-arm behavior).
- [x] Grep checks on source: `"mcp"`, `McpServer::new`, `.serve(`, `with_writer` all present in `crates/ucil-daemon/src/main.rs`; `fn e2e_mcp_stdio_handshake_returns_22_tools_with_ceqp` and `CARGO_BIN_EXE_ucil-daemon` present in `crates/ucil-daemon/tests/e2e_mcp_stdio.rs`.

## Summary of changes

- `crates/ucil-daemon/src/main.rs` — dispatched on `std::env::args().nth(1).as_deref()`. `Some("mcp")` → initialize `tracing_subscriber::fmt().with_writer(std::io::stderr).init()` (stderr-directed so JSON-RPC stdout stays pristine), construct `ucil_daemon::server::McpServer::new()`, call `.serve(tokio::io::stdin(), tokio::io::stdout()).await` with an `anyhow` `.context(...)`. Otherwise falls through to the existing Phase-0 `tracing_subscriber::fmt::init(); tracing::info!(...); Ok(())` path.
- `crates/ucil-daemon/tests/e2e_mcp_stdio.rs` — new black-box integration test that spawns the compiled binary via `env!("CARGO_BIN_EXE_ucil-daemon")`, replays the `initialize` + `tools/list` handshake, and asserts 22 tools + 22/22 CEQP coverage. No `mod tests {}` wrapper (DEC-0005 flat style).
- No other files changed. `Cargo.toml` dev-deps unchanged: `serde_json` is already in `[dependencies]` and integration tests can use it directly.

## Commit ladder

1. `070204a feat(daemon): wire mcp --stdio subcommand into main.rs` (+22 / -6 main.rs)
2. `5418513 test(daemon): e2e integration test — spawn binary, assert 22 tools + CEQP` (+146 new test file)

## Out of scope (explicitly per WO)

- No KG bootstrap in `mcp` mode — stub path only.
- No daemon-mode CLI wiring — non-`mcp` arm preserved byte-for-byte.
- No clap/structopt/argh — `std::env::args().nth(1).as_deref()` is sufficient.
- No edits to `server.rs`, `lib.rs`, or any forbidden path.
- No feature-list flip — all 34 Phase-1 features already `passes=true`; this WO closes the `e2e-mcp-smoke.sh` gate sub-check.
