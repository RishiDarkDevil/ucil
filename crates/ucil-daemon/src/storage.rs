//! UCIL daemon on-disk storage layout: the two-tier `.ucil/` directory tree.
//!
//! Master-plan §11.2 (lines 1060-1088) specifies a two-tier layout that
//! separates cross-branch knowledge from per-branch symbol indexes:
//!
//! ```text
//! .ucil/
//! ├── shared/                  # cross-branch (knowledge.db, memory.db, history.db)
//! ├── branches/<branch>/       # per-branch (symbols.db, vectors/, tags.lmdb, state.json)
//! ├── sessions/                # per-agent session snapshots
//! ├── plugins/                 # plugin-specific data
//! ├── backups/                 # auto-backups before compaction
//! ├── otel/                    # OpenTelemetry export buffer
//! └── logs/                    # daemon.log and rotated siblings
//! ```
//!
//! [`StorageLayout`] owns a `base` (the absolute path to `.ucil/`) and a
//! sanitised `branch` name, and exposes read-only accessors for every
//! subpath the daemon writes to. [`StorageLayout::init`] creates the seven
//! directories idempotently via [`std::fs::create_dir_all`] — master-plan
//! §18 Phase 1 Week 2 line 1737 bullets this as a week-2 deliverable
//! (feature `P1-W2-F06`).
//!
//! Branch names may contain forward slashes in git (`feat/new-thing`);
//! [`StorageLayout::init`] translates `/` to `-` before touching the
//! filesystem so the layout stays flat — a nested branch directory would
//! collide with its parent when two branches share a prefix
//! (`feat/foo` and `feat/bar` would both try to create `branches/feat/`).
//!
//! # Examples
//!
//! ```no_run
//! use std::path::Path;
//! use ucil_daemon::storage::StorageLayout;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let layout = StorageLayout::init(Path::new(".ucil"), "main")?;
//! assert_eq!(layout.branch(), "main");
//! let symbols_db = layout.branch_symbols_db_path();
//! let knowledge_db = layout.shared_knowledge_db_path();
//! # drop((symbols_db, knowledge_db));
//! # Ok(())
//! # }
//! ```

// Public API items share a name prefix with the module ("storage" →
// "StorageLayout", "StorageError"). Convention matches `session_manager`
// and `lifecycle` in this crate.
#![allow(clippy::module_name_repetitions)]

use std::{
    path::{Path, PathBuf},
    result::Result,
};

use thiserror::Error;

/// Errors returned by [`StorageLayout`] operations.
///
/// `#[non_exhaustive]` so future variants (e.g. a permission-denied
/// recovery path) can land without a SemVer-breaking change.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StorageError {
    /// An I/O error occurred while creating or accessing one of the
    /// layout directories. `path` names the specific directory whose
    /// creation failed.
    #[error("io at {path}: {source}", path = path.display())]
    Io {
        /// Path that was being created when the error occurred.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// The caller passed an empty branch name to [`StorageLayout::init`].
    /// Branch names must be non-empty — the empty string would produce a
    /// `branches/` directory with no branch subdirectory beneath it,
    /// breaking the master-plan §11.2 invariant.
    #[error("empty branch name")]
    EmptyBranch,
}

/// Materialised two-tier layout rooted at a caller-supplied `.ucil/` path.
///
/// Construct via [`StorageLayout::init`], which creates every required
/// subdirectory. All path accessors are pure — they do not touch the
/// filesystem — so the struct is cheap to clone and pass around.
///
/// The `base` is stored as an owned [`PathBuf`]; callers typically pass
/// `<repo>/.ucil`. The `branch` is stored *after* `/` → `-` sanitisation
/// so repeated `branch()` reads do not recompute the mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLayout {
    pub(crate) base: PathBuf,
    pub(crate) branch: String,
}

