//! Schema version stamping and downgrade guard for `.ucil/state.db`.
//!
//! In Phase 0 the migration runner has exactly two responsibilities:
//! 1. **Stamp** the current [`SCHEMA_VERSION`] into the database on every init.
//! 2. **Refuse** to start if the database contains a version string that is
//!    *newer* than [`SCHEMA_VERSION`] (i.e. a downgrade scenario).
//!
//! No actual schema DDL migrations ship in Phase 0; those begin in later phases.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension as _};
use thiserror::Error;

// ── Version constant ──────────────────────────────────────────────────────────

/// Current schema version.  Stamped into `.ucil/state.db` on every [`stamp_version`] call.
pub const SCHEMA_VERSION: &str = "1.0.0";

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors that can occur during schema version management.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MigrationError {
    /// The database records a schema version that is *newer* than the version
    /// supported by this binary.  Running would risk data corruption.
    #[error(
        "schema downgrade not supported: database has version {stamped}, \
         binary supports {binary}"
    )]
    Downgrade {
        /// The version string stamped in the database.
        stamped: String,
        /// The version string the running binary expects.
        binary: String,
    },

    /// A `SQLite` I/O or API error.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Stamps [`SCHEMA_VERSION`] into the database at `db_path`.
///
/// Creates the `schema_versions` table if it does not yet exist, then inserts
/// a row containing [`SCHEMA_VERSION`] and the current UTC timestamp
/// (`datetime('now')` in `SQLite`).
///
/// # Errors
///
/// Returns [`MigrationError::Sqlite`] if the database cannot be opened or any
/// SQL statement fails.
pub fn stamp_version(db_path: &Path) -> Result<(), MigrationError> {
    let conn = Connection::open(db_path)?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_versions (
             id         INTEGER PRIMARY KEY AUTOINCREMENT,
             version    TEXT    NOT NULL,
             stamped_at TEXT    NOT NULL DEFAULT (datetime('now'))
         );",
    )?;

    conn.execute(
        "INSERT INTO schema_versions (version, stamped_at) \
         VALUES (?1, datetime('now'));",
        params![SCHEMA_VERSION],
    )?;

    Ok(())
}

/// Checks that the latest stamped version in `db_path` is not newer than
/// [`SCHEMA_VERSION`].
///
/// Version comparison is lexicographic; this is intentional because semver
/// strings of the form `MAJOR.MINOR.PATCH` sort correctly under ASCII
/// lexicographic ordering for versions with the same number of digits per
/// component.  If the `schema_versions` table does not exist or is empty,
/// this function returns `Ok(())` without error.
///
/// # Errors
///
/// Returns [`MigrationError::Downgrade`] when the latest stamped version is
/// lexicographically greater than [`SCHEMA_VERSION`].  Returns
/// [`MigrationError::Sqlite`] on any database I/O failure.
pub fn check_version(db_path: &Path) -> Result<(), MigrationError> {
    let conn = Connection::open(db_path)?;

    // Detect whether the table exists at all.
    let table_exists: bool = conn.query_row(
        "SELECT COUNT(*) \
         FROM   sqlite_master \
         WHERE  type = 'table' \
         AND    name = 'schema_versions';",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;

    if !table_exists {
        return Ok(());
    }

    // Read the most recently stamped version.
    let stamped: Option<String> = conn
        .query_row(
            "SELECT version FROM schema_versions ORDER BY id DESC LIMIT 1;",
            [],
            |row| row.get(0),
        )
        .optional()?;

    match stamped {
        None => Ok(()),
        Some(v) if v.as_str() <= SCHEMA_VERSION => Ok(()),
        Some(v) => Err(MigrationError::Downgrade {
            stamped: v,
            binary: SCHEMA_VERSION.to_owned(),
        }),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{check_version, stamp_version, MigrationError, SCHEMA_VERSION};

    fn temp_db() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("state.db");
        (dir, path)
    }

    #[test]
    fn stamp_creates_table_and_row() {
        let (_dir, path) = temp_db();

        stamp_version(&path).expect("stamp should succeed");

        // Verify the row landed in the database.
        let conn = rusqlite::Connection::open(&path).expect("open db");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_versions WHERE version = ?1;",
                rusqlite::params![SCHEMA_VERSION],
                |r| r.get(0),
            )
            .expect("count query");
        assert_eq!(count, 1, "exactly one row should exist after one stamp");
    }

    #[test]
    fn check_returns_ok_after_stamp() {
        let (_dir, path) = temp_db();

        stamp_version(&path).expect("stamp");
        check_version(&path).expect("check after stamp should return Ok");
    }

    #[test]
    fn check_returns_ok_on_empty_db() {
        let (_dir, path) = temp_db();

        // No stamp — table doesn't exist yet.
        check_version(&path).expect("check on fresh db should return Ok");
    }

    #[test]
    fn check_returns_downgrade_error_for_future_version() {
        let (_dir, path) = temp_db();

        // Manually stamp a version string that sorts after SCHEMA_VERSION.
        let conn = rusqlite::Connection::open(&path).expect("open");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_versions (
                 id         INTEGER PRIMARY KEY AUTOINCREMENT,
                 version    TEXT    NOT NULL,
                 stamped_at TEXT    NOT NULL DEFAULT (datetime('now'))
             );",
        )
        .expect("create table");
        conn.execute(
            "INSERT INTO schema_versions (version, stamped_at) VALUES ('9.9.9', datetime('now'));",
            [],
        )
        .expect("insert future version");
        drop(conn);

        let err = check_version(&path).expect_err("should return Downgrade error");
        assert!(
            matches!(err, MigrationError::Downgrade { .. }),
            "expected Downgrade, got: {err}"
        );
    }

    #[test]
    fn stamp_is_idempotent_in_rows() {
        let (_dir, path) = temp_db();

        stamp_version(&path).expect("first stamp");
        stamp_version(&path).expect("second stamp");

        let conn = rusqlite::Connection::open(&path).expect("open");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_versions;", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, 2, "each stamp adds a new row");
    }
}
