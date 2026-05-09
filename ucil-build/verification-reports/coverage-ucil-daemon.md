# Coverage Gate — ucil-daemon

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-09T10:35:41Z

`cargo test -p ucil-daemon` failed under coverage instrumentation. Tail of log:

```
test crates/ucil-daemon/src/session_ttl.rs - session_ttl::is_expired (line 66) ... ok
test crates/ucil-daemon/src/g4.rs - g4::merge_g4_dependency_union (line 666) ... ok
test crates/ucil-daemon/src/priority_queue.rs - priority_queue (line 35) ... ok
test crates/ucil-daemon/src/g8.rs - g8::merge_g8_test_discoveries (line 587) ... ok
test crates/ucil-daemon/src/g7.rs - g7::merge_g7_by_severity (line 599) ... ok
test crates/ucil-daemon/src/g3.rs - g3::merge_g3_by_entity (line 560) ... ok
test crates/ucil-daemon/src/g5.rs - g5::assemble_g5_context (line 581) ... ok
test crates/ucil-daemon/src/branch_manager.rs - branch_manager::BranchManager::branch_vectors_dir (line 275) ... ok
test crates/ucil-daemon/src/branch_manager.rs - branch_manager::BranchManager::new (line 232) ... ok

failures:

---- crates/ucil-daemon/src/g5.rs - g5::execute_g5 (line 459) stdout ----
error[E0308]: mismatched types
   --> crates/ucil-daemon/src/g5.rs:472:26
    |
472 | let outcome = execute_g5(&sources, &q, G5_MASTER_DEADLINE).await;
    |               ---------- ^^^^^^^^ expected `&[Box<dyn G5Source>]`, found `&Vec<Box<dyn G5Source + Send + Sync>>`
    |               |
    |               arguments to this function are incorrect
    |
    = note: expected reference `&[Box<(dyn G5Source + 'static)>]`
               found reference `&Vec<Box<dyn G5Source + Send + Sync>>`
note: function defined here
   --> crates/ucil-daemon/src/g5.rs:476:14
    |
476 | pub async fn execute_g5(
    |              ^^^^^^^^^^

error: aborting due to 1 previous error

For more information about this error, try `rustc --explain E0308`.
Couldn't compile the test.

failures:
    crates/ucil-daemon/src/g5.rs - g5::execute_g5 (line 459)

test result: FAILED. 32 passed; 1 failed; 3 ignored; 0 measured; 0 filtered out; finished in 3.22s

error: doctest failed, to rerun pass `-p ucil-daemon --doc`
```
