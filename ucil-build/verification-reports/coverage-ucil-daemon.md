# Coverage Gate — ucil-daemon

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-04-18T09:40:36Z

`cargo llvm-cov` failed. Tail of log:

```
test session_ttl::test_compute_expires_at_zero_ttl_yields_created_at ... ok
test session_ttl::test_default_ttl_is_one_hour ... ok
test plugin_manager::test_hot_cold_lifecycle ... ok
test session_manager::tests::get_session_returns_some_after_create ... ok
test session_manager::tests::detect_branch_returns_non_empty_inside_git_repo ... ok
test session_manager::tests::discover_worktrees_returns_at_least_one ... ok
test watcher::test_post_tool_use_hook_bypasses_debounce ... ok
test watcher::test_watcher_shutdown_is_clean ... ok
test watcher::test_notify_emits_event_after_debounce ... ok
test watcher::test_hook_event_source_is_distinct ... ok
test plugin_manager::tests::health_check_timeout_fires_when_command_hangs ... ok
test watcher::test_notify_debounces_editor_writes ... ok

test result: ok. 52 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s

     Running unittests tests/support/mock_mcp_plugin.rs (target/llvm-cov-target/debug/deps/mock_mcp_plugin-14035ee40a14d6c3)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/main.rs (target/llvm-cov-target/debug/deps/ucil_daemon-8f29739e5e6430fa)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/plugin_manager.rs (target/llvm-cov-target/debug/deps/plugin_manager-6d63294ce2fb2e53)

running 3 tests
test plugin_manager::spawn_fails_cleanly_when_command_is_missing ... ok
test plugin_manager::discover_finds_mock_manifest_and_health_check_succeeds ... ok
test plugin_manager::spawn_and_health_check_returns_mock_tools ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

warning: /home/rishidarkdevil/Desktop/ucil-wt/WO-0026/target/llvm-cov-target/WO-0026-733602-557569589502809164_0.profraw: invalid instrumentation profile data (file header is corrupt)
warning: /home/rishidarkdevil/Desktop/ucil-wt/WO-0026/target/llvm-cov-target/WO-0026-733603-557569589502809164_0.profraw: invalid instrumentation profile data (file header is corrupt)
error: no profile can be merged
error: failed to merge profile data: process didn't exit successfully: `/home/rishidarkdevil/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-profdata merge -sparse -f /home/rishidarkdevil/Desktop/ucil-wt/WO-0026/target/llvm-cov-target/WO-0026-profraw-list -o /home/rishidarkdevil/Desktop/ucil-wt/WO-0026/target/llvm-cov-target/WO-0026.profdata` (exit status: 1)
```
