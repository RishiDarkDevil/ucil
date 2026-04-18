//! UCIL daemon process lifecycle: PID file, signal-driven shutdown, and
//! crash-recovery checkpoint.
//!
//! Three concerns live here:
//!
//! 1. [`PidFile`] — a best-effort guard that records the running daemon's
//!    PID at a caller-supplied path (typically `.ucil/daemon.pid`, per
//!    master-plan §11 line 1064). The file is written once at startup and
//!    removed on `Drop`.
//! 2. [`wait_for_shutdown`] — a Unix-only async helper that resolves with
//!    a [`ShutdownReason`] the first time the process receives `SIGTERM`
//!    or `SIGHUP` (master-plan §18 Phase 1 Week 3 line 1740 — process
//!    lifecycle requirement).
//! 3. [`Checkpoint`] — a serde-backed progress marker persisted at
//!    `.ucil/checkpoint.json`. The daemon writes the last-indexed commit,
//!    active branch, and daemon version on each indexing-phase boundary so
//!    that a restarted daemon can resume indexing without a full
//!    re-index (master-plan §18 Phase 1 Week 3 line 1740 — crash
//!    recovery, feature `P1-W3-F09`).
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

use serde::{Deserialize, Serialize};
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

// ── Checkpoint ───────────────────────────────────────────────────────────
//
// `Checkpoint` persists the daemon's indexing progress to
// `.ucil/checkpoint.json` so that a restarted daemon can resume without a
// full re-index (master-plan §11.2 line 1076 names the per-branch
// `state.json`; the daemon-wide progress marker lives at the `.ucil/`
// root per §18 Phase 1 Week 3 line 1740). The file is single-writer —
// only the owning daemon writes to it — so we don't need the atomic
// rename dance; a plain `OpenOptions::create().truncate().write()` +
// `serde_json::to_writer_pretty` + `flush()` is sufficient. If
// concurrency concerns surface later, raise an ADR.

/// Errors returned by [`Checkpoint`] operations.
///
/// `#[non_exhaustive]` so future variants (e.g. a schema-version
/// mismatch) can land without a SemVer-breaking change.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CheckpointError {
    /// An I/O error occurred while reading or writing the checkpoint
    /// file. `path` names the checkpoint location that was being
    /// accessed.
    #[error("checkpoint io at {path}: {source}", path = path.display())]
    Io {
        /// Path that was being accessed when the error occurred.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// The checkpoint file exists but its contents could not be parsed
    /// as JSON. Fresh-start (no file) returns `Ok(None)` from
    /// [`Checkpoint::read`] rather than this error.
    #[error("checkpoint parse at {path}: {source}", path = path.display())]
    Parse {
        /// Path of the malformed checkpoint file.
        path: PathBuf,
        /// Underlying JSON parse error.
        #[source]
        source: serde_json::Error,
    },
}

/// Daemon indexing-progress snapshot persisted at `.ucil/checkpoint.json`.
///
/// Written by the daemon on each indexing-phase boundary; read at
/// startup to skip an already-indexed prefix of the commit history
/// (master-plan §18 Phase 1 Week 3 — crash recovery, feature
/// `P1-W3-F09`).
///
/// All fields are `pub` because call-sites legitimately need to inspect
/// the last-indexed commit and the active branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Commit SHA of the last successfully-indexed revision, or `None`
    /// if the daemon has never completed an indexing run.
    pub last_indexed_commit: Option<String>,
    /// Branch that was active when this checkpoint was written. On
    /// restart, the daemon resumes on the same branch.
    pub active_branch: String,
    /// Unix timestamp (seconds since epoch) at which this checkpoint
    /// was written.
    pub saved_at: u64,
    /// Version of the daemon that wrote this checkpoint (the value of
    /// `CARGO_PKG_VERSION` at build time). Lets a future daemon detect
    /// an incompatible checkpoint schema and re-index from scratch.
    pub daemon_version: String,
}

