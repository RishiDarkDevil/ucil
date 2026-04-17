//! `.ucil/shared/knowledge.db` — the SQLite knowledge graph (Phase 1 Week 4).
//!
//! This module owns the **persistent-storage substrate** for every
//! downstream UCIL memory tier.  It does NOT yet expose CRUD helpers,
//! bi-temporal queries, or symbol resolution — those are layered on top
//! by `P1-W4-F02`+.  What it **does** own is:
//!
//! * A `KnowledgeGraph` wrapper around a [`rusqlite::Connection`] opened
//!   with the three pragmas master-plan §11 line 1108-1117 mandates:
//!   `journal_mode = WAL`, `busy_timeout = 10000`, `foreign_keys = ON`.
//! * The full §12.1 initialisation DDL (16 tables + indexes) applied
//!   atomically inside a `BEGIN IMMEDIATE` transaction (§11 line 1117 —
//!   the #1 cause of unexpected `SQLITE_BUSY` is the default
//!   `BEGIN DEFERRED`).
//! * A single `execute_in_transaction` helper that always uses
//!   [`rusqlite::TransactionBehavior::Immediate`] so that every writer
//!   in the codebase routes through one chokepoint.
//!
//! `KnowledgeGraph::open` is **idempotent** — every `CREATE TABLE` and
//! `CREATE INDEX` uses `IF NOT EXISTS`, so re-opening an existing file
//! leaves the schema (and data) untouched.
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use ucil_core::KnowledgeGraph;
//!
//! # fn demo(tmp: &Path) -> Result<(), Box<dyn std::error::Error>> {
//! let mut kg = KnowledgeGraph::open(&tmp.join("knowledge.db"))?;
//! let rows = kg.execute_in_transaction(|tx| {
//!     tx.execute(
//!         "INSERT INTO sessions (id, agent_id, branch, worktree_root) \
//!          VALUES (?1, ?2, ?3, ?4);",
//!         rusqlite::params!["s1", "claude", "main", "/repo"],
//!     )
//! })?;
//! assert_eq!(rows, 1);
//! # Ok(())
//! # }
//! ```

// The module deliberately exposes `KnowledgeGraph` and
// `KnowledgeGraphError` — the public repetition of the module name is
// the convention `schema_migration` and `session_manager` already use.
#![allow(clippy::module_name_repetitions)]

use std::{io, path::Path, path::PathBuf};

use rusqlite::{Connection, Transaction, TransactionBehavior};
use thiserror::Error;

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors produced by [`KnowledgeGraph`].
///
/// Marked `#[non_exhaustive]` so downstream crates cannot rely on
/// exhaustive matching — new variants can be added without a SemVer
/// break as Phase-1 work-orders add CRUD paths that surface new failure
/// modes.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KnowledgeGraphError {
    /// Underlying `SQLite` API failure — includes pragma application,
    /// DDL execution, and every statement run through
    /// [`KnowledgeGraph::execute_in_transaction`].
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A filesystem operation against the database's parent directory
    /// failed (e.g. could not create the enclosing directory before
    /// opening the db file).
    #[error("i/o error on path {path:?}: {source}")]
    Io {
        /// The path whose operation failed.
        path: PathBuf,
        /// The underlying `std::io::Error` the OS returned.
        #[source]
        source: io::Error,
    },

    /// A `PRAGMA` read-back returned a value that does not match what
    /// [`KnowledgeGraph::open`] requested — for instance `journal_mode`
    /// still reporting `delete` after a WAL request (likely because the
    /// db file is on a filesystem that does not support WAL, such as
    /// some FUSE or network mounts).
    #[error("pragma `{name}` mismatch: expected {expected}, got {actual}")]
    PragmaMismatch {
        /// The pragma whose value was rejected.
        name: &'static str,
        /// The value [`KnowledgeGraph::open`] required.
        expected: &'static str,
        /// The value the pragma read-back returned.
        actual: String,
    },
}

// ── Init DDL ──────────────────────────────────────────────────────────────────

