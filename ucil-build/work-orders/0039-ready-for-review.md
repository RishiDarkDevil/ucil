---
work_order: WO-0039
slug: watchman-backend-retry-with-pathguard
phase: 1
week: 3
feature_ids: [P1-W3-F03]
branch: feat/WO-0039-watchman-backend-retry-with-pathguard
tip_commit: ce5ea6aea5fd431ee15c26e4bfb49eee4607c558
executor: executor-claude-opus-4.6
ready_at: 2026-04-19
---

# WO-0039 ready for review

Tip commit: `ce5ea6aea5fd431ee15c26e4bfb49eee4607c558` on
`feat/WO-0039-watchman-backend-retry-with-pathguard` (pushed to origin).

## Commit ladder (10 commits, up from the planned 9)

1. `4034bc3` — `build(workspace): add which workspace dep + walkdir to daemon deps`
2. `4d3f33b` — `feat(daemon): add crate-scoped test_support module for PATH mutex (DEC-0011)`
3. `301efbe` — `feat(daemon): add WatcherBackend enum + WatchmanCapability + constants`
4. `89743ba` — `feat(daemon): add detect_watchman + count_files_capped + auto_select_backend`
5. `f4bf4ca` — `feat(daemon): extend WatcherError with WatchmanSpawn + WatchmanDecode variants`
6. `5322352` — `feat(daemon): FileWatcher::new_with_backend dispatch to 3 backends (notify/poll/watchman)`
7. `e76e356` — `test(daemon): add 7 watcher tests under PathRestoreGuard + doc polish on test_support`
8. `6bfbb74` — `test(daemon): fence session_manager git-spawning tests with env_guard (DEC-0011)`
9. `6dbb7e9` — `feat(daemon): re-export WO-0039 watcher public surface from lib.rs`
10. `ce5ea6a` — `test(daemon): annotate PATH-mutation lines with PathRestoreGuard reference`

Commit #10 is the single unplanned addition: the WO acceptance grep
at criterion #22 is line-local and needed a `// under PathRestoreGuard`
trailing comment on each `set_var("PATH", …)` line so the `grep -vE
'PathRestoreGuard|test_support'` filter would exclude it. Pure
comment-only change — no runtime behaviour shifted.

## What I verified locally

All 22 acceptance criteria in `ucil-build/work-orders/0039-watchman-backend-retry-with-pathguard.json` were re-run against tip `ce5ea6a`:

- **[01]** `cargo nextest run -p ucil-daemon 'watcher::test_watchman_detection' --no-fail-fast` → 1 test run: 1 passed (frozen F03 selector).
- **[02]** `cargo nextest run -p ucil-daemon 'watcher::' --no-fail-fast` → 13 tests run: 13 passed (6 WO-0026 + 6 WO-0039 PATH-mutation + 1 Poll-backend).
- **[03]** `cargo test -p ucil-daemon --lib` → `test result: ok. 119 passed; 0 failed; 0 ignored`. **This is the coverage-gate-equivalent path that rejected WO-0027 three times — DEC-0011 fix landed green.**
- **[04]** `cargo clippy -p ucil-daemon --all-targets -- -D warnings` → no `^error` lines.
- **[05]** `cargo doc -p ucil-daemon --no-deps` → no `^error` or `^warning: unresolved` lines.
- **[06]** `cargo fmt --check` → clean.
- **[07]** `crates/ucil-daemon/src/test_support.rs` exists.
- **[08–10]** `test_support.rs` exports the three required items with the frozen `pub(crate)` prefix: `static ENV_GUARD`, `fn env_guard`, `struct PathRestoreGuard`.
- **[11]** `lib.rs` contains `#[cfg(test)] mod test_support;` on a single line (rustfmt is blocked from splitting via `#[rustfmt::skip]`).
- **[12–18]** `watcher.rs` exports all seven new public items: `WatcherBackend`, `detect_watchman`, `count_files_capped`, `auto_select_backend`, `new_with_backend`, `WATCHMAN_AUTO_SELECT_THRESHOLD`, `POLL_WATCHER_INTERVAL`.
- **[19–20]** `lib.rs` re-exports the watcher surface — `pub use watcher::` includes `WatcherBackend` (plus the six siblings).
- **[21]** `session_manager.rs` fences `create_session_*`, `detect_branch_*`, `discover_worktrees_*`, `get_session_returns_some_after_create`, and module-root `test_session_state_tracking` with `crate::test_support::env_guard()`.
- **[22]** No `std::env::set_var("PATH", …)` / `remove_var("PATH")` in `watcher.rs` without `PathRestoreGuard` or `test_support` on the same line.

