//! UCIL daemon process lifecycle: PID file and signal-driven shutdown.
//!
//! The lifecycle module owns two concerns:
//!
//! 1. A best-effort [`PidFile`] that records the running daemon's PID at a
//!    caller-supplied path (typically `.ucil/daemon.pid`). The file is
//!    written once at startup and removed on `Drop`.
//! 2. A cross-platform (Unix) [`wait_for_shutdown`] helper that resolves
//!    with a [`ShutdownReason`] the first time the process receives
//!    `SIGTERM` or `SIGHUP`.
//!
//! A convenience [`Lifecycle`] handle owns the [`PidFile`] and exposes
//! [`Lifecycle::run_until_shutdown`] for call-sites that want to await
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
//! let reason = lifecycle.run_until_shutdown().await;
//! println!("daemon shutting down: {reason:?}");
//! # Ok(())
//! # }
//! ```

// Public API items intentionally share a name prefix with the module
// ("lifecycle" → "Lifecycle", no repetition today but keep parity with
// `session_manager`).
#![allow(clippy::module_name_repetitions)]

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use thiserror::Error;

/// Errors returned by [`PidFile`] operations.
#[derive(Debug, Error)]
pub enum PidFileError {
    /// An I/O error occurred while writing or reading the PID file.
    #[error("pid-file I/O error at {}: {source}", path.display())]
    Io {
        /// Path that was being accessed when the error occurred.
        path: PathBuf,
        /// Underlying [`std::io::Error`].
        #[source]
        source: std::io::Error,
    },
    /// The PID file exists but its contents could not be parsed as a PID.
    #[error("stale pid-file at {}: contents not a valid pid ({pid})", path.display())]
    Stale {
        /// Path of the malformed PID file.
        path: PathBuf,
        /// The (possibly garbage) numeric value parsed from the file.
        pid: u32,
    },
}

/// A best-effort PID file guard.
///
/// On `Drop`, the file at the stored path is removed; failures during
/// removal are intentionally swallowed because `Drop` must not panic.
#[derive(Debug)]
#[must_use = "PidFile removes its file on Drop — bind it to a name or the file is removed immediately"]
pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    /// Write the current process's PID to `path`, truncating any existing
    /// file.
    ///
    /// The file's parent directory must already exist; this function does
    /// not create intermediate directories.
    ///
    /// # Errors
    ///
    /// Returns [`PidFileError::Io`] if the file cannot be opened, truncated,
    /// or written.
    pub fn write(path: &Path) -> Result<Self, PidFileError> {
        let mut file = std::fs::OpenOptions::new()
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

    /// Read a PID from the file at `path`.
    ///
    /// # Errors
    ///
    /// - [`PidFileError::Io`] if the file cannot be opened or read.
    /// - [`PidFileError::Stale`] if the file's contents are not a valid
    ///   non-negative integer.
    pub fn read(path: &Path) -> Result<u32, PidFileError> {
        let raw = std::fs::read_to_string(path).map_err(|source| PidFileError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let trimmed = raw.trim();
        trimmed.parse::<u32>().map_err(|_| PidFileError::Stale {
            path: path.to_path_buf(),
            pid: trimmed.parse::<u32>().unwrap_or(0),
        })
    }

    /// Return the path this guard is managing.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        // Best-effort removal: never panic in Drop, ignore failures (the
        // file may already be gone, or the directory may have been
        // unmounted during shutdown).
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Reason a [`Lifecycle`] returned from its `run_until_shutdown` loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// Received `SIGTERM` — normal shutdown request.
    Sigterm,
    /// Received `SIGHUP` — reload or hang-up shutdown.
    Sighup,
}

/// Await a Unix shutdown signal and return the [`ShutdownReason`] that
/// caused resolution.
///
/// Listens on both `SIGTERM` and `SIGHUP`; whichever arrives first wins.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the underlying signal handler could not
/// be installed (for example, too many signal handlers already
/// registered).
pub async fn wait_for_shutdown() -> std::io::Result<ShutdownReason> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut term = signal(SignalKind::terminate())?;
    let mut hup = signal(SignalKind::hangup())?;

    let reason = tokio::select! {
        _ = term.recv() => ShutdownReason::Sigterm,
        _ = hup.recv() => ShutdownReason::Sighup,
    };
    Ok(reason)
}

