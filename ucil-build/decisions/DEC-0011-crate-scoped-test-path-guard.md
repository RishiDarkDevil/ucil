---
id: DEC-0011
title: Crate-scoped test_support::ENV_GUARD for PATH-mutating × subprocess-spawning tests
date: 2026-04-19
status: accepted
work_order: WO-0039
features: [P1-W3-F03]
raised_by: planner
supersedes: none
related: [WO-0027-rejections, 036e9cf]
commits_cited: [036e9cf, 592c908, de5039d, 42aba9d]
---

# DEC-0011: Crate-scoped `test_support::ENV_GUARD` for PATH-mutating × subprocess-spawning tests

## Context

WO-0027 shipped the P1-W3-F03 Watchman-detection / backend-selection
surface onto `crates/ucil-daemon/src/watcher.rs`. Every acceptance criterion
under `cargo nextest run` passed (nextest spawns one process per test, so
env-var mutations are per-test-process isolated). The verifier nonetheless
rejected WO-0027 three consecutive times:

- `592c908` (reject #1), `de5039d` (retry-2), `42aba9d` (retry-3) — all
  citing the same PATH-mutation race.

Root cause: the coverage gate in `scripts/verify/coverage-gate.sh` invokes
`cargo llvm-cov`, which drives plain `cargo test`. Under `cargo test`, every
`#[test]` / `#[tokio::test]` function in a given crate runs on a dedicated
thread of a single test binary (one process, many threads). The watcher
tests call `std::env::set_var("PATH", <tempdir with fake shim>)` /
`remove_var("PATH")` to exercise `detect_watchman()`. In the same test
process, `session_manager::tests::detect_branch_*` /
`discover_worktrees_*` / `create_session_*` spawn `git` via
`tokio::process::Command::new("git")` which performs a `PATH` lookup at
spawn time. When the two classes of tests interleave, the git spawns land
inside the watcher-test's blanked-`PATH` window and fail with
`No such file or directory (os error 2)` for the `git` executable — which
surfaces as cascade failures in 5/5 session_manager tests under the
coverage harness.

The fix (landed at `036e9cf` on `feat/WO-0027-watchman-detection-and-backend-selection`,
user-authorized) promotes the watcher's module-local `ENV_GUARD: Mutex<()>`
to a new crate-scoped `test_support` module (compiled only under
`#[cfg(test)]`). Both test classes acquire the same lock: watcher tests via
a `PathRestoreGuard` RAII wrapper that also captures+restores `PATH`,
session_manager tests via a bare `env_guard()` call for the duration of
the git-spawning operation. `cargo test -p ucil-daemon --lib` went from
5/5 failing to 59/0 passing.

The fix is on the feat branch but was never merged — the branch is now 168
commits behind main. WO-0039 re-lands the watchman surface on top of
current main with `test_support` baked in from the start so the race
cannot recur.

## Decision

Establish the following pattern for `ucil-daemon` and any future UCIL
crate whose test suite contains both (a) tests that mutate
process-global state via `std::env::set_var` / `remove_var` and (b)
tests that spawn subprocesses whose path-resolution depends on that
state:

1. Create a `src/test_support.rs` module under `#[cfg(test)] mod
   test_support;` (or `#[cfg(test)] pub(crate) mod test_support;` if
   inner items need other-module visibility).
2. Export a crate-scoped `static ENV_GUARD: Mutex<()> = Mutex::new(())`.
3. Expose two access points:
   - `env_guard() -> MutexGuard<'static, ()>` — bare lock acquisition for
     tests that only *read* the state (e.g. inherit `PATH` when spawning
     a subprocess). Poison maps to the inner guard via
     `.unwrap_or_else(PoisonError::into_inner)` so a prior panicking test
     does not permanently poison the entire suite.
   - `PathRestoreGuard::new() -> PathRestoreGuard` — RAII wrapper that
     holds `env_guard()` for its lifetime, snapshots `PATH` on
     construction, and restores (or clears) it on `Drop`. Use in tests
     that *mutate* `PATH`.
4. Every test that mutates `PATH` holds a `PathRestoreGuard` for its
   entire critical section.
5. Every test that spawns a subprocess whose path-lookup depends on
   `PATH` holds a bare `env_guard()` at least across the subprocess
   call.
6. Keep all test_support items `pub(crate)` — they are a test-only
   internal API, not part of the crate's public surface. The module
   itself is `#[cfg(test)]`.

## Rationale

- Nextest's per-test-process isolation masked the bug locally; the
  coverage gate's `cargo test` path is the canonical CI harness.
  Making it pass under both runners is required by the phase gate
  formula (§ CLAUDE.md gate criteria 1 + 2).
- Moving the mutex to crate scope is strictly additive: watcher tests
  retain exactly the same save-restore semantics; session_manager
  runtime code is untouched; only session_manager's test functions
  gain a single `let _g = crate::test_support::env_guard();` line.
- The pattern generalises: if future WOs introduce more
  `std::env::set_var`-based tests, they reuse `test_support::ENV_GUARD`
  rather than creating module-local mutexes that re-introduce this
  class of race.
- Crate-scoping avoids the over-broad workspace-wide test_support crate
  that was considered and rejected: ucil-daemon is the only crate with
  this specific interaction at the Phase 1 scope, and inter-crate
  coupling through a shared test-support crate would complicate
  `cargo nextest run --workspace` dependency-graph parallelism for no
  gain.

## Consequences

- WO-0039 scope-in EXPANDS vs WO-0027: now permits tests-only edits to
  `crates/ucil-daemon/src/session_manager.rs` to acquire `env_guard()`
  in the 4 git-spawning test functions (`detect_branch_*`,
  `discover_worktrees_returns_at_least_one`,
  `get_session_returns_some_after_create`, `create_session_returns_fresh_uuid_each_call`).
  RUNTIME paths (`SessionManager::create_session`,
  `SessionManager::detect_branch`, `SessionManager::discover_worktrees`)
  remain byte-identical.
- WO-0039 scope-in ADDS a new file
  `crates/ucil-daemon/src/test_support.rs` and registers it as
  `#[cfg(test)] mod test_support;` in `crates/ucil-daemon/src/lib.rs`.
- The acceptance_criteria now REQUIRE `cargo test -p ucil-daemon --lib`
  (not just `cargo nextest run`) to exit 0 with the new tests — this
  is the single gate most likely to surface any residual race.
- No runtime behavior change. No production API surface change in
  session_manager. Coverage numbers may shift by a handful of lines
  (test_support module adds ~75 lines of #[cfg(test)] code that is
  trivially covered by any test that exercises the mutex).

## Pattern recognition

If a similar race recurs in another crate (likely candidates: future
`ucil-cli`, `ucil-agents` if they grow env-var tests), apply the same
pattern: a crate-local `test_support` module with `ENV_GUARD` +
RAII wrappers. If the pattern crosses crate boundaries (e.g. two
separate crates' tests need to fence against each other), revisit with
an ADR proposing a `ucil-test-support` workspace crate.

## Revisit trigger

- If `cargo nextest run -p ucil-daemon` starts taking >10× wall-clock
  longer than its pre-WO-0039 median, the crate-scoped mutex is
  over-serialising — introduce a finer-grained lock (e.g. path-only
  vs generic-env) or reduce the number of PATH-mutating tests.
- If `test_support` grows beyond ~150 lines or gains >3 distinct
  guards, split it into `src/test_support/mod.rs` with submodules
  (`env.rs`, `fs.rs`, etc.) and revisit ADR.
