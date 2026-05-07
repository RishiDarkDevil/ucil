//! Per-branch `LanceDB` vector-store lifecycle.
//!
//! Master-plan §6.4 line 144 specifies a "Branch index manager" that
//! "Creates, updates, prunes, and archives per-branch code indexes.
//! Delta indexing from parent branches for fast creation."  Master-plan
//! §3.2 line 1643 places `branch_manager.rs` directly under
//! `crates/ucil-daemon/src/`.  Master-plan §11.2 line 1074 fixes the
//! per-branch `vectors/` directory as the `LanceDB` connection root,
//! and master-plan §12.2 lines 1321-1346 freezes the `code_chunks`
//! table schema (12 fields including a `FixedSizeList<Float32, 768>`
//! `embedding` column for the `CodeRankEmbed` default).  Master-plan
//! §18 Phase 2 Week 7 line 1782 ("`LanceDB` per-branch") is the
//! deliverable this module ships against feature `P2-W7-F09`.
//!
//! [`BranchManager`] exposes three async lifecycle operations:
//!
//! * [`BranchManager::create_branch_table`] opens a
//!   `lancedb::connect()` at the per-branch `vectors/` directory and
//!   creates an empty `code_chunks` table conforming to the §12.2
//!   schema.  Passing `parent = Some(other)` performs a *delta
//!   clone* by recursively copying the parent branch's `vectors/`
//!   directory tree before opening the connection — the new branch
//!   starts with the parent's already-indexed Lance dataset files
//!   byte-for-byte, then subsequent indexing only re-processes
//!   changed files (per the `file_hash` column populated by
//!   `P2-W8-F04` background indexing).
//! * [`BranchManager::archive_branch_table`] renames
//!   `<base>/branches/<sanitised>/` to
//!   `<base>/branches/.archive/<sanitised>-<unix_ts_micros>/` so the
//!   table data persists for forensics but the live tree is clean.
//!   The whole branch directory moves atomically (vectors/,
//!   symbols.db, tags.lmdb, state.json), preserving cross-table
//!   consistency.
//! * Read-only accessors [`BranchManager::branch_vectors_dir`],
//!   [`BranchManager::archive_root`], and
//!   [`BranchManager::branches_root`] for path arithmetic without
//!   filesystem access.
//!
//! ## Design notes
//!
//! * **Delta clone via directory recursive-copy.**  The phrase "delta
//!   indexing from parent branches" in §6.4 line 144 is implemented at
//!   the filesystem level — preserves the parent's already-indexed
//!   Lance dataset files byte-for-byte.  No use of `lancedb`'s
//!   hypothetical `delta` / `replicate` / `merge` features (none such
//!   are stable in 0.16); only `file_hash`-driven re-processing in
//!   P2-W8-F04 distinguishes changed from unchanged chunks.
//! * **Per-branch isolation guarantee.**  One `lancedb::Connection`
//!   per `<base>/branches/<branch>/vectors/`, no cross-branch table
//!   sharing.  Two branches cannot collide on a `code_chunks` table
//!   even if they were created in parallel.
//! * **Archive convention.**  Archives live under a hidden
//!   `<base>/branches/.archive/` directory keyed by `<sanitised
//!   branch>-<unix_ts_micros>`.  The hidden-directory prefix is
//!   intentional: filesystem listings tooling that respects POSIX
//!   hidden-dir conventions skips it by default.
//! * **No symlink traversal.**  Per master-plan §11.4 line 1090,
//!   branch trees are pure file/directory hierarchies.  The
//!   [`BranchManager::create_branch_table`] recursive-copy helper
//!   skips entries that are not `is_file()` or `is_dir()` —
//!   symlinks are silently dropped on clone.
//!
//! Production wiring (creating a per-branch `code_chunks` table on
//! first session against a previously-unseen branch, archiving on
//! branch deletion detection) is deferred to feature `P2-W8-F04`
//! (`LanceDB` background chunk indexing).  This module ships the
//! standalone [`BranchManager`] API + the unit test verifying its
//! lifecycle semantics.

