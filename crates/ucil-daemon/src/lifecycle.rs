//! UCIL daemon process lifecycle: PID file and signal-driven shutdown.
//!
//! Two concerns live here:
//!
//! 1. [`PidFile`] — a best-effort guard that records the running daemon's
//!    PID at a caller-supplied path (typically `.ucil/daemon.pid`, per
//!    master-plan §11 line 1064). The file is written once at startup and
//!    removed on `Drop`.
//! 2. [`wait_for_shutdown`] — a Unix-only async helper that resolves with
//!    a [`ShutdownReason`] the first time the process receives `SIGTERM`
//!    or `SIGHUP` (master-plan §18 Phase 1 Week 3 line 1740 — process
//!    lifecycle requirement).
//!
//! A convenience [`Lifecycle`] handle owns the [`PidFile`] and exposes
//! [`Lifecycle::run_until_shutdown`] for call-sites that want to block on
//! the signal directly.
//!
//! # Examples
//!
//! ```no_run
//! use std::path::Path;
//! use ucil_daemon::lifecycle::{Lifecycle, PidFile};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let pid_file = PidFile::write(Path::new(".ucil/daemon.pid"))?;
//! let lifecycle = Lifecycle::new(pid_file);
//! let reason = lifecycle.run_until_shutdown().await?;
//! println!("daemon shutting down: {reason:?}");
//! # Ok(())
//! # }
//! ```

// Public API items share a name prefix with the module ("lifecycle" →
// "Lifecycle"); pedantic clippy would flag that. The convention matches
// `session_manager` / `plugin_manager` in this crate.
#![allow(clippy::module_name_repetitions)]

use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use thiserror::Error;
use tokio::signal::unix::{signal, SignalKind};

/// Errors returned by [`PidFile`] operations.
///
/// `#[non_exhaustive]` so future variants (e.g. a stale-pid recovery path)
/// can land without a SemVer-breaking change.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PidFileError {
    /// An I/O error occurred while reading or writing the PID file.
    #[error("pid-file io at {path}: {source}", path = path.display())]
    Io {
        /// Path that was being accessed when the error occurred.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// A PID file exists at `path` but refers to a process that is no
    /// longer live. Returned by future liveness-check callers; not
    /// produced by the current [`PidFile::write`]/[`PidFile::read`] paths.
    #[error("stale pid-file at {path}: pid={pid} no longer live", path = path.display())]
    Stale {
        /// Path of the stale PID file.
        path: PathBuf,
        /// PID recorded in the stale file.
        pid: u32,
    },
}

/// Guard that owns a PID file on disk.
///
/// On [`PidFile::write`] the file is created (or truncated) and the
/// current process id is written as decimal ASCII. On [`Drop`] the file
/// is removed on a best-effort basis — errors in `Drop` are silently
/// swallowed so the daemon never panics while shutting down.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use ucil_daemon::lifecycle::PidFile;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let guard = PidFile::write(Path::new(".ucil/daemon.pid"))?;
/// // … daemon runs …
/// drop(guard); // removes the file
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    /// Write the current process id to `path` and return a guard.
    ///
    /// PID-file writes happen exactly once at daemon startup (master-plan
    /// §11 line 1064 specifies the pathname only), so synchronous
    /// `std::fs` is used rather than `tokio::fs`.
    ///
    /// # Errors
    ///
    /// Returns [`PidFileError::Io`] if the file cannot be created, written,
    /// or flushed.
    #[must_use = "dropping the guard without binding it removes the pid file immediately"]
    pub fn write(path: &Path) -> Result<Self, PidFileError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|source| PidFileError::Io {
                path: path.to_path_buf(),
                source,
            })?;

        let pid = std::process::id();
        write!(file, "{pid}").map_err(|source| PidFileError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        file.flush().map_err(|source| PidFileError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// Read the PID recorded at `path` without removing the file.
    ///
    /// The file must contain a single decimal integer, optionally
    /// surrounded by whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`PidFileError::Io`] if the file cannot be opened, read,
    /// or does not contain a parseable unsigned 32-bit integer.
    pub fn read(path: &Path) -> Result<u32, PidFileError> {
        let mut file =
            OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(|source| PidFileError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|source| PidFileError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        contents
            .trim()
            .parse::<u32>()
            .map_err(|e| PidFileError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            })
    }

    /// Path this guard owns.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        // Best-effort removal. Never panic in Drop; the daemon may be
        // shutting down for reasons that leave the filesystem in an
        // unusual state.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Reason a daemon shut down via [`wait_for_shutdown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// The process received `SIGTERM`.
    Sigterm,
    /// The process received `SIGHUP`.
    Sighup,
}

/// Wait for the first SIGTERM or SIGHUP delivered to this process.
///
/// Registers signal handlers for both `SignalKind::terminate()` and
/// `SignalKind::hangup()` and returns the reason corresponding to
/// whichever is received first. No timeout is applied — the whole point
/// is to block until a signal arrives; if the caller wants a shutdown
/// deadline it should wrap the call in [`tokio::time::timeout`].
///
/// # Errors
///
/// Returns [`std::io::Error`] if the process cannot register either
/// signal handler (for example, if the handler limit has been reached).
pub async fn wait_for_shutdown() -> std::io::Result<ShutdownReason> {
    let mut term = signal(SignalKind::terminate())?;
    let mut hup = signal(SignalKind::hangup())?;
    let reason = tokio::select! {
        _ = term.recv() => ShutdownReason::Sigterm,
        _ = hup.recv() => ShutdownReason::Sighup,
    };
    Ok(reason)
}

/// Owns a [`PidFile`] for the lifetime of a running daemon and exposes a
/// convenient [`Lifecycle::run_until_shutdown`] helper.
#[derive(Debug)]
pub struct Lifecycle {
    pid_file: PidFile,
}

impl Lifecycle {
    /// Create a new `Lifecycle` taking ownership of a [`PidFile`].
    #[must_use]
    pub const fn new(pid_file: PidFile) -> Self {
        Self { pid_file }
    }

    /// Path of the managed PID file.
    #[must_use]
    pub fn pid_file_path(&self) -> &Path {
        self.pid_file.path()
    }

    /// Block until the first SIGTERM or SIGHUP arrives.
    ///
    /// # Errors
    ///
    /// Propagates the [`std::io::Error`] returned by [`wait_for_shutdown`]
    /// if signal handlers cannot be registered.
    pub async fn run_until_shutdown(&self) -> std::io::Result<ShutdownReason> {
        wait_for_shutdown().await
    }
}

// ── Tests ────────────────────────────────────────────────────────────────
//
// Tests live at module root (NOT wrapped in `#[cfg(test)] mod tests { }`)
// per DEC-0005: the frozen P1-W3-F01 nextest selector `lifecycle::` is an
// exact path prefix match, and wrapping in `mod tests` would insert a
// `::tests::` segment that breaks selector resolution.

#[cfg(test)]
#[test]
fn test_pid_file_write_creates_file_with_current_pid() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("daemon.pid");
    let guard = PidFile::write(&path).expect("write");
    assert!(path.exists(), "pid file should exist after write");

    let contents = std::fs::read_to_string(&path).expect("read");
    let parsed: u32 = contents.trim().parse().expect("parse pid");
    assert_eq!(parsed, std::process::id(), "pid must match current process");
    drop(guard);
}