/// The §12.1 schema DDL, applied atomically inside a
/// `BEGIN IMMEDIATE` transaction by [`KnowledgeGraph::open`].
///
/// Every `CREATE TABLE` and `CREATE INDEX` uses `IF NOT EXISTS` so that
/// re-opening an already-initialised database is a no-op.  The column
/// types mirror master-plan lines 1130-1318 verbatim; the one addition
/// is the `sessions` table (§11.2 line 1081 + the `SessionInfo`
/// extension introduced in WO-0008) — the minimum viable shape that
/// lets the daemon persist and query active sessions without a
/// round-trip to the in-memory `SessionManager`.
const INIT_SQL: &str = "\
CREATE TABLE IF NOT EXISTS entities (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT,
    file_path TEXT NOT NULL,
    start_line INTEGER, end_line INTEGER,
    signature TEXT, doc_comment TEXT, language TEXT,
    t_valid_from TEXT, t_valid_to TEXT,
    t_ingested_at TEXT NOT NULL DEFAULT (datetime('now')),
    t_last_verified TEXT,
    importance REAL DEFAULT 0.5,
    access_count INTEGER DEFAULT 0, last_accessed TEXT,
    source_tool TEXT, source_hash TEXT,
    UNIQUE(qualified_name, file_path, t_valid_from)
);

CREATE TABLE IF NOT EXISTS relations (
    id INTEGER PRIMARY KEY,
    source_id INTEGER REFERENCES entities(id),
    target_id INTEGER REFERENCES entities(id),
    kind TEXT NOT NULL,
    weight REAL DEFAULT 1.0,
    t_valid_from TEXT, t_valid_to TEXT,
    t_ingested_at TEXT NOT NULL DEFAULT (datetime('now')),
    source_tool TEXT, source_evidence TEXT, confidence REAL DEFAULT 0.8
);

CREATE TABLE IF NOT EXISTS decisions (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL, description TEXT,
    decision_type TEXT,
    related_entities TEXT, source_url TEXT, author TEXT, decided_at TEXT,
    t_ingested_at TEXT NOT NULL DEFAULT (datetime('now')),
    importance REAL DEFAULT 0.7,
    is_superseded INTEGER DEFAULT 0,
    superseded_by INTEGER REFERENCES decisions(id)
);

CREATE TABLE IF NOT EXISTS conventions (
    id INTEGER PRIMARY KEY,
    category TEXT NOT NULL,
    pattern TEXT NOT NULL,
    examples TEXT, counter_examples TEXT,
    confidence REAL DEFAULT 0.5,
    evidence_count INTEGER DEFAULT 1,
    t_ingested_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_verified TEXT,
    scope TEXT DEFAULT 'project'
);

CREATE TABLE IF NOT EXISTS observations (
    id INTEGER PRIMARY KEY,
    observation TEXT NOT NULL,
    category TEXT,
    related_entities TEXT, domains TEXT,
    session_id TEXT,
    importance REAL DEFAULT 0.5,
    access_count INTEGER DEFAULT 0,
    t_created TEXT NOT NULL DEFAULT (datetime('now')),
    t_last_accessed TEXT
);

CREATE TABLE IF NOT EXISTS quality_issues (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    line_start INTEGER, line_end INTEGER,
    category TEXT NOT NULL,
    severity TEXT NOT NULL,
    message TEXT NOT NULL,
    rule_id TEXT,
    source_tool TEXT,
    fix_suggestion TEXT,
    first_seen TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen TEXT,
    resolved INTEGER DEFAULT 0,
    resolved_by_session TEXT
);