// Public API items in this module share a name prefix with the module
// (`branch_manager` → `BranchManager`, `BranchManagerError`,
// `BranchTableInfo`).  Convention matches `storage::StorageLayout`,
// `plugin_manager::PluginManager`, and `session_manager::SessionManager`.
#![allow(clippy::module_name_repetitions)]

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use thiserror::Error;

/// Hidden-directory name under `<base>/branches/` where archived branches are moved.
///
/// The leading `.` matches `POSIX` hidden-dir convention so `ls`
/// (without `-a`) and most filesystem-listing tools skip it by default
/// — keeping the live `branches/` tree visually uncluttered.  See
/// module rustdoc for the full archive convention.
pub const ARCHIVE_DIR_NAME: &str = ".archive";

/// Errors returned by [`BranchManager`] operations.
///
/// `#[non_exhaustive]` so future variants can land without a
/// `SemVer`-breaking change — same convention as
/// [`crate::storage::StorageError`] and other crate-local error enums.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BranchManagerError {
    /// A branch directory path could not be encoded as UTF-8.
    /// `LanceDB`'s `connect()` requires UTF-8 URIs; an OsString-only
    /// path (only reachable on a non-UTF-8 mount on Unix or a
    /// long-Path WTF-16 path on Windows) is rejected here rather than
    /// silently lossy-converted.
    #[error("branch path is not valid UTF-8: {path:?}")]
    NonUtf8Path {
        /// Path that failed UTF-8 encoding.
        path: PathBuf,
    },

    /// A `lancedb` operation failed (connect, list tables, create
    /// empty table, etc.).
    #[error("lancedb operation failed: {source}")]
    Lance {
        /// Underlying `lancedb` error.
        #[from]
        source: lancedb::Error,
    },

    /// Constructing the `code_chunks` Arrow schema failed.  Practically
    /// unreachable for the fixed §12.2 schema, but kept for
    /// completeness so future `Schema::try_new` paths can surface
    /// errors typed.
    #[error("arrow schema construction failed: {source}")]
    Arrow {
        /// Underlying `arrow-schema` error.
        #[from]
        source: arrow_schema::ArrowError,
    },

    /// An I/O error occurred while creating, copying, or renaming a
    /// branch directory.
    #[error("io error during branch directory operation: {source}")]
    Io {
        /// Underlying OS error.
        #[from]
        source: std::io::Error,
    },

    /// Cloning a branch from a parent failed because the parent has
    /// no `vectors/` directory.  Returned by
    /// [`BranchManager::create_branch_table`] when `parent =
    /// Some(name)` but `<base>/branches/<sanitised parent>/vectors/`
    /// does not exist.
    #[error("parent branch '{parent}' has no vectors directory at {path:?}")]
    ParentNotFound {
        /// Sanitised parent branch name (after `/` → `-`).
        parent: String,
        /// Absolute path that was checked.
        path: PathBuf,
    },

    /// Archiving a branch failed because the target branch directory
    /// does not exist.  Returned by
    /// [`BranchManager::archive_branch_table`] when
    /// `<base>/branches/<sanitised name>/` is absent.
    #[error("branch '{name}' has no directory to archive at {path:?}")]
    BranchNotFound {
        /// Sanitised branch name (after `/` → `-`).
        name: String,
        /// Absolute path that was checked.
        path: PathBuf,
    },
}

/// Description of a `code_chunks` table after
/// [`BranchManager::create_branch_table`] has populated the per-branch
/// `vectors/` directory.
///
/// Returned so callers can audit the post-creation state without
/// re-opening the `lancedb::Connection`.
///
/// * `branch` is the SANITISED branch name (after `/` → `-` mapping)
///   so callers can use it as a directory key without re-running the
///   sanitiser.
/// * `vectors_dir` is the absolute path to the `LanceDB` connection
///   root (`<base>/branches/<sanitised>/vectors/`).
/// * `table_count` is the number of tables present after creation.
///   For a fresh branch this is `1` (just `code_chunks`); for a
///   delta-clone branch it is `≥ 1` because the parent's tables were
///   copied across.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTableInfo {
    /// Sanitised branch name.
    pub branch: String,
    /// Absolute path to the `vectors/` directory.
    pub vectors_dir: PathBuf,
    /// Number of tables present after creation.
    pub table_count: usize,
}

