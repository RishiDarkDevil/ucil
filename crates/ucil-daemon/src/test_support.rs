//! Cross-module test utilities for `ucil-daemon`.
//!
//! The only resident so far is a process-wide `PATH` mutex. Any test
//! that either mutates `PATH` (see `watcher::tests`) or depends on a
//! sane inherited `PATH` to spawn a subprocess (see
//! `session_manager::tests` spawning `git`) must acquire the guard via
//! [`env_guard()`] or [`PathRestoreGuard::new()`] so the two classes
//! of tests never interleave their critical sections.
//!
//! Background: `cargo test` (the path `cargo llvm-cov` drives in
//! `scripts/verify/coverage-gate.sh`) runs each `#[test]` function on
//! a dedicated thread of a single process. `std::env::set_var` /
//! `remove_var` mutate that one shared process env. A `watcher` test
//! that blanks `PATH` can therefore race with a `session_manager`
//! test whose `tokio::process::Command::new("git")` performs a PATH
//! lookup at spawn time. The test_support module gives both sides a
//! single rendezvous mutex.
//!
//! Cargo `nextest`, by contrast, spawns one process per test function
//! so the env-var mutation is per-test-process isolated there — the
//! bug WO-0027 tripped over was only visible under `cargo test` /
//! `cargo llvm-cov`. See DEC-0011 for the full decision record.

#[cfg(test)]
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Process-wide `PATH` guard. Crate-scoped so every test module in
/// `ucil-daemon` references the same `Mutex<()>`; do NOT shadow this
/// with a module-local static.
#[cfg(test)]
pub(crate) static ENV_GUARD: Mutex<()> = Mutex::new(());

/// Acquire the `PATH` guard without touching the environment. Use
/// this in tests that only *read* `PATH` (e.g. spawning `git` via
/// inherited env) and need to fence concurrent mutators.
///
/// Poisoning maps to the inner guard — a previous panic while a test
/// held the lock still leaves the caller holding a usable
/// `MutexGuard` here; the caller may still find the env in a
/// questionable state, but that's a separate failure mode the
/// surrounding test will surface via its own assertions.
#[cfg(test)]
pub(crate) fn env_guard() -> MutexGuard<'static, ()> {
    ENV_GUARD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// RAII guard that captures `PATH` on construction, holds
/// [`ENV_GUARD`] for its entire lifetime, and restores `PATH` (or
/// clears it if it was originally unset) on drop. Use in tests that
/// mutate `PATH`.
#[cfg(test)]
pub(crate) struct PathRestoreGuard {
    original: Option<std::ffi::OsString>,
    _lock: MutexGuard<'static, ()>,
}

#[cfg(test)]
impl PathRestoreGuard {
    pub(crate) fn new() -> Self {
        let lock = env_guard();
        let original = std::env::var_os("PATH");
        Self {
            original,
            _lock: lock,
        }
    }
}

#[cfg(test)]
impl Drop for PathRestoreGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var("PATH", value);
        } else {
            std::env::remove_var("PATH");
        }
    }
}