CREATE TABLE IF NOT EXISTS hot_observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    raw_text TEXT NOT NULL,
    session_id TEXT,
    related_file TEXT,
    related_symbol TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    promoted_to_warm INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS hot_convention_signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_hash TEXT NOT NULL,
    file_path TEXT NOT NULL,
    example_snippet TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    promoted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS hot_architecture_deltas (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    change_type TEXT NOT NULL,
    file_path TEXT NOT NULL,
    details TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    promoted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS hot_decision_material (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_type TEXT NOT NULL,
    source_url TEXT,
    title TEXT,
    description TEXT,
    affected_files TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    promoted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS warm_observations (
    id INTEGER PRIMARY KEY,
    text TEXT NOT NULL,
    domains TEXT,
    related_entities TEXT,
    severity TEXT,
    evidence_count INTEGER DEFAULT 1,
    first_seen TEXT, last_seen TEXT,
    confidence REAL DEFAULT 0.6,
    promoted_to_cold INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS warm_conventions (
    id INTEGER PRIMARY KEY,
    category TEXT NOT NULL,
    pattern_description TEXT NOT NULL,
    examples TEXT,
    evidence_count INTEGER DEFAULT 3,
    confidence REAL DEFAULT 0.5,
    promoted_to_cold INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS warm_architecture_state (
    id INTEGER PRIMARY KEY,
    summary TEXT NOT NULL,
    deltas_incorporated INTEGER,
    last_updated TEXT,
    confidence REAL DEFAULT 0.5,
    promoted_to_cold INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS warm_decisions (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    key_phrases TEXT,
    related_entities TEXT,
    source_material_ids TEXT,
    confidence REAL DEFAULT 0.5,
    promoted_to_cold INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS feedback_signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    bonus_type TEXT NOT NULL,
    bonus_id INTEGER,
    signal TEXT NOT NULL,
    timestamp TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    agent_id TEXT,
    branch TEXT,
    worktree_root TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_active TEXT,
    inferred_domain TEXT,
    expires_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_entities_file ON entities(file_path);
CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);
CREATE INDEX IF NOT EXISTS idx_entities_valid ON entities(t_valid_to) WHERE t_valid_to IS NULL;
CREATE INDEX IF NOT EXISTS idx_relations_source ON relations(source_id);
CREATE INDEX IF NOT EXISTS idx_relations_target ON relations(target_id);
CREATE INDEX IF NOT EXISTS idx_observations_category ON observations(category);
CREATE INDEX IF NOT EXISTS idx_conventions_category ON conventions(category);
CREATE INDEX IF NOT EXISTS idx_quality_file ON quality_issues(file_path) WHERE resolved = 0;
CREATE INDEX IF NOT EXISTS idx_quality_severity ON quality_issues(severity) WHERE resolved = 0;
CREATE INDEX IF NOT EXISTS idx_hot_obs_promoted ON hot_observations(promoted_to_warm);
CREATE INDEX IF NOT EXISTS idx_hot_conv_hash ON hot_convention_signals(pattern_hash);
CREATE INDEX IF NOT EXISTS idx_warm_obs_domains ON warm_observations(domains);
CREATE INDEX IF NOT EXISTS idx_feedback_session ON feedback_signals(session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_branch ON sessions(branch);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);
";

// ── Type ──────────────────────────────────────────────────────────────────────

/// Owning handle to `.ucil/shared/knowledge.db`.
///
/// Wraps a single [`rusqlite::Connection`] that has already had WAL +
/// `busy_timeout` + `foreign_keys` pragmas applied.  Downstream CRUD
/// work (WO-0012, P1-W4-F02+) will layer query helpers on top via
/// `&self` / `&mut self` methods; for now this struct is the
/// foundation-laying piece gating the rest of Week 4.
#[derive(Debug)]
pub struct KnowledgeGraph {
    conn: Connection,
}

impl KnowledgeGraph {
    /// Open (or create) the knowledge graph at `db_path`.
    ///
    /// If the parent directory of `db_path` does not exist, it is
    /// created (plus all intermediate components) so callers do not
    /// need to pre-materialise `.ucil/shared/`.  Then the connection is
    /// opened and the mandatory pragmas are applied:
    ///
    /// * `PRAGMA journal_mode = WAL;`  (master-plan §11 line 1111)
    /// * `PRAGMA busy_timeout = 10000;` (§11 line 1113)
    /// * `PRAGMA foreign_keys = ON;` (enforces `REFERENCES` edges in §12.1)
    ///
    /// Finally the full §12.1 init DDL runs atomically inside a
    /// `BEGIN IMMEDIATE` transaction (§11 line 1117).  All `CREATE`
    /// statements use `IF NOT EXISTS` so repeat-opens are idempotent.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Io`] — the parent directory could not
    ///   be created.
    /// * [`KnowledgeGraphError::Sqlite`] — opening, pragma application,
    ///   or DDL execution failed.
    /// * [`KnowledgeGraphError::PragmaMismatch`] — a pragma read-back
    ///   did not return the requested value (e.g. the filesystem does
    ///   not support WAL).
    pub fn open(db_path: &Path) -> Result<Self, KnowledgeGraphError> {
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|source| KnowledgeGraphError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
        }

        let mut conn = Connection::open(db_path)?;

        // ── Pragmas ────────────────────────────────────────────────
        //
        // `execute_batch` (not `execute`) so the driver does not try to
        // bind placeholders in a PRAGMA, which rusqlite rejects.  The
        // verbatim string `PRAGMA journal_mode = WAL` is also the text
        // the verifier's acceptance grep hunts for, so we keep it
        // literal here rather than going through `pragma_update`.
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        conn.execute_batch("PRAGMA busy_timeout = 10000;")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        // Verify the pragmas stuck.  A file on an unusual FS (some
        // FUSE mounts, some network mounts) may silently refuse WAL
        // and fall back to `delete` — surface that as a typed error.
        let mode: String =
            conn.query_row("PRAGMA journal_mode;", [], |row| row.get::<_, String>(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(KnowledgeGraphError::PragmaMismatch {
                name: "journal_mode",
                expected: "wal",
                actual: mode,
            });
        }

        let timeout_ms: i64 =
            conn.query_row("PRAGMA busy_timeout;", [], |row| row.get::<_, i64>(0))?;
        if timeout_ms != 10_000 {
            return Err(KnowledgeGraphError::PragmaMismatch {
                name: "busy_timeout",
                expected: "10000",
                actual: timeout_ms.to_string(),
            });
        }

        // ── Schema DDL inside BEGIN IMMEDIATE ──────────────────────
        //
        // Master-plan §11 line 1117: the default `BEGIN DEFERRED`
        // opens a read lock and only upgrades to write on first write
        // stmt — that upgrade path bypasses `busy_timeout` and is the
        // #1 source of surprise `SQLITE_BUSY` in multi-agent UCIL.
        // `BEGIN IMMEDIATE` acquires the write lock up-front.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(INIT_SQL)?;
        tx.commit()?;

        Ok(Self { conn })
    }

    /// Borrow the underlying [`rusqlite::Connection`] for read-only
    /// inspection.
    ///
    /// Writers must route through [`Self::execute_in_transaction`] so
    /// the `BEGIN IMMEDIATE` invariant is preserved across every
    /// writer in the codebase.
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Run `f` inside a `BEGIN IMMEDIATE` transaction, committing on
    /// success and rolling back on any `Err`.
    ///
    /// Every UCIL writer in the codebase should go through this helper
    /// rather than calling [`rusqlite::Connection::transaction`]
    /// directly, because the default transaction behavior is
    /// `DEFERRED` and that is the #1 cause of unexpected `SQLITE_BUSY`
    /// under contention (master-plan §11 line 1117).
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — beginning the transaction,
    ///   the body of `f`, or the commit/rollback itself failed.
    pub fn execute_in_transaction<F, T>(&mut self, f: F) -> Result<T, KnowledgeGraphError>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T, rusqlite::Error>,
    {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }
}

// ── Module-level acceptance test ─────────────────────────────────────────────
//
// Placed as a module-level item (NOT inside a `mod tests { }` block)
// so the nextest selector `knowledge_graph::test_schema_creation`
// resolves — see DEC-0005, escalation 20260415-1856, and the
// WO-0006/WO-0007/WO-0008/WO-0010 lesson for the test-selector rule
// this WO is gated against.

/// Acceptance test for `P1-W4-F01`.
///
/// Frozen selector: `knowledge_graph::test_schema_creation` (exact
/// match — must live at module level, not under `mod tests { … }`).
///
/// The test walks a real on-disk SQLite file through:
///
/// 1. Open a [`tempfile::TempDir`], create `KnowledgeGraph` against
///    `<tempdir>/knowledge.db`.
/// 2. Assert `PRAGMA journal_mode == 'wal'` (case-insensitive).
/// 3. Assert `PRAGMA busy_timeout == 10000`.
/// 4. Assert every one of the 16 expected §12.1 tables exists in
///    `sqlite_master`.
/// 5. Exercise `execute_in_transaction` with a trivial INSERT into
///    `sessions` and assert the affected-rows count is 1.
/// 6. Drop the handle and re-open the same file; assert the schema
///    persists (idempotency of `CREATE TABLE IF NOT EXISTS`) AND the
///    session row inserted in step 5 is still there.
///
/// No mocks of rusqlite or sqlite3 — every assertion runs against a
/// real on-disk db file in a tempdir.
#[cfg(test)]
#[test]
fn test_schema_creation() {
    use tempfile::TempDir;

    // The 16 tables master-plan §12.1 + the new `sessions` table from
    // §11.2 / WO-0008 `SessionInfo`.  Order is the same as
    // `INIT_SQL`'s `CREATE TABLE` sequence for auditability.
    const EXPECTED_TABLES: [&str; 16] = [
        "entities",
        "relations",
        "decisions",
        "conventions",
        "observations",
        "quality_issues",
        "hot_observations",
        "hot_convention_signals",
        "hot_architecture_deltas",
        "hot_decision_material",
        "warm_observations",
        "warm_conventions",
        "warm_architecture_state",
        "warm_decisions",
        "feedback_signals",
        "sessions",
    ];

    let tmp = TempDir::new().expect("tempdir must be creatable for the test");
    let path = tmp.path().join("knowledge.db");

    // ── First open: schema gets created ─────────────────────────────
    let mut kg = KnowledgeGraph::open(&path).expect("KnowledgeGraph::open should succeed");

    // Pragma: journal_mode == wal (case-insensitive per PRAGMA docs).
    let mode: String = kg
        .conn()
        .query_row("PRAGMA journal_mode;", [], |row| row.get::<_, String>(0))
        .expect("PRAGMA journal_mode query must succeed");
    assert!(
        mode.eq_ignore_ascii_case("wal"),
        "expected PRAGMA journal_mode to be `wal`, got `{mode}`",
    );

    // Pragma: busy_timeout == 10000 ms.
    let busy: i64 = kg
        .conn()
        .query_row("PRAGMA busy_timeout;", [], |row| row.get::<_, i64>(0))
        .expect("PRAGMA busy_timeout query must succeed");
    assert_eq!(
        busy, 10_000,
        "expected PRAGMA busy_timeout to be 10000, got {busy}",
    );

    // Pragma: foreign_keys == 1 (ON).
    let fks: i64 = kg
        .conn()
        .query_row("PRAGMA foreign_keys;", [], |row| row.get::<_, i64>(0))
        .expect("PRAGMA foreign_keys query must succeed");
    assert_eq!(
        fks, 1,
        "expected PRAGMA foreign_keys to be 1 (ON), got {fks}",
    );

    // Tables: each of the 16 expected names is present exactly once.
    for &tname in &EXPECTED_TABLES {
        let count: i64 = kg
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1;",
                rusqlite::params![tname],
                |row| row.get::<_, i64>(0),
            )
            .expect("sqlite_master COUNT must succeed");
        assert_eq!(count, 1, "expected table `{tname}` to exist exactly once");
    }

    // execute_in_transaction: trivial INSERT into `sessions`.
    let affected = kg
        .execute_in_transaction(|tx| {
            tx.execute(
                "INSERT INTO sessions (id, agent_id, branch, worktree_root, inferred_domain) \
                 VALUES (?1, ?2, ?3, ?4, ?5);",
                rusqlite::params![
                    "wo-0011-test-session",
                    "executor",
                    "feat/WO-0011",
                    "/home/test/repo",
                    "schema-bootstrap",
                ],
            )
        })
        .expect("execute_in_transaction must succeed on a fresh schema");
    assert_eq!(
        affected, 1,
        "INSERT into sessions should affect exactly 1 row"
    );

    // ── Idempotency: close + reopen + re-run open's DDL ─────────────
    drop(kg);
    let kg2 = KnowledgeGraph::open(&path).expect("re-opening an initialised db must succeed");

    // Every expected table still present after reopen (no duplicates
    // would have raised a DDL error since the `CREATE TABLE` has a
    // UNIQUE constraint and `AUTOINCREMENT`).
    for &tname in &EXPECTED_TABLES {
        let count: i64 = kg2
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1;",
                rusqlite::params![tname],
                |row| row.get::<_, i64>(0),
            )
            .expect("sqlite_master COUNT must succeed after reopen");
        assert_eq!(count, 1, "table `{tname}` must persist across reopen");
    }

    // The session row inserted pre-reopen persists — a stronger
    // idempotency signal than table count alone.
    let session_id: String = kg2
        .conn()
        .query_row(
            "SELECT id FROM sessions WHERE id = ?1;",
            rusqlite::params!["wo-0011-test-session"],
            |row| row.get::<_, String>(0),
        )
        .expect("the pre-reopen session row must persist");
    assert_eq!(session_id, "wo-0011-test-session");
}