/// Owner of the per-branch `LanceDB` connection lifecycle.
///
/// Construct via [`BranchManager::new`] with the absolute path of the
/// per-repo `<base>/branches/` directory.  All async lifecycle methods
/// take `&self` (no interior mutability needed; the `lancedb`
/// `Connection` is short-lived per call), so a single
/// [`BranchManager`] can serve concurrent calls from multiple
/// sessions.
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
/// use ucil_daemon::branch_manager::BranchManager;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mgr = BranchManager::new(PathBuf::from("/tmp/example/.ucil/branches"));
/// // `mgr.create_branch_table("main", None).await` etc. — see method docs.
/// drop(mgr);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct BranchManager {
    branches_root: PathBuf,
    archive_root: PathBuf,
}

impl BranchManager {
    /// Build a new manager rooted at `branches_root`.
    ///
    /// `branches_root` is typically `<base>/branches/` where `<base>`
    /// is the `.ucil/` directory.  The archive subdirectory
    /// (`<branches_root>/.archive/`) is computed eagerly from the
    /// root; it is NOT created here —
    /// [`BranchManager::archive_branch_table`] creates it lazily on
    /// first archive.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use ucil_daemon::branch_manager::{BranchManager, ARCHIVE_DIR_NAME};
    ///
    /// let root = PathBuf::from("/tmp/example/branches");
    /// let mgr = BranchManager::new(root.clone());
    /// let _ = assert_eq!(mgr.branches_root(), root.as_path());
    /// let _ = assert_eq!(mgr.archive_root(), root.join(ARCHIVE_DIR_NAME).as_path());
    /// ```
    pub fn new(branches_root: impl Into<PathBuf>) -> Self {
        let branches_root = branches_root.into();
        let archive_root = branches_root.join(ARCHIVE_DIR_NAME);
        Self {
            branches_root,
            archive_root,
        }
    }

    /// Absolute path of the per-repo `<base>/branches/` directory.
    #[must_use]
    pub fn branches_root(&self) -> &Path {
        &self.branches_root
    }

    /// Absolute path of the archive subdirectory
    /// (`<branches_root>/.archive/`).  The directory itself is created
    /// lazily on first archive — calling this accessor does not touch
    /// the filesystem.
    #[must_use]
    pub fn archive_root(&self) -> &Path {
        &self.archive_root
    }

    /// Compute the path of a branch's `vectors/` directory.
    ///
    /// Pure path arithmetic — does NOT touch the filesystem.  The
    /// branch name is run through the module-private
    /// `sanitise_branch_name` helper (`/` → `-`) so callers can pass
    /// raw git ref names like `feat/foo` without first sanitising
    /// them.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use ucil_daemon::branch_manager::BranchManager;
    ///
    /// let mgr = BranchManager::new(PathBuf::from("/repo/branches"));
    /// let _ = assert_eq!(
    ///     mgr.branch_vectors_dir("feat/foo"),
    ///     PathBuf::from("/repo/branches/feat-foo/vectors"),
    /// );
    /// ```
    #[must_use]
    pub fn branch_vectors_dir(&self, name: &str) -> PathBuf {
        self.branches_root
            .join(sanitise_branch_name(name))
            .join("vectors")
    }
}

