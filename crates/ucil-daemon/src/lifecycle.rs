//! UCIL daemon process lifecycle: PID file and signal-driven shutdown.
//!
//! The lifecycle module owns two concerns:
//!
//! 1. A best-effort [`PidFile`] that records the running daemon's PID at a
//!    caller-supplied path (typically `.ucil/daemon.pid`). The file is
//!    written once at startup and removed on `Drop`.
//! 2. (Added in a follow-up commit) a shutdown-signal awaiter and a
//!    [`Lifecycle`] convenience handle that owns the PID file and awaits
//!    `SIGTERM` / `SIGHUP`.

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
}