impl Checkpoint {
    /// Construct a fresh checkpoint for a daemon that has never indexed
    /// anything on `active_branch`.
    ///
    /// `saved_at` is filled from [`std::time::SystemTime::now`];
    /// `daemon_version` from `env!("CARGO_PKG_VERSION")` so the value
    /// is baked in at compile time of the daemon binary.
    #[must_use]
    pub fn new(active_branch: String) -> Self {
        Self {
            last_indexed_commit: None,
            active_branch,
            saved_at: now_unix_secs(),
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Write `value` to `path` as pretty-printed JSON, creating or
    /// truncating the file. Flushes before returning.
    ///
    /// # Errors
    ///
    /// Returns [`CheckpointError::Io`] if the file cannot be created,
    /// written to, or flushed.
    #[tracing::instrument(
        name = "ucil.daemon.lifecycle.checkpoint.write",
        level = "debug",
        skip(value)
    )]
    pub fn write(path: &Path, value: &Self) -> Result<(), CheckpointError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|source| CheckpointError::Io {
                path: path.to_path_buf(),
                source,
            })?;

        serde_json::to_writer_pretty(&mut file, value).map_err(|e| {
            // serde_json write errors during serialisation of an
            // owned-type value can only originate from the underlying
            // writer (our `File`), since `Serialize` for Checkpoint is
            // infallible. Surface these as Io with the kind hint preserved.
            let kind = e.io_error_kind().unwrap_or(std::io::ErrorKind::Other);
            CheckpointError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(kind, e),
            }
        })?;
        file.flush().map_err(|source| CheckpointError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(())
    }

    /// Read and deserialise the checkpoint at `path`.
    ///
    /// Returns `Ok(None)` if the file does not exist — this is the
    /// expected fresh-start / pre-first-run case, not an error.
    /// Returns [`CheckpointError::Parse`] if the file exists but is
    /// not valid JSON of the expected shape.
    ///
    /// # Errors
    ///
    /// Returns [`CheckpointError::Io`] for any I/O error other than
    /// `NotFound`. Returns [`CheckpointError::Parse`] if the file
    /// exists but cannot be decoded into a [`Checkpoint`].
    #[tracing::instrument(name = "ucil.daemon.lifecycle.checkpoint.read", level = "debug")]
    pub fn read(path: &Path) -> Result<Option<Self>, CheckpointError> {
        let contents = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(CheckpointError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        serde_json::from_str(&contents)
            .map(Some)
            .map_err(|source| CheckpointError::Parse {
                path: path.to_path_buf(),
                source,
            })
    }

    /// Read the checkpoint at `path`, or construct a fresh one rooted
    /// at `default_branch` if no checkpoint file exists.
    ///
    /// # Errors
    ///
    /// Propagates [`CheckpointError::Io`] and [`CheckpointError::Parse`]
    /// from [`Checkpoint::read`]. A missing file is NOT an error —
    /// `restore_or_new` returns a freshly-constructed checkpoint in that
    /// case.
    pub fn restore_or_new(path: &Path, default_branch: &str) -> Result<Self, CheckpointError> {
        Ok(Self::read(path)?.unwrap_or_else(|| Self::new(default_branch.to_string())))
    }
}

/// Return the current unix time in seconds, or 0 if the clock is before
/// `UNIX_EPOCH` (which should be impossible on any modern OS).
///
/// Duplicated from `session_manager::now_unix_secs` to keep the
/// `lifecycle` module self-contained; see the work-order note on
/// minimal blast-radius extensions.
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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

// ── Checkpoint tests ─────────────────────────────────────────────────────
//
// Tests live at module root (NOT inside `mod tests { }`) so the frozen
// P1-W3-F09 selector `lifecycle::test_crash_recovery` resolves with
// exact path semantics, mirroring the PidFile tests above.

#[cfg(test)]
#[test]
fn test_crash_recovery() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("checkpoint.json");

    // Simulate a running daemon persisting its progress, then a restart.
    let cp = Checkpoint {
        last_indexed_commit: Some("abc123def".to_string()),
        active_branch: "main".to_string(),
        saved_at: 1_712_345_678,
        daemon_version: "0.1.0".to_string(),
    };
    Checkpoint::write(&path, &cp).expect("write checkpoint");
    assert!(path.exists(), "checkpoint file must exist after write");

    let restored = Checkpoint::restore_or_new(&path, "fallback").expect("restore");
    assert_eq!(
        restored.last_indexed_commit,
        Some("abc123def".to_string()),
        "restored checkpoint must retain last_indexed_commit",
    );
    assert_eq!(
        restored.active_branch, "main",
        "restored active_branch must win over default_branch",
    );
    assert_eq!(restored.saved_at, 1_712_345_678);
    assert_eq!(restored.daemon_version, "0.1.0");

    // Simulate a fresh daemon start (no checkpoint on disk).
    std::fs::remove_file(&path).expect("remove checkpoint");
    let fresh = Checkpoint::restore_or_new(&path, "dev").expect("restore fresh");
    assert_eq!(
        fresh.last_indexed_commit, None,
        "fresh checkpoint must have no last_indexed_commit",
    );
    assert_eq!(
        fresh.active_branch, "dev",
        "fresh checkpoint must pick up default_branch",
    );
    assert_eq!(
        fresh.daemon_version,
        env!("CARGO_PKG_VERSION"),
        "fresh checkpoint must stamp the current daemon version",
    );
}

#[cfg(test)]
#[test]
fn test_checkpoint_write_then_read_roundtrip() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("checkpoint.json");

    let original = Checkpoint {
        last_indexed_commit: Some("deadbeefcafe".to_string()),
        active_branch: "feat-round-trip".to_string(),
        saved_at: 1_700_000_042,
        daemon_version: "9.9.9".to_string(),
    };
    Checkpoint::write(&path, &original).expect("write");

    let read_back = Checkpoint::read(&path).expect("read").expect("Some");
    assert_eq!(
        read_back, original,
        "roundtrip must preserve every field exactly"
    );
}

#[cfg(test)]
#[test]
fn test_checkpoint_read_missing_returns_none() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("nope.json");
    assert!(!path.exists(), "sanity: precondition — file must not exist");

    let result = Checkpoint::read(&path).expect("read must not error on missing file");
    assert!(
        result.is_none(),
        "missing checkpoint file must map to Ok(None), not an error",
    );
}

#[cfg(test)]
#[test]
fn test_checkpoint_read_malformed_returns_parse_error() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("broken.json");
    std::fs::write(&path, "not json").expect("seed malformed file");

    let err = Checkpoint::read(&path).expect_err("malformed JSON must error");
    match err {
        CheckpointError::Parse { path: err_path, .. } => {
            assert_eq!(err_path, path);
        }
        CheckpointError::Io { .. } => panic!("expected Parse, got Io"),
    }
}