/// Build the `code_chunks` table schema fixed by master-plan §12.2
/// lines 1321-1346.
///
/// Twelve fields in declaration order:
///
/// | column        | type                                       | nullable | rationale |
/// |---------------|--------------------------------------------|----------|-----------|
/// | `id`          | `Utf8`                                     | no       | row primary key |
/// | `file_path`   | `Utf8`                                     | no       | repo-relative path |
/// | `start_line`  | `Int32`                                    | no       | 1-based |
/// | `end_line`    | `Int32`                                    | no       | inclusive |
/// | `content`     | `Utf8`                                     | no       | chunk source text |
/// | `language`    | `Utf8`                                     | no       | tree-sitter language id |
/// | `symbol_name` | `Utf8`                                     | YES      | empty for non-symbol chunks (e.g. docstrings) |
/// | `symbol_kind` | `Utf8`                                     | YES      | empty for non-symbol chunks |
/// | `embedding`   | `FixedSizeList<Float32, 768>`              | no       | `CodeRankEmbed` default (P2-W8-F02) |
/// | `token_count` | `Int32`                                    | no       | tokeniser-reported length |
/// | `file_hash`   | `Utf8`                                     | no       | drives delta re-indexing in P2-W8-F04 |
/// | `indexed_at`  | `Timestamp(Microsecond, None)`             | no       | local-time write watermark |
///
/// The 768-dimensional embedding column matches `CodeRankEmbed`'s
/// default model size (master-plan §12.2 line 1332 documents both 768
/// and 1024 — a future ADR may grow this to 1024 for `Qwen3-Embedding`
/// per `P2-W8-F03`).
#[must_use]
pub fn code_chunks_schema() -> SchemaRef {
    let embedding_inner = Arc::new(Field::new("item", DataType::Float32, false));
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("file_path", DataType::Utf8, false),
        Field::new("start_line", DataType::Int32, false),
        Field::new("end_line", DataType::Int32, false),
        Field::new("content", DataType::Utf8, false),
        Field::new("language", DataType::Utf8, false),
        Field::new("symbol_name", DataType::Utf8, true),
        Field::new("symbol_kind", DataType::Utf8, true),
        Field::new(
            "embedding",
            DataType::FixedSizeList(embedding_inner, 768),
            false,
        ),
        Field::new("token_count", DataType::Int32, false),
        Field::new("file_hash", DataType::Utf8, false),
        Field::new(
            "indexed_at",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
    ]))
}

/// Replace `/` in a branch name with `-` so the per-branch directory
/// stays flat under `<base>/branches/`.  Mirror of
/// `storage::sanitise_branch` (private to that module).
///
/// Promoted to `pub` in `WO-0064` (`P2-W8-F04`) per the carve-out at
/// lines 347-349 of the prior revision: "intentional duplication so
/// `branch_manager.rs` is self-contained — a shared helper would
/// require a new public symbol surface and is deferred to a follow-up
/// ADR if a third caller materialises".  `lancedb_indexer.rs` IS the
/// third caller (the first two being [`BranchManager::create_branch_table`]
/// and [`BranchManager::archive_branch_table`]) so the visibility is
/// promoted here without an ADR per the explicit carve-out.
#[must_use]
pub fn sanitise_branch_name(name: &str) -> String {
    name.replace('/', "-")
}