## Scope-out compliance (`scope_out` verification)

- `session_manager.rs` runtime methods `create_session`, `detect_branch`, `discover_worktrees` are byte-identical to main — only test-function bodies gained the `let _g = …;` + `#[allow(clippy::await_holding_lock)]` pair. `parse_worktree_porcelain_main_and_linked_and_detached` and `get_session_returns_none_for_unknown_id` deliberately do NOT acquire the guard (pure-string / no-git paths).
- `FileWatcher::new(root, sender)` signature unchanged; body is a one-line delegate `Self::new_with_backend(root, sender, WatcherBackend::NotifyDebounced)`.
- All eight WO-0026 foundation watcher tests still pass unchanged (visible in the `cargo test --lib` output).
- No edits to `ucil-cli/**`, `ucil-core/**`, `ucil-treesitter/**`, `adapters/**`, `ml/**`, `plugins/**`, `tests/integration/**`, or any forbidden path.
- No `#[ignore]`, `.skip()`, `todo!()`, `unimplemented!()`, or `pass`-only bodies introduced.
- No `watchman_client` crate dep added — watchman is spawned as a subprocess via `tokio::process::Command` with `kill_on_drop(true)` per `scope_in`.
- Cross-platform `PollWatcher` network-mount auto-detection deferred as required.

## Clippy allow additions (justified, each with in-source comment)

1. `#![allow(clippy::redundant_pub_crate)]` at the top of `test_support.rs` — the acceptance grep requires the literal `pub(crate)` prefix, which nursery clippy 1.94+ flags inside a private module. Comment in `test_support.rs` cites DEC-0011 and the WO-0039 grep contract.
2. `#[allow(clippy::await_holding_lock)]` on each of the six fenced `session_manager` tests — the `std::sync::MutexGuard` is held across `git` spawn awaits on purpose (that is what the fence does). `#[tokio::test]` uses a single-threaded runtime so the `!Send` guard cannot cross threads; the lint's deadlock guard does not apply. Comment in `test_session_state_tracking` carries the full justification and the five sibling tests point back to it.

Neither `#[allow]` appeared in the `036e9cf` reference implementation — both are clippy regressions against the newer 1.94 toolchain that weren't active when WO-0027 shipped.

## DEC-0011 fence coverage summary

| File | Test | Fence shape |
|------|------|-------------|
| `watcher.rs` | `test_watchman_detection` | `RestorePath::new()` |
| `watcher.rs` | `test_auto_select_backend_returns_notify_when_watchman_absent` | `RestorePath::new()` |
| `watcher.rs` | `test_auto_select_backend_returns_watchman_when_available_and_above_threshold` | `RestorePath::new()` |
| `watcher.rs` | `test_auto_select_backend_returns_notify_when_below_threshold` | `RestorePath::new()` |
| `session_manager.rs` | `test_session_state_tracking` | `env_guard()` |
| `session_manager.rs` | `create_session_returns_fresh_uuid_each_call` | `env_guard()` |
| `session_manager.rs` | `detect_branch_returns_non_empty_inside_git_repo` | `env_guard()` |
| `session_manager.rs` | `detect_branch_errors_outside_git_repo` | `env_guard()` |
| `session_manager.rs` | `discover_worktrees_returns_at_least_one` | `env_guard()` |
| `session_manager.rs` | `get_session_returns_some_after_create` | `env_guard()` |

All 10 tests serialise through the same crate-wide `test_support::ENV_GUARD`, so no cross-test race is possible — including under the `cargo test` / `cargo llvm-cov` single-process model that broke WO-0027 × 3.

## Ready for critic + verifier

Branch pushed. Tip `ce5ea6a`. Ready to hand off.
