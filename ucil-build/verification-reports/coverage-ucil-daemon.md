# Coverage Gate — ucil-daemon

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-04-18T11:06:01Z

`cargo test -p ucil-daemon` failed under coverage instrumentation. Tail of log:

```
test watcher::test_watcher_shutdown_is_clean ... ok
test watcher::test_notify_emits_event_after_debounce ... ok
test watcher::test_hook_event_source_is_distinct ... ok
test plugin_manager::tests::health_check_timeout_fires_when_command_hangs ... ok
test watcher::test_notify_debounces_editor_writes ... ok
test watcher::test_poll_backend_delivers_events ... ok

failures:

---- session_manager::tests::create_session_returns_fresh_uuid_each_call stdout ----

thread 'session_manager::tests::create_session_returns_fresh_uuid_each_call' (1125576) panicked at crates/ucil-daemon/src/session_manager.rs:508:50:
second session: Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- session_manager::tests::detect_branch_returns_non_empty_inside_git_repo stdout ----

thread 'session_manager::tests::detect_branch_returns_non_empty_inside_git_repo' (1125578) panicked at crates/ucil-daemon/src/session_manager.rs:520:14:
detect_branch should succeed inside a git repo: Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })

---- session_manager::tests::discover_worktrees_returns_at_least_one stdout ----

thread 'session_manager::tests::discover_worktrees_returns_at_least_one' (1125579) panicked at crates/ucil-daemon/src/session_manager.rs:541:14:
discover_worktrees should succeed inside a git repo: Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })

---- session_manager::tests::get_session_returns_some_after_create stdout ----

thread 'session_manager::tests::get_session_returns_some_after_create' (1125582) panicked at crates/ucil-daemon/src/session_manager.rs:559:49:
create: Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })


failures:
    session_manager::tests::create_session_returns_fresh_uuid_each_call
    session_manager::tests::detect_branch_returns_non_empty_inside_git_repo
    session_manager::tests::discover_worktrees_returns_at_least_one
    session_manager::tests::get_session_returns_some_after_create

test result: FAILED. 55 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.16s

error: test failed, to rerun pass `-p ucil-daemon --lib`
```