impl BranchManager {
    /// Open a per-branch `LanceDB` connection root and create the
    /// `code_chunks` table if it does not already exist.
    ///
    /// On `parent = None` this creates a fresh branch from scratch:
    /// the per-branch `vectors/` directory is `mkdir -p`'d and the
    /// empty `code_chunks` table conforming to the master-plan §12.2
    /// schema is created.
    ///
    /// On `parent = Some(name)` this performs a *delta clone*: the
    /// parent's `vectors/` directory is recursively copied to the
    /// new branch's `vectors/` directory before the connection is
    /// opened, so the new branch starts with the parent's
    /// already-indexed Lance dataset files byte-for-byte.  If the
    /// parent has no `code_chunks` table (e.g. a partial clone), an
    /// empty one is then created; if it already has one, the table
    /// listing returned in [`BranchTableInfo::table_count`] reflects
    /// the inherited table.
    ///
    /// Branch names are sanitised by replacing `/` with `-` before
    /// any filesystem call — callers may pass raw git ref names like
    /// `feat/foo`.
    ///
    /// # Errors
    ///
    /// Returns [`BranchManagerError::NonUtf8Path`] if the per-branch
    /// `vectors/` path is not valid UTF-8 (cannot be passed to
    /// `lancedb::connect`).  Returns
    /// [`BranchManagerError::ParentNotFound`] if `parent =
    /// Some(name)` but the parent's `vectors/` directory does not
    /// exist.  Returns [`BranchManagerError::Io`] on any directory-
    /// creation, recursive-copy, or `try_exists` failure.  Returns
    /// [`BranchManagerError::Lance`] on `lancedb::connect`,
    /// `table_names`, or `create_empty_table` failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::PathBuf;
    /// use ucil_daemon::branch_manager::BranchManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # tokio::runtime::Runtime::new()?.block_on(async {
    /// let mgr = BranchManager::new(PathBuf::from("/repo/.ucil/branches"));
    /// let info = mgr.create_branch_table("main", None).await?;
    /// let _ = assert_eq!(info.branch, "main");
    /// let _ = assert!(info.vectors_dir.ends_with("main/vectors"));
    /// # Ok::<(), ucil_daemon::branch_manager::BranchManagerError>(())
    /// # })?;
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(
        name = "ucil.daemon.branch_manager.create",
        level = "debug",
        skip(self),
        fields(branch = name, parent = ?parent),
    )]
    pub async fn create_branch_table(
        &self,
        name: &str,
        parent: Option<&str>,
    ) -> Result<BranchTableInfo, BranchManagerError> {
        let sanitised = sanitise_branch_name(name);
        let target_branch_dir = self.branches_root.join(&sanitised);
        let target_vectors = target_branch_dir.join("vectors");

        if let Some(parent_name) = parent {
            let sanitised_parent = sanitise_branch_name(parent_name);
            let source_vectors = self.branches_root.join(&sanitised_parent).join("vectors");
            if !tokio::fs::try_exists(&source_vectors).await? {
                return Err(BranchManagerError::ParentNotFound {
                    parent: sanitised_parent,
                    path: source_vectors,
                });
            }
            copy_dir_recursive(&source_vectors, &target_vectors).await?;
        }

        // Idempotent — recursive-copy already created the directory
        // when parent.is_some(); fresh-branch path needs the mkdir.
        tokio::fs::create_dir_all(&target_vectors).await?;

        let uri = target_vectors
            .to_str()
            .ok_or_else(|| BranchManagerError::NonUtf8Path {
                path: target_vectors.clone(),
            })?;
        let conn = lancedb::connect(uri).execute().await?;
        let existing = conn.table_names().execute().await?;
        if !existing.iter().any(|t| t == "code_chunks") {
            conn.create_empty_table("code_chunks", code_chunks_schema())
                .execute()
                .await?;
        }
        let final_tables = conn.table_names().execute().await?;

        Ok(BranchTableInfo {
            branch: sanitised,
            vectors_dir: target_vectors,
            table_count: final_tables.len(),
        })
    }

    /// Move a per-branch directory tree under
    /// `<branches_root>/.archive/<sanitised>-<unix_ts_micros>/` and
    /// return the absolute path of the archive target on success.
    ///
    /// The whole branch directory moves atomically (`tokio::fs::rename`
    /// is atomic on the same filesystem), preserving cross-table
    /// consistency: `vectors/`, `symbols.db`, `tags.lmdb`, and
    /// `state.json` all land under the archive in one operation.
    /// `<unix_ts_micros>` is the microsecond Unix timestamp at archive
    /// time (clock-skew safe via `unwrap_or_default` on the duration
    /// since `UNIX_EPOCH` — system clocks earlier than 1970 give a
    /// deterministic zero suffix instead of panicking).
    ///
    /// The archive `.archive/` directory itself is created lazily on
    /// first archive (`tokio::fs::create_dir_all` is idempotent).
    ///
    /// # Errors
    ///
    /// Returns [`BranchManagerError::BranchNotFound`] if the branch
    /// directory is absent (nothing to archive).  Returns
    /// [`BranchManagerError::Io`] on any directory-creation or
    /// rename failure (e.g. cross-device rename, permission denied).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::PathBuf;
    /// use ucil_daemon::branch_manager::BranchManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # tokio::runtime::Runtime::new()?.block_on(async {
    /// let mgr = BranchManager::new(PathBuf::from("/repo/.ucil/branches"));
    /// // assume "feat/foo" was created earlier via create_branch_table
    /// let archived_at = mgr.archive_branch_table("feat/foo").await?;
    /// let _ = assert!(archived_at.starts_with("/repo/.ucil/branches/.archive"));
    /// # Ok::<(), ucil_daemon::branch_manager::BranchManagerError>(())
    /// # })?;
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(
        name = "ucil.daemon.branch_manager.archive",
        level = "debug",
        skip(self),
        fields(branch = name),
    )]
    pub async fn archive_branch_table(&self, name: &str) -> Result<PathBuf, BranchManagerError> {
        let sanitised = sanitise_branch_name(name);
        let source = self.branches_root.join(&sanitised);
        if !tokio::fs::try_exists(&source).await? {
            return Err(BranchManagerError::BranchNotFound {
                name: sanitised,
                path: source,
            });
        }
        let ts_micros = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or_default();
        tokio::fs::create_dir_all(&self.archive_root).await?;
        let target = self.archive_root.join(format!("{sanitised}-{ts_micros}"));
        tokio::fs::rename(&source, &target).await?;
        Ok(target)
    }
}