/// Convenience handle bundling a [`PidFile`] with a shutdown-signal
/// awaiter.
///
/// Dropping the `Lifecycle` removes the PID file. Callers typically
/// construct it at daemon startup and await [`Self::run_until_shutdown`]
/// at the end of `main`.
#[derive(Debug)]
pub struct Lifecycle {
    _pid_file: PidFile,
}

impl Lifecycle {
    /// Construct a new `Lifecycle` that owns `pid_file`.
    #[must_use]
    pub const fn new(pid_file: PidFile) -> Self {
        Self {
            _pid_file: pid_file,
        }
    }

    /// Await shutdown.
    ///
    /// Returns the [`ShutdownReason`] indicating which signal fired. If
    /// the signal handlers cannot be installed, falls back to
    /// [`ShutdownReason::Sigterm`] after logging a trace message. (The
    /// daemon is effectively un-shuttable in that pathological case, so
    /// we still return *some* reason rather than panic.)
    pub async fn run_until_shutdown(&self) -> ShutdownReason {
        match wait_for_shutdown().await {
            Ok(reason) => reason,
            Err(err) => {
                tracing::error!(?err, "failed to install shutdown signal handlers");
                ShutdownReason::Sigterm
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_file_write_creates_file_with_current_pid() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("daemon.pid");
        let guard = PidFile::write(&path).expect("write pid-file");
        assert!(path.exists(), "pid-file must exist after write");
        let contents = std::fs::read_to_string(&path).expect("read pid-file");
        assert_eq!(
            contents.trim(),
            std::process::id().to_string(),
            "pid-file contents should be the current process id"
        );
        drop(guard);
    }

    #[test]
    fn pid_file_read_returns_written_pid() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("daemon.pid");
        let _guard = PidFile::write(&path).expect("write pid-file");
        let pid = PidFile::read(&path).expect("read pid-file");
        assert_eq!(
            pid,
            std::process::id(),
            "read must return the process id we wrote"
        );
    }

    #[test]
    fn pid_file_drop_removes_file() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("daemon.pid");
        {
            let _guard = PidFile::write(&path).expect("write pid-file");
            assert!(path.exists(), "pid-file must exist inside scope");
        }
        assert!(
            !path.exists(),
            "pid-file must be removed when the guard drops"
        );
    }

    #[test]
    fn pid_file_double_write_is_idempotent_or_errors_cleanly() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("daemon.pid");
        let first = PidFile::write(&path).expect("first write");
        // The second write should truncate and rewrite — not panic, not
        // leave the file in a half-written state.
        let second = PidFile::write(&path).expect("second write");
        let pid = PidFile::read(&path).expect("read back");
        assert_eq!(
            pid,
            std::process::id(),
            "after double-write, the file still contains our pid"
        );
        drop(second);
        drop(first);
    }

    #[test]
    fn pid_file_read_of_garbage_returns_stale_error() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("daemon.pid");
        std::fs::write(&path, "not-a-pid").expect("seed garbage file");
        let err = PidFile::read(&path).expect_err("garbage pid must not parse");
        assert!(
            matches!(err, PidFileError::Stale { .. }),
            "expected Stale variant, got {err:?}"
        );
    }

    #[test]
    fn shutdown_reason_debug_is_readable() {
        assert_eq!(format!("{:?}", ShutdownReason::Sigterm), "Sigterm");
        assert_eq!(format!("{:?}", ShutdownReason::Sighup), "Sighup");
    }

    #[test]
    fn lifecycle_holds_pid_file_and_removes_on_drop() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("daemon.pid");
        {
            let pid_file = PidFile::write(&path).expect("write pid-file");
            let _life = Lifecycle::new(pid_file);
            assert!(path.exists(), "pid-file must exist while Lifecycle is live");
        }
        assert!(
            !path.exists(),
            "Lifecycle dropping the PidFile must remove the file"
        );
    }
}