impl StorageLayout {
    /// Initialise the two-tier `.ucil/` layout at `base` for `branch`.
    ///
    /// Creates (via [`std::fs::create_dir_all`], so re-running is a no-op):
    /// `shared/`, `branches/<sanitised-branch>/`, `sessions/`, `plugins/`,
    /// `backups/`, `otel/`, and `logs/`. A branch name containing `/` is
    /// sanitised to `-` *before* any filesystem call, keeping the layout
    /// flat (see module rustdoc for the rationale).
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::EmptyBranch`] if `branch` is the empty
    /// string. Returns [`StorageError::Io`] if any of the seven
    /// directories cannot be created.
    #[tracing::instrument(
        name = "ucil.daemon.storage.init",
        level = "debug",
        fields(branch = branch),
    )]
    pub fn init(base: &Path, branch: &str) -> Result<Self, StorageError> {
        if branch.is_empty() {
            return Err(StorageError::EmptyBranch);
        }
        let sanitised = sanitise_branch(branch);
        let layout = Self {
            base: base.to_path_buf(),
            branch: sanitised,
        };

        for dir in [
            layout.shared_dir(),
            layout.branch_dir(),
            layout.sessions_dir(),
            layout.plugins_dir(),
            layout.backups_dir(),
            layout.otel_dir(),
            layout.logs_dir(),
        ] {
            std::fs::create_dir_all(&dir).map_err(|source| StorageError::Io {
                path: dir.clone(),
                source,
            })?;
        }

        Ok(layout)
    }

    /// Root `.ucil/` path this layout was initialised under.
    #[must_use]
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Sanitised branch name used for the per-branch subdirectory.
    ///
    /// Always free of `/` (forward slashes are replaced with `-` during
    /// [`StorageLayout::init`]).
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// `<base>/shared/` — cross-branch data (knowledge.db, memory.db,
    /// history.db).
    #[must_use]
    pub fn shared_dir(&self) -> PathBuf {
        self.base.join("shared")
    }

    /// `<base>/branches/<branch>/` — per-branch symbol / vector data.
    #[must_use]
    pub fn branch_dir(&self) -> PathBuf {
        self.base.join("branches").join(&self.branch)
    }

    /// `<base>/sessions/` — per-agent session snapshots.
    #[must_use]
    pub fn sessions_dir(&self) -> PathBuf {
        self.base.join("sessions")
    }

    /// `<base>/plugins/` — plugin-specific data.
    #[must_use]
    pub fn plugins_dir(&self) -> PathBuf {
        self.base.join("plugins")
    }

    /// `<base>/backups/` — auto-backups taken before compaction.
    #[must_use]
    pub fn backups_dir(&self) -> PathBuf {
        self.base.join("backups")
    }

    /// `<base>/otel/` — OpenTelemetry export buffer.
    #[must_use]
    pub fn otel_dir(&self) -> PathBuf {
        self.base.join("otel")
    }

    /// `<base>/logs/` — daemon log files.
    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.base.join("logs")
    }

    /// `<base>/shared/knowledge.db` — master-plan §11.2 line 1067.
    #[must_use]
    pub fn shared_knowledge_db_path(&self) -> PathBuf {
        self.shared_dir().join("knowledge.db")
    }

    /// `<base>/shared/memory.db` — master-plan §11.2 line 1069.
    #[must_use]
    pub fn shared_memory_db_path(&self) -> PathBuf {
        self.shared_dir().join("memory.db")
    }

    /// `<base>/shared/history.db` — master-plan §11.2 line 1070.
    #[must_use]
    pub fn shared_history_db_path(&self) -> PathBuf {
        self.shared_dir().join("history.db")
    }

    /// `<base>/branches/<branch>/symbols.db` — master-plan §11.2 line 1073.
    #[must_use]
    pub fn branch_symbols_db_path(&self) -> PathBuf {
        self.branch_dir().join("symbols.db")
    }

    /// `<base>/branches/<branch>/vectors/` — master-plan §11.2 line 1074.
    #[must_use]
    pub fn branch_vectors_dir(&self) -> PathBuf {
        self.branch_dir().join("vectors")
    }

    /// `<base>/branches/<branch>/tags.lmdb` — master-plan §11.2 line 1075.
    #[must_use]
    pub fn branch_tags_lmdb_path(&self) -> PathBuf {
        self.branch_dir().join("tags.lmdb")
    }

    /// `<base>/branches/<branch>/state.json` — master-plan §11.2 line 1076.
    #[must_use]
    pub fn branch_state_json_path(&self) -> PathBuf {
        self.branch_dir().join("state.json")
    }
}

/// Replace `/` in a branch name with `-` so the layout stays flat.
///
/// Callers must have already rejected the empty string; this helper
/// does not re-check. Keeping the function private lets the public
/// [`StorageLayout::init`] own the empty-branch contract.
fn sanitise_branch(branch: &str) -> String {
    branch.replace('/', "-")
}

// ── Tests ────────────────────────────────────────────────────────────────
//
// Tests live at module root (NOT wrapped in `#[cfg(test)] mod tests { }`)
// per DEC-0005 + the WO-0006 rejection post-mortem: the frozen F06
// selector is `storage::test_two_tier_layout`, an exact path prefix match
// produced by nextest. A `mod tests { }` wrapper would insert a `::tests::`
// segment and break selector resolution — the same bug that rejected
// WO-0006 three times. Mirror `lifecycle.rs:257-333` instead.

#[cfg(test)]
#[test]
fn test_two_tier_layout() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = tmp.path();
    let layout = StorageLayout::init(base, "main").expect("init");

