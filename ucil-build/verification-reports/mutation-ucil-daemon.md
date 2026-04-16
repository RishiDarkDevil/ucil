# Mutation Gate — ucil-daemon

- **Verdict**: FAIL
- **Min score**: 70%
- **Generated**: 2026-04-16T21:42:04Z

## Summary

| Metric       | Count |
|--------------|-------|
| Caught       | 5 |
| Missed       | 7 |
| Unviable     | 26 |
| Timeout      | 0 |
| Failure      | 0 |
| **Score**    | **41%** (caught / (caught+missed)) |

## Missed mutants

- `crates/ucil-daemon/src/main.rs:10:5: replace main -> Result<()> with Ok(())`
- `crates/ucil-daemon/src/session_manager.rs:189:36: replace + with - in SessionManager::create_session`
- `crates/ucil-daemon/src/session_manager.rs:189:36: replace + with * in SessionManager::create_session`
- `crates/ucil-daemon/src/session_manager.rs:225:19: replace == with != in SessionManager::detect_branch`
- `crates/ucil-daemon/src/session_manager.rs:346:48: replace > with == in SessionManager::purge_expired`
- `crates/ucil-daemon/src/session_manager.rs:346:48: replace > with >= in SessionManager::purge_expired`
- `crates/ucil-daemon/src/session_manager.rs:347:30: replace - with + in SessionManager::purge_expired`

## Raw outcomes

See `mutants.out/outcomes.json` in the crate directory for the full dump.

## Why this is failing

Mutation score 41% is below the floor of 70%. This means one
or more mutations (listed above) survived the test suite — i.e., the tests
passed even when the implementation was broken. Add assertions that would
fail under each listed mutant, then re-run.
