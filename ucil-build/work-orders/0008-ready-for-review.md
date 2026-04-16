# WO-0008 — Ready for Review

- **Branch**: `feat/WO-0008-daemon-lifecycle-session-state`
- **Final commit**: `8b7b1fd51aff86069abea97da23b694405950318`
- **Features**: `P1-W3-F01` (process lifecycle), `P1-W4-F07` (session state tracking)

## What I verified locally

All six acceptance criteria pass from a clean build inside
`../ucil-wt/WO-0008`:

1. `cargo nextest run -p ucil-daemon lifecycle::` — **7 tests, 7 passed** (spec asked for ≥5):
   - `lifecycle::tests::pid_file_write_creates_file_with_current_pid`
   - `lifecycle::tests::pid_file_read_returns_written_pid`
   - `lifecycle::tests::pid_file_drop_removes_file`
   - `lifecycle::tests::pid_file_double_write_is_idempotent_or_errors_cleanly`
   - `lifecycle::tests::pid_file_read_of_garbage_returns_stale_error`
   - `lifecycle::tests::shutdown_reason_debug_is_readable`
   - `lifecycle::tests::lifecycle_holds_pid_file_and_removes_on_drop`
2. `cargo nextest run -p ucil-daemon session_manager::test_session_state_tracking` — **exactly 1 test, passing**. The frozen exact-match selector matches the module-level placement.
3. `cargo nextest run -p ucil-daemon session_manager::` — **8 tests, 8 passed**. No regression on P1-W2-F05.
4. `cargo clippy -p ucil-daemon --all-targets -- -D warnings` — clean.
5. `cargo build --workspace` — clean.
6. Module-level placement check: `fn test_session_state_tracking` at line **422**, `mod tests {` at line **491**. 422 < 491 ✅.

## What landed

### `crates/ucil-daemon/src/lifecycle.rs` (new, 332 lines)
- `PidFileError` enum (`Io { path, source }`, `Stale { path, pid }`) via `thiserror`.
- `PidFile` guard — `write(&Path) -> Result<Self, PidFileError>`, `read(&Path) -> Result<u32, PidFileError>`, `path() -> &Path`. `Drop` best-effort removes the file.
- `ShutdownReason { Sigterm, Sighup }`.
- `wait_for_shutdown()` — installs `SIGTERM` and `SIGHUP` via `tokio::signal::unix::signal(...)`, `tokio::select!`s on both `.recv()` futures. Returns `std::io::Result<ShutdownReason>`.
- `Lifecycle` struct owning a `PidFile`, with `new(pid_file)` constructor and `run_until_shutdown()` that wraps `wait_for_shutdown` (falls back to `Sigterm` if handler install fails — the daemon is un-shuttable in that pathological case so returning *some* reason is preferable to a panic).
- Unit tests live under `lifecycle::tests::…` (module-prefix match — the frozen selector is `lifecycle::`).

### `crates/ucil-daemon/src/session_manager.rs` (extended)
- Added `CallRecord { tool, at }` (`Debug + Clone + Serialize + Deserialize`).
- Added `pub const DEFAULT_TTL_SECS: u64 = 3600`.
- Extended `SessionInfo` with four new `#[serde(default)]` fields: `call_history: Vec<CallRecord>`, `inferred_domain: Option<String>`, `files_in_context: BTreeSet<PathBuf>`, `expires_at: u64`.
- Added five new `SessionManager` methods: `record_call`, `add_file_to_context`, `set_inferred_domain`, `set_ttl`, `purge_expired`.
- Added EXACTLY one new `#[cfg(test)] #[tokio::test] async fn test_session_state_tracking` at **module level** (not inside the inner `tests` submodule) so the frozen exact-match selector `session_manager::test_session_state_tracking` matches. The test covers all five scenarios listed in scope_in.

### `crates/ucil-daemon/src/lib.rs` (extended)
- Added `pub mod lifecycle;`.
- Added `pub use lifecycle::{Lifecycle, PidFile, PidFileError, ShutdownReason};`.
- Extended the `session_manager` re-export list with `CallRecord` and `DEFAULT_TTL_SECS`.

## What I deliberately did NOT touch (scope_out)

- `crates/ucil-daemon/src/main.rs` — lifecycle is not wired into the entry point; that's a separate WO.
- `crates/ucil-treesitter/**`, `crates/ucil-daemon/src/storage.rs` — in-flight on other branches.
- `crates/ucil-daemon/src/session_manager.rs` existing `mod tests` block contents — left intact so P1-W2-F05 stays passing.
- `notify`, `salsa`, plugin.toml, SQLite schema — all deferred per scope_out.
- `tests/fixtures/**`, `ucil-build/feature-list.json` — untouched.

## Commit cadence

Six commits, all with Conventional Commits format + `Phase`/`Feature`/`Work-order` trailers. Largest commit is 203 lines (commit 1 introducing `PidFile` + tests — DEC-0005 applies). Each intermediate commit compiles and passes `cargo clippy -- -D warnings`.

```
8b7b1fd chore(daemon): re-export CallRecord and DEFAULT_TTL_SECS from lib.rs
63b50ed test(daemon): add module-level test_session_state_tracking
99e3919 feat(daemon): add SessionManager state methods
18230b0 feat(daemon): add CallRecord, DEFAULT_TTL_SECS, SessionInfo state fields
9badca1 feat(daemon): add ShutdownReason, wait_for_shutdown, and Lifecycle
55159c3 feat(daemon): add lifecycle module skeleton with PidFile guard
```

## Lessons applied

- WO-0007 exact-match selector trap → test is at module level; the placement check in AC6 passes.
- WO-0006 oversized-commit ADR (DEC-0005) → commit 1 is a coherent module-introduction, subsequent commits are ≤115 lines each.
- No `unwrap()` / `expect()` in non-test code — all `Result`-returning public APIs use `?` with `thiserror`-backed variants.