#[cfg(test)]
#[test]
fn test_pid_file_read_returns_written_pid() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("daemon.pid");
    let _guard = PidFile::write(&path).expect("write");
    let pid = PidFile::read(&path).expect("read");
    assert_eq!(pid, std::process::id());
}

#[cfg(test)]
#[test]
fn test_pid_file_drop_removes_file() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("daemon.pid");
    {
        let _guard = PidFile::write(&path).expect("write");
        assert!(path.exists(), "file exists while guard is alive");
    }
    assert!(!path.exists(), "file must be removed when guard drops");
}

#[cfg(test)]
#[test]
fn test_pid_file_double_write_is_idempotent_or_errors_cleanly() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("daemon.pid");
    let first = PidFile::write(&path).expect("first write");
    // Second write to the same path must succeed (truncate semantics) and
    // must not panic. The first guard is still live — dropping it after
    // the second write is the cleanup responsibility of the test.
    let second = PidFile::write(&path).expect("second write");
    let pid = PidFile::read(&path).expect("read");
    assert_eq!(pid, std::process::id());
    drop(second);
    // After dropping the second guard the file is gone; dropping the
    // first guard is a best-effort no-op.
    assert!(!path.exists(), "second drop removed the file");
    drop(first);
}

#[cfg(test)]
#[test]
fn test_shutdown_reason_debug_is_readable() {
    assert_eq!(format!("{:?}", ShutdownReason::Sigterm), "Sigterm");
    assert_eq!(format!("{:?}", ShutdownReason::Sighup), "Sighup");
    assert_eq!(ShutdownReason::Sigterm, ShutdownReason::Sigterm);
    assert_ne!(ShutdownReason::Sigterm, ShutdownReason::Sighup);
}

#[cfg(test)]
#[test]
fn test_pid_file_error_io_display() {
    let err = PidFileError::Io {
        path: PathBuf::from("/nope/daemon.pid"),
        source: std::io::Error::from(std::io::ErrorKind::NotFound),
    };
    let msg = format!("{err}");
    assert!(msg.contains("/nope/daemon.pid"), "{msg}");
    assert!(msg.contains("pid-file io"), "{msg}");
}

#[cfg(test)]
#[test]
fn test_pid_file_error_stale_display() {
    let err = PidFileError::Stale {
        path: PathBuf::from("/old/daemon.pid"),
        pid: 4242,
    };
    let msg = format!("{err}");
    assert!(msg.contains("/old/daemon.pid"), "{msg}");
    assert!(msg.contains("4242"), "{msg}");
    assert!(msg.contains("stale"), "{msg}");
}

#[cfg(test)]
#[test]
fn test_pid_file_path_accessor() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("daemon.pid");
    let guard = PidFile::write(&path).expect("write");
    assert_eq!(guard.path(), path.as_path());
}

#[cfg(test)]
#[tokio::test]
async fn test_lifecycle_pid_file_path_exposes_owned_path() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("daemon.pid");
    let pid_file = PidFile::write(&path).expect("write");
    let lifecycle = Lifecycle::new(pid_file);
    assert_eq!(lifecycle.pid_file_path(), path.as_path());
}

#[cfg(test)]
#[test]
fn test_pid_file_read_rejects_non_numeric_contents() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("daemon.pid");
    std::fs::write(&path, "not-a-pid").expect("seed");
    let err = PidFile::read(&path).expect_err("read must fail on garbage");
    // Must surface as PidFileError::Io with an InvalidData kind.
    match err {
        PidFileError::Io { source, .. } => {
            assert_eq!(source.kind(), std::io::ErrorKind::InvalidData);
        }
        PidFileError::Stale { .. } => panic!("unexpected Stale for parse failure"),
    }
}