/// Recursively copy `src` to `dst`, creating directories as needed
/// and skipping anything that is not a regular file or directory
/// (symlinks are dropped per master-plan §11.4 line 1090 — branch
/// trees are pure file/directory hierarchies).
///
/// `Box::pin` is required because Rust's async fn does not natively
/// support recursive calls (the future would have an infinite size);
/// pinning to the heap breaks the cycle.
fn copy_dir_recursive<'a>(
    src: &'a Path,
    dst: &'a Path,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), BranchManagerError>> + Send + 'a>>
{
    Box::pin(async move {
        tokio::fs::create_dir_all(dst).await?;
        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            let ftype = entry.file_type().await?;
            if ftype.is_dir() {
                copy_dir_recursive(&src_path, &dst_path).await?;
            } else if ftype.is_file() {
                tokio::fs::copy(&src_path, &dst_path).await?;
            }
            // Skip symlinks / sockets / fifos — branch trees are pure
            // file/directory hierarchies (master-plan §11.4 line 1090).
        }
        Ok(())
    })
}

// ── Tests ────────────────────────────────────────────────────────────────
//
// `test_lancedb_per_branch` lives at module root (NOT inside `mod
// tests {}`) per DEC-0007: the frozen P2-W7-F09 selector
// `branch_manager::test_lancedb_per_branch` resolves to
// `ucil_daemon::branch_manager::test_lancedb_per_branch`, which only
// matches if the function is at module root.  Same placement
// convention as `storage::test_two_tier_layout` at `storage.rs:267`.