    // Every directory named in master-plan §11.2 must exist after init.
    for sub in [
        "shared",
        "branches/main",
        "sessions",
        "plugins",
        "backups",
        "otel",
        "logs",
    ] {
        let p = base.join(sub);
        assert!(p.is_dir(), "expected directory at {}", p.display());
    }

    // Re-running init on the same base must succeed (idempotent via
    // create_dir_all).
    let again = StorageLayout::init(base, "main").expect("second init");
    assert_eq!(again, layout, "idempotent init must return an equal layout");

    // Accessor paths resolve under the expected subtrees.
    assert_eq!(
        layout.shared_knowledge_db_path(),
        base.join("shared/knowledge.db")
    );
    assert_eq!(
        layout.shared_memory_db_path(),
        base.join("shared/memory.db")
    );
    assert_eq!(
        layout.shared_history_db_path(),
        base.join("shared/history.db")
    );
    assert_eq!(
        layout.branch_symbols_db_path(),
        base.join("branches/main/symbols.db"),
    );
    assert_eq!(
        layout.branch_vectors_dir(),
        base.join("branches/main/vectors"),
    );
    assert_eq!(
        layout.branch_tags_lmdb_path(),
        base.join("branches/main/tags.lmdb"),
    );
    assert_eq!(
        layout.branch_state_json_path(),
        base.join("branches/main/state.json"),
    );
}

#[cfg(test)]
#[test]
fn test_branch_name_with_slashes_is_sanitised() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = tmp.path();
    let layout = StorageLayout::init(base, "feat/new-thing").expect("init");

    assert_eq!(layout.branch(), "feat-new-thing");
    assert!(
        base.join("branches/feat-new-thing").is_dir(),
        "sanitised branch directory must exist",
    );
    assert!(
        !base.join("branches/feat").exists(),
        "unsanitised nested directory must NOT exist",
    );
    assert_eq!(
        layout.branch_symbols_db_path(),
        base.join("branches/feat-new-thing/symbols.db"),
    );
}

#[cfg(test)]
#[test]
fn test_empty_branch_rejected() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let err = StorageLayout::init(tmp.path(), "").expect_err("must reject empty branch");
    match err {
        StorageError::EmptyBranch => {}
        StorageError::Io { .. } => panic!("expected EmptyBranch, got Io"),
    }
    // No directories should have been created for a rejected init.
    assert!(!tmp.path().join("shared").exists());
    assert!(!tmp.path().join("branches").exists());
}

#[cfg(test)]
#[test]
fn test_init_is_idempotent() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = tmp.path();

    let first = StorageLayout::init(base, "main").expect("first init");
    let second = StorageLayout::init(base, "main").expect("second init");
    let third = StorageLayout::init(base, "main").expect("third init");

    assert_eq!(first, second);
    assert_eq!(second, third);

    for sub in [
        "shared",
        "branches/main",
        "sessions",
        "plugins",
        "backups",
        "otel",
        "logs",
    ] {
        assert!(
            base.join(sub).is_dir(),
            "directory {sub} must still exist after three inits",
        );
    }
}

#[cfg(test)]
#[test]
fn test_shared_paths_are_branch_independent() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = tmp.path();

    let on_main = StorageLayout::init(base, "main").expect("init main");
    let on_feat = StorageLayout::init(base, "feat-x").expect("init feat-x");

    assert_eq!(
        on_main.shared_knowledge_db_path(),
        on_feat.shared_knowledge_db_path(),
        "shared knowledge.db must be branch-independent",
    );
    assert_eq!(
        on_main.shared_memory_db_path(),
        on_feat.shared_memory_db_path(),
    );
    assert_eq!(
        on_main.shared_history_db_path(),
        on_feat.shared_history_db_path(),
    );
    assert_ne!(
        on_main.branch_symbols_db_path(),
        on_feat.branch_symbols_db_path(),
        "per-branch symbols.db must differ between branches",
    );
    assert_ne!(on_main.branch_dir(), on_feat.branch_dir());
}

#[cfg(test)]
#[test]
fn test_storage_error_io_display_mentions_path() {
    let err = StorageError::Io {
        path: PathBuf::from("/nope/.ucil/shared"),
        source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    };
    let msg = format!("{err}");
    assert!(msg.contains("/nope/.ucil/shared"), "{msg}");
    assert!(msg.contains("io at"), "{msg}");
}
