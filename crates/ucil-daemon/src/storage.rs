//! Two-tier `.ucil/` storage layout initialiser.
//!
//! [`StorageLayout::init`] creates the canonical directory hierarchy that UCIL
//! uses to store all per-project state:
//!
//! ```text
//! <base>/
//! └── .ucil/
//!     ├── shared/            ← cross-branch artefacts (e.g. shared embeddings)
//!     ├── branches/
//!     │   └── <branch>/      ← per-branch state (LanceDB, LMDB tag cache, …)
//!     ├── sessions/          ← per-session transient state
//!     ├── logs/              ← structured daemon logs
//!     └── plugins/           ← plugin data directories
//! ```
//!
//! All directories are created with [`std::fs::create_dir_all`], so the call
//! is **idempotent** — calling `init` on an already-initialised tree is safe.
//!
//! Branch names containing forward slashes (e.g. `"feat/my-feature"`) are
//! sanitised to hyphens when forming the directory name so that nested paths
//! are avoided.

// Items intentionally share a name prefix with the module.
#![allow(clippy::module_name_repetitions)]

use std::path::{Path, PathBuf};

use thiserror::Error;

// ── Error type ─────────────────────────────────────────────────────────────

/// Errors returned by [`StorageLayout::init`] and its accessor methods.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StorageError {
    /// An OS-level I/O error while creating a directory.
    #[error("I/O error creating {}: {source}", path.display())]
    Io {
        /// The path that triggered the error.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },
}

// ── Handle type ────────────────────────────────────────────────────────────

/// A typed handle to an initialised `.ucil/` storage tree.
///
/// All accessor methods return `PathBuf` / `&Path` values pointing into the
/// directory tree that was created by [`init`][StorageLayout::init].
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use ucil_daemon::storage::StorageLayout;
///
/// let layout = StorageLayout::init(Path::new("/my/project"), "main").unwrap();
/// assert!(layout.shared_dir().exists());
/// assert!(layout.branch_dir().exists());
/// ```
#[derive(Debug, Clone)]
pub struct StorageLayout {
    /// The project root passed to [`init`][StorageLayout::init].
    base: PathBuf,
    /// The sanitised branch name (slashes → hyphens).
    branch: String,
    /// `<base>/.ucil/shared/`
    shared_dir: PathBuf,
    /// `<base>/.ucil/branches/<branch>/`
    branch_dir: PathBuf,
}

impl StorageLayout {
    /// Initialise the two-tier `.ucil/` storage layout under `base`.
    ///
    /// Creates the following directories (idempotent via `create_dir_all`):
    ///
    /// | Directory                             | Purpose                        |
    /// |---------------------------------------|--------------------------------|
    /// | `<base>/.ucil/shared/`                | Cross-branch shared artefacts  |
    /// | `<base>/.ucil/branches/<branch>/`     | Branch-local state             |
    /// | `<base>/.ucil/sessions/`              | Per-session transient state    |
    /// | `<base>/.ucil/logs/`                  | Structured daemon logs         |
    /// | `<base>/.ucil/plugins/`               | Plugin data directories        |
    ///
    /// Forward slashes in `branch` are replaced by hyphens when forming the
    /// branch-specific directory name (e.g. `"feat/foo"` → `"feat-foo"`).
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Io`] if any `create_dir_all` call fails.
    pub fn init(base: &Path, branch: &str) -> Result<Self, StorageError> {
        let ucil_root = base.join(".ucil");

        // Sanitise branch name: slashes → hyphens, to avoid nested paths.
        let branch_safe = branch.replace('/', "-");

        let shared_dir = ucil_root.join("shared");
        let branch_dir = ucil_root.join("branches").join(&branch_safe);
        let sessions_dir = ucil_root.join("sessions");
        let logs_dir = ucil_root.join("logs");
        let plugins_dir = ucil_root.join("plugins");

        for dir in [
            &shared_dir,
            &branch_dir,
            &sessions_dir,
            &logs_dir,
            &plugins_dir,
        ] {
            std::fs::create_dir_all(dir).map_err(|source| StorageError::Io {
                path: dir.clone(),
                source,
            })?;
        }

        Ok(Self {
            base: base.to_path_buf(),
            branch: branch_safe,
            shared_dir,
            branch_dir,
        })
    }

    /// Return the project root that was passed to [`init`].
    #[must_use]
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Return the sanitised branch name used as the directory component.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Return `<base>/.ucil/shared/`.
    #[must_use]
    pub fn shared_dir(&self) -> &Path {
        &self.shared_dir
    }

    /// Return `<base>/.ucil/branches/<branch>/`.
    #[must_use]
    pub fn branch_dir(&self) -> &Path {
        &self.branch_dir
    }

    /// Return `<base>/.ucil/sessions/`.
    #[must_use]
    pub fn sessions_dir(&self) -> PathBuf {
        self.base.join(".ucil").join("sessions")
    }

    /// Return `<base>/.ucil/logs/`.
    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.base.join(".ucil").join("logs")
    }

    /// Return `<base>/.ucil/plugins/`.
    #[must_use]
    pub fn plugins_dir(&self) -> PathBuf {
        self.base.join(".ucil").join("plugins")
    }
}

// ── Unit tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The two-tier layout must be fully created on first call and all
    /// mandatory directories must exist on disk afterward.
    #[test]
    fn test_two_tier_layout() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let base = tmp.path();
        let branch = "feat/my-feature";

        let layout = StorageLayout::init(base, branch).expect("init must succeed");

        // shared_dir exists
        assert!(
            layout.shared_dir().exists(),
            "shared_dir() must exist: {}",
            layout.shared_dir().display()
        );

        // branch_dir exists
        assert!(
            layout.branch_dir().exists(),
            "branch_dir() must exist: {}",
            layout.branch_dir().display()
        );

        // branch_dir contains the sanitised branch name
        let branch_dir_str = layout.branch_dir().to_string_lossy();
        assert!(
            branch_dir_str.contains("feat-my-feature"),
            "branch_dir should contain 'feat-my-feature', got: {branch_dir_str}"
        );

        // sessions_dir exists
        assert!(
            layout.sessions_dir().exists(),
            "sessions_dir() must exist: {}",
            layout.sessions_dir().display()
        );

        // logs_dir and plugins_dir exist
        assert!(
            layout.logs_dir().exists(),
            "logs_dir() must exist: {}",
            layout.logs_dir().display()
        );
        assert!(
            layout.plugins_dir().exists(),
            "plugins_dir() must exist: {}",
            layout.plugins_dir().display()
        );

        // init is idempotent: calling it twice must not error
        StorageLayout::init(base, branch).expect("second init must also succeed");
    }
}