/// Walk a directory up to `depth` levels and return entries as
/// "path/relative/to/root [F|D]" strings.  Used only inside
/// `test_lancedb_per_branch` panic messages to surface the actual
/// filesystem state when an assertion fails — improves operator-
/// readable diagnostics per WO-0051 lessons line 405 without inflating
/// the test body.
#[cfg(test)]
fn walk_dir(p: &Path) -> Vec<String> {
    fn recurse(root: &Path, p: &Path, depth: usize, out: &mut Vec<String>) {
        if depth == 0 {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(p) {
            for entry in entries.flatten() {
                let path = entry.path();
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                let kind = if path.is_dir() { "D" } else { "F" };
                out.push(format!("{rel} [{kind}]"));
                if path.is_dir() {
                    recurse(root, &path, depth - 1, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    recurse(p, p, 3, &mut out);
    out
}

#[cfg(test)]
#[tokio::test]
#[allow(clippy::too_many_lines)] // 5 sub-assertions in one test body — splitting would either duplicate setup or break the in-order lifecycle invariants the test asserts.
async fn test_lancedb_per_branch() {
    let tmp = tempfile::TempDir::new().expect("tmpdir");
    let branches_root = tmp.path().join("branches");
    tokio::fs::create_dir(&branches_root)
        .await
        .expect("mkdir branches");
    let mgr = BranchManager::new(&branches_root);

    // ── (SA1) Create root branch + open table ──────────────────────────
    let info = mgr
        .create_branch_table("main", None)
        .await
        .expect("create main");
    assert_eq!(
        info.branch, "main",
        "sanitised name preserved on no-slash input; got {:?}",
        info.branch
    );
    assert!(
        branches_root.join("main/vectors").exists(),
        "vectors dir must exist; tree dump: {:?}",
        walk_dir(&branches_root)
    );
    let conn = lancedb::connect(branches_root.join("main/vectors").to_str().unwrap())
        .execute()
        .await
        .expect("lancedb connect");
    let tables = conn.table_names().execute().await.expect("list tables");
    assert!(
        tables.iter().any(|t| t == "code_chunks"),
        "code_chunks must be present; got {tables:?}"
    );
    assert_eq!(
        info.table_count, 1,
        "fresh branch has exactly 1 table; got {}",
        info.table_count
    );

    // ── (SA2) Clone-from-parent ────────────────────────────────────────
    let info2 = mgr
        .create_branch_table("feat/foo", Some("main"))
        .await
        .expect("clone child");
    assert_eq!(
        info2.branch, "feat-foo",
        "slash-sanitisation applied; got {:?}",
        info2.branch
    );
    assert!(
        branches_root.join("feat-foo/vectors").exists(),
        "child vectors dir must exist; tree dump: {:?}",
        walk_dir(&branches_root)
    );
    let conn2 = lancedb::connect(branches_root.join("feat-foo/vectors").to_str().unwrap())
        .execute()
        .await
        .expect("lancedb child connect");
    let tables2 = conn2
        .table_names()
        .execute()
        .await
        .expect("list child tables");
    assert!(
        tables2.iter().any(|t| t == "code_chunks"),
        "clone preserved code_chunks; got {tables2:?}"
    );

    // ── (SA3) Sanitisation invariant ───────────────────────────────────
    assert!(
        !branches_root.join("feat/foo").exists(),
        "raw slashed path must not exist as a literal directory; got listing {:?}",
        walk_dir(&branches_root)
    );

    // ── (SA4) Archive roundtrip ────────────────────────────────────────
    let archive_path = mgr.archive_branch_table("feat/foo").await.expect("archive");
    assert!(
        !branches_root.join("feat-foo").exists(),
        "original branch dir gone after archive; tree={:?}",
        walk_dir(&branches_root)
    );
    assert!(
        branches_root.join(".archive").exists(),
        "archive root created; tree={:?}",
        walk_dir(&branches_root)
    );
    let archive_entries: Vec<String> = std::fs::read_dir(branches_root.join(".archive"))
        .expect("read .archive")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        archive_entries.iter().any(|n| n.starts_with("feat-foo-")),
        "archive contains feat-foo-<ts> entry; got {archive_entries:?}"
    );
    assert!(
        archive_path.starts_with(&branches_root),
        "returned path under branches_root; got {archive_path:?}"
    );

    // ── (SA5) Archive-side connectability ──────────────────────────────
    let archived_vectors = archive_path.join("vectors");
    assert!(
        archived_vectors.exists(),
        "archived vectors dir present; archive_path={archive_path:?}"
    );
    let conn3 = lancedb::connect(archived_vectors.to_str().unwrap())
        .execute()
        .await
        .expect("lancedb archive connect");
    let tables3 = conn3
        .table_names()
        .execute()
        .await
        .expect("list archive tables");
    assert!(
        tables3.iter().any(|t| t == "code_chunks"),
        "archive preserves code_chunks for forensic queries; got {tables3:?}"
    );
}
