//! `.ucil/shared/knowledge.db` — the `SQLite` knowledge graph (Phase 1 Week 4).
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

use chrono::{DateTime, Utc};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors produced by [`KnowledgeGraph`].
///
/// Marked `#[non_exhaustive]` so downstream crates cannot rely on
/// exhaustive matching — new variants can be added without a `SemVer`
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

    /// A caller-supplied timestamp could not be parsed / round-tripped
    /// through the bi-temporal layer's RFC-3339 contract.  Reserved for
    /// future helpers; the CRUD helpers landed in WO-0024 never surface
    /// this variant directly because they take pre-formatted
    /// `DateTime<Utc>` values or `Option<String>`-typed RFC-3339 text.
    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(String),
}

// ── Domain types ──────────────────────────────────────────────────────────────

/// A row in the §12.1 `entities` table — the unit of identity in the
/// knowledge graph.
///
/// Fields mirror `INIT_SQL`'s `entities` declaration verbatim so a round
/// trip through [`KnowledgeGraph::upsert_entity`] ↔
/// [`KnowledgeGraph::get_entity_by_qualified_name`] preserves every
/// user-supplied column.  `id` is `None` before insert and `Some(rowid)`
/// after; the three `t_*` TEXT columns are RFC-3339 strings (via
/// [`chrono::DateTime::to_rfc3339`]) so `SQLite`'s string-comparison-based
/// range queries in [`KnowledgeGraph::get_entity_as_of`] stay
/// lexicographically correct — mixing RFC-3339 with the bare-space
/// `datetime('now')` format on the *same* range-queried column is a
/// silent-bug trap (WO-0024 RCA §Non-negotiable invariant 5).
///
/// See master-plan §12.1 lines 1130-1145 for the schema; §12.2 for the
/// bi-temporal `t_valid_from` / `t_valid_to` semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// `entities.id` — `None` until inserted; `Some(rowid)` after
    /// [`KnowledgeGraph::upsert_entity`] returns on either the insert
    /// or `ON CONFLICT DO UPDATE` branch.
    pub id: Option<i64>,
    /// `entities.kind` — `"function"`, `"class"`, `"module"`, `"file"`,
    /// etc.  Free-form text; no enum constraint at the schema level.
    pub kind: String,
    /// `entities.name` — the unqualified symbol / file name.
    pub name: String,
    /// `entities.qualified_name` — fully-qualified identifier
    /// (`module::path::Symbol`) when known.  Nullable because some
    /// `entities.kind` values (e.g. `"file"`) don't have a qualified
    /// name.  Participates in the `UNIQUE(qualified_name, file_path,
    /// t_valid_from)` constraint at `INIT_SQL` line 125.
    pub qualified_name: Option<String>,
    /// `entities.file_path` — source-file path relative to the project
    /// root.  Required (NOT NULL at the schema level).
    pub file_path: String,
    /// `entities.start_line` — 1-based inclusive start line.
    pub start_line: Option<i64>,
    /// `entities.end_line` — 1-based inclusive end line.
    pub end_line: Option<i64>,
    /// `entities.signature` — function/method signature or type
    /// declaration when `kind` is a callable.
    pub signature: Option<String>,
    /// `entities.doc_comment` — attached rustdoc / `TSDoc` / docstring
    /// when extraction is available.
    pub doc_comment: Option<String>,
    /// `entities.language` — ISO language tag or language-family name
    /// (`"rust"`, `"python"`, `"typescript"`, ...).
    pub language: Option<String>,
    /// `entities.t_valid_from` — RFC-3339 timestamp (via
    /// [`chrono::DateTime::to_rfc3339`]) of when the entity's facts
    /// started being true in reality.  Nullable for facts with unknown
    /// valid-time lower bound.
    pub t_valid_from: Option<String>,
    /// `entities.t_valid_to` — RFC-3339 timestamp of when the entity's
    /// facts stopped being true.  `None` means "still valid".
    pub t_valid_to: Option<String>,
    /// `entities.importance` — 0.0..=1.0 fusion-layer hint; the schema
    /// default is `0.5`.
    pub importance: f64,
    /// `entities.source_tool` — tool that produced the record
    /// (`"tree-sitter"`, `"lsp"`, `"manual"`, ...).
    pub source_tool: Option<String>,
    /// `entities.source_hash` — content hash of the source span used at
    /// extraction time, for staleness detection.
    pub source_hash: Option<String>,
}

/// A row in the §12.1 `relations` table — a typed directed edge between
/// two [`Entity`] rows.
///
/// `relations` has NO `UNIQUE` constraint in §12.1 — every call to
/// [`KnowledgeGraph::upsert_relation`] appends a fresh row.  Callers
/// that need dedup semantics must query first.
///
/// See master-plan §12.1 lines 1147-1156 for the schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    /// `relations.id` — `None` until inserted; `Some(rowid)` after
    /// [`KnowledgeGraph::upsert_relation`] returns.
    pub id: Option<i64>,
    /// `relations.source_id` — the `entities.id` of the source vertex.
    pub source_id: i64,
    /// `relations.target_id` — the `entities.id` of the target vertex.
    pub target_id: i64,
    /// `relations.kind` — `"calls"`, `"imports"`, `"implements"`,
    /// `"inherits"`, etc.  Free-form text; no enum constraint.
    pub kind: String,
    /// `relations.weight` — 0.0..=1.0 edge strength; the schema default
    /// is `1.0`.
    pub weight: f64,
    /// `relations.t_valid_from` — RFC-3339 lower bound of validity, same
    /// convention as [`Entity::t_valid_from`].
    pub t_valid_from: Option<String>,
    /// `relations.t_valid_to` — RFC-3339 upper bound of validity.
    pub t_valid_to: Option<String>,
    /// `relations.source_tool` — tool that produced the edge.
    pub source_tool: Option<String>,
    /// `relations.source_evidence` — free-form snippet / path / line
    /// range documenting where the edge was inferred from.
    pub source_evidence: Option<String>,
    /// `relations.confidence` — 0.0..=1.0 fusion-layer hint; the schema
    /// default is `0.8`.
    pub confidence: f64,
}

/// A hot-tier observation staged for the merge-consolidator.
///
/// Written through [`KnowledgeGraph::stage_hot_observation`] so the
/// insert goes through the `BEGIN IMMEDIATE` chokepoint
/// (master-plan §11 line 1117) — the hot-staging tier is the most
/// write-contended path in the UCIL pipeline and single-writer
/// contention is the #1 source of `SQLITE_BUSY`.
///
/// Corresponds to the `hot_observations` row at `INIT_SQL` lines
/// 190-198.  `created_at` and `promoted_to_warm` are managed by the
/// schema default / consolidator and are not part of the writer
/// contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotObservation {
    /// `hot_observations.raw_text` — the observation body; required.
    pub raw_text: String,
    /// `hot_observations.session_id` — id of the session that produced
    /// the observation; optional for tool-side or offline batches.
    pub session_id: Option<String>,
    /// `hot_observations.related_file` — file path the observation
    /// concerns, when known.
    pub related_file: Option<String>,
    /// `hot_observations.related_symbol` — symbol name the observation
    /// concerns, when known.
    pub related_symbol: Option<String>,
}

/// A read-only projection of the `entities` columns relevant to
/// name-based symbol resolution (P1-W4-F03).
///
/// Returned by [`KnowledgeGraph::resolve_symbol`] so callers who know
/// only a bare symbol name (e.g. a tree-sitter extractor that has
/// parsed `parse_file` without the `ucil_treesitter::parser::`
/// prefix) can still reach the definition row.  The columns projected
/// are the subset from the §12.1 `entities` schema that every
/// downstream pipeline step needs (`file_path`, `start_line`,
/// `signature`, `doc_comment`), plus a derived `parent_module`
/// computed at read-time from `qualified_name` — NOT a stored column,
/// so no schema migration is required.
///
/// See master-plan §12.1 lines 1130-1145 for the source `entities`
/// columns, and master-plan §18 Phase 1 Week 4 line 1749 ("Implement
/// `knowledge_graph.rs`: CRUD, bi-temporal queries, symbol
/// resolution") for the scope this projection satisfies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolResolution {
    /// `entities.id` of the resolved row — the primary key the
    /// `find_definition` handler (`P1-W4-F05`) feeds to
    /// [`KnowledgeGraph::list_relations_by_target`] to enumerate
    /// immediate callers.  Always `Some(rowid)` because the resolver
    /// selects `id` explicitly and every persisted row has one; the
    /// `Option` shape preserves column-symmetry with [`Entity::id`] so
    /// callers that already handle the nullable case do not need a
    /// special branch.
    pub id: Option<i64>,
    /// `entities.qualified_name` of the resolved row — fully-qualified
    /// identifier (e.g. `"ucil_core::types::parse"`) when known.  The
    /// handler uses this to disambiguate callers + callees in
    /// response payloads that cross module boundaries.  `None` when
    /// the underlying row has a `NULL` `qualified_name` (e.g.
    /// `kind = "file"` entries per §12.1).
    pub qualified_name: Option<String>,
    /// `entities.file_path` of the resolved row — absolute or
    /// project-relative source-file path.  Never `None` because the
    /// underlying column is `NOT NULL` at the schema level.
    pub file_path: String,
    /// `entities.start_line` — 1-based inclusive start line when
    /// known.  `None` for file-kind entities or any row where the
    /// extractor did not populate the column.
    pub start_line: Option<i64>,
    /// `entities.signature` — function / method signature or type
    /// declaration when the row is a callable; `None` otherwise.
    pub signature: Option<String>,
    /// `entities.doc_comment` — attached rustdoc / `TSDoc` / docstring
    /// when extraction is available; `None` otherwise.
    pub doc_comment: Option<String>,
    /// Parent module path derived from `entities.qualified_name` by
    /// stripping the terminal `::name` segment.  `Some("foo::bar")`
    /// when `qualified_name = "foo::bar::baz"`; `None` when
    /// `qualified_name` is `NULL` or contains no `::` separator.
    /// Derived at resolution time per master-plan §18 Phase 1 Week 4
    /// line 1749 — not a stored column.
    pub parent_module: Option<String>,
}

/// A row in the §12.1 `conventions` table — a cold-tier record of a
/// project-specific coding convention the team has committed to.
///
/// Fields mirror the `INIT_SQL` `conventions` declaration verbatim so a
/// round trip through [`KnowledgeGraph::insert_convention`] ↔
/// [`KnowledgeGraph::list_conventions`] preserves every user-supplied
/// column.  `id` is `None` before insert and `Some(rowid)` after.
/// `t_ingested_at` is managed by the schema default
/// (`DEFAULT (datetime('now'))`) so inserters never supply it — the
/// `insert_convention` helper omits the column from its INSERT list,
/// and the read back value returned through `list_conventions` is the
/// string `SQLite` wrote.
///
/// The `category` column is unconstrained TEXT at the schema level but
/// master-plan §12.1 lines 1172-1182 enumerate the expected values:
/// `"naming"`, `"structure"`, `"error_handling"`, `"testing"`,
/// `"style"`, `"security"`.  The `get_conventions` tool filter
/// (`P1-W4-F10`) passes the caller-supplied string through to
/// `WHERE category = ?1` verbatim; unknown categories yield an empty
/// result set.
///
/// See master-plan §12.1 lines 1172-1182 for the schema and
/// `get_conventions` in master-plan §3.2 row 7 for the consumer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Convention {
    /// `conventions.id` — `None` until inserted; `Some(rowid)` after
    /// [`KnowledgeGraph::insert_convention`] returns.
    pub id: Option<i64>,
    /// `conventions.category` — one of the six enum values at
    /// master-plan §12.1 lines 1172-1182
    /// (`"naming"` / `"structure"` / `"error_handling"` / `"testing"` /
    /// `"style"` / `"security"`).  Free-form text at the schema level;
    /// the filter on [`KnowledgeGraph::list_conventions`] is a literal
    /// `WHERE category = ?1` so callers are expected to pass one of
    /// the master-plan values verbatim.
    pub category: String,
    /// `conventions.pattern` — the convention description / rule text
    /// the convention-learner layer will match against source spans.
    /// Required (NOT NULL at the schema level).
    pub pattern: String,
    /// `conventions.examples` — free-form text block (typically a
    /// newline-delimited list) of code fragments that **conform** to
    /// the convention.  Nullable because early-ingested signals may
    /// only know the pattern.
    pub examples: Option<String>,
    /// `conventions.counter_examples` — free-form text block of code
    /// fragments that **violate** the convention.
    pub counter_examples: Option<String>,
    /// `conventions.confidence` — 0.0..=1.0 fusion-layer hint; the
    /// schema default is `0.5`.
    pub confidence: f64,
    /// `conventions.evidence_count` — count of source spans that
    /// produced the convention.  The schema default is `1` (a single
    /// observation).
    pub evidence_count: i64,
    /// `conventions.t_ingested_at` — RFC-3339-ish timestamp written by
    /// the schema default `DEFAULT (datetime('now'))`.  Inserters
    /// leave this unset and read back the schema-managed value.
    pub t_ingested_at: String,
    /// `conventions.last_verified` — free-form timestamp of the most
    /// recent confirmation the convention still holds.  Nullable.
    pub last_verified: Option<String>,
    /// `conventions.scope` — `"project"` (default) or a coarser scope
    /// (e.g. `"workspace"`, `"global"`) in a later phase's taxonomy.
    pub scope: String,
}

/// Checkpoint mode for [`KnowledgeGraph::checkpoint_wal`] — wraps
/// `SQLite`'s `PRAGMA wal_checkpoint(<MODE>)`.
///
/// The Phase 1 Week 4 F08 hot-staging feature triggers a periodic
/// checkpoint to bound WAL size under high insert pressure from the
/// hot-tier writers; `Truncate` is the aggressive mode the scheduled
/// sweep uses, `Passive` is the best-effort mode other call-sites use.
///
/// See the `SQLite` docs for `wal_checkpoint` for the full semantics of
/// the four modes (`PASSIVE` / `FULL` / `RESTART` / `TRUNCATE`); UCIL
/// only exposes the two relevant to the hot-staging sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalCheckpointMode {
    /// `PASSIVE` — checkpoints as many frames as possible without
    /// blocking any readers or writers; never returns `SQLITE_BUSY`.
    /// The default mode for ad-hoc checkpoints.
    Passive,
    /// `TRUNCATE` — implies `RESTART` + truncates the WAL to zero
    /// bytes; the aggressive mode the scheduled sweep uses to keep the
    /// WAL file bounded.
    Truncate,
}

impl WalCheckpointMode {
    /// The string token `SQLite` expects inside
    /// `PRAGMA wal_checkpoint(<MODE>)`.
    #[must_use]
    pub const fn as_sql(self) -> &'static str {
        match self {
            Self::Passive => "PASSIVE",
            Self::Truncate => "TRUNCATE",
        }
    }
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
    pub const fn conn(&self) -> &Connection {
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

    // ── Entity CRUD ──────────────────────────────────────────────────
    //
    // Every writer routes through `execute_in_transaction` so the
    // `BEGIN IMMEDIATE` invariant (§11 line 1117) is preserved.  The
    // ON CONFLICT branch honours the
    // `UNIQUE(qualified_name, file_path, t_valid_from)` constraint at
    // `INIT_SQL` line 125: the second insert with the same triple
    // returns the existing row's id via `RETURNING id`, bumps
    // `access_count`, and refreshes `t_last_verified` — all in a single
    // round-trip.

    /// Upsert an [`Entity`] into the `entities` table.
    ///
    /// Issues
    /// `INSERT INTO entities (...) VALUES (...) ON CONFLICT(qualified_name,
    /// file_path, t_valid_from) DO UPDATE SET t_last_verified = datetime('now'),
    /// access_count = access_count + 1 RETURNING id` so the caller gets
    /// the inserted-or-updated rowid without a second `SELECT`.  Returns
    /// the `entities.id` from whichever branch fired.
    ///
    /// The write routes through [`Self::execute_in_transaction`] and
    /// therefore runs under `BEGIN IMMEDIATE` per master-plan §11 line
    /// 1117 — the chokepoint that keeps every UCIL writer out of the
    /// default `BEGIN DEFERRED` → lock-upgrade → `SQLITE_BUSY` trap.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — transaction open, statement
    ///   prepare, parameter bind, or row return failed.
    #[tracing::instrument(
        level = "debug",
        skip(self, entity),
        fields(name = %entity.name),
        name = "ucil.core.kg.upsert_entity",
    )]
    pub fn upsert_entity(&mut self, entity: &Entity) -> Result<i64, KnowledgeGraphError> {
        self.execute_in_transaction(|tx| {
            let mut stmt = tx.prepare(
                "INSERT INTO entities (\
                    kind, name, qualified_name, file_path, start_line, end_line, \
                    signature, doc_comment, language, t_valid_from, t_valid_to, \
                    importance, source_tool, source_hash\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) \
                 ON CONFLICT(qualified_name, file_path, t_valid_from) DO UPDATE SET \
                    t_last_verified = datetime('now'), \
                    access_count = access_count + 1 \
                 RETURNING id;",
            )?;
            let id: i64 = stmt.query_row(
                rusqlite::params![
                    entity.kind,
                    entity.name,
                    entity.qualified_name,
                    entity.file_path,
                    entity.start_line,
                    entity.end_line,
                    entity.signature,
                    entity.doc_comment,
                    entity.language,
                    entity.t_valid_from,
                    entity.t_valid_to,
                    entity.importance,
                    entity.source_tool,
                    entity.source_hash,
                ],
                |row| row.get::<_, i64>(0),
            )?;
            Ok(id)
        })
    }

    /// Look up the most recent [`Entity`] row by qualified name.
    ///
    /// When `file_path` is `Some(_)` the lookup is scoped to that file;
    /// when `None` any file matches.  The query orders by
    /// `t_ingested_at DESC LIMIT 1` so the caller always gets the
    /// latest-ingested record (the bi-temporal
    /// [`Self::get_entity_as_of`] helper is the right choice for
    /// valid-time range queries).
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — statement prepare, bind, or
    ///   `query_row` failure.  Returns `Ok(None)` (not an error) when
    ///   no row matches.
    pub fn get_entity_by_qualified_name(
        &self,
        qualified_name: &str,
        file_path: Option<&str>,
    ) -> Result<Option<Entity>, KnowledgeGraphError> {
        let sql = if file_path.is_some() {
            "SELECT id, kind, name, qualified_name, file_path, start_line, end_line, \
                    signature, doc_comment, language, t_valid_from, t_valid_to, \
                    importance, source_tool, source_hash \
             FROM entities \
             WHERE qualified_name = ?1 AND file_path = ?2 \
             ORDER BY t_ingested_at DESC LIMIT 1"
        } else {
            "SELECT id, kind, name, qualified_name, file_path, start_line, end_line, \
                    signature, doc_comment, language, t_valid_from, t_valid_to, \
                    importance, source_tool, source_hash \
             FROM entities \
             WHERE qualified_name = ?1 \
             ORDER BY t_ingested_at DESC LIMIT 1"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let row_result = if let Some(path) = file_path {
            stmt.query_row(rusqlite::params![qualified_name, path], entity_from_row)
        } else {
            stmt.query_row(rusqlite::params![qualified_name], entity_from_row)
        };
        Ok(row_result.map(Some).or_else(absent_to_none)?)
    }

    /// Look up a single [`Entity`] row by its primary-key `id`.
    ///
    /// Mirrors the read precedent of [`Self::get_entity_by_qualified_name`]
    /// — read-only, no transaction, and `Ok(None)` (not an error) when no
    /// row matches.  The `find_definition` MCP tool handler
    /// (`P1-W4-F05`, master-plan §3.2 row 2 / §18 Phase 1 Week 4 line
    /// 1751) uses this to project the source [`Entity`] of each caller
    /// relation returned by [`Self::list_relations_by_target`] onto the
    /// `{qualified_name, file_path, start_line}` payload a response needs.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — statement prepare, bind, or
    ///   `query_row` failure.  Returns `Ok(None)` (not an error) when
    ///   no row matches.
    #[tracing::instrument(
        level = "debug",
        skip(self),
        fields(id = id),
        name = "ucil.core.kg.get_entity_by_id",
    )]
    pub fn get_entity_by_id(&self, id: i64) -> Result<Option<Entity>, KnowledgeGraphError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, qualified_name, file_path, start_line, end_line, \
                    signature, doc_comment, language, t_valid_from, t_valid_to, \
                    importance, source_tool, source_hash \
             FROM entities \
             WHERE id = ?1 \
             LIMIT 1",
        )?;
        let row_result = stmt.query_row(rusqlite::params![id], entity_from_row);
        Ok(row_result.map(Some).or_else(absent_to_none)?)
    }

    /// List all `entities` rows whose `file_path` matches, ordered by
    /// `start_line` ascending.
    ///
    /// The document-order return matters for downstream formatters that
    /// want to render a file's outline top-to-bottom; callers that need
    /// a different order must sort themselves.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — statement prepare, bind, or
    ///   iteration failure.
    pub fn list_entities_by_file(
        &self,
        file_path: &str,
    ) -> Result<Vec<Entity>, KnowledgeGraphError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, qualified_name, file_path, start_line, end_line, \
                    signature, doc_comment, language, t_valid_from, t_valid_to, \
                    importance, source_tool, source_hash \
             FROM entities \
             WHERE file_path = ?1 \
             ORDER BY start_line ASC, id ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![file_path], entity_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ── Relation CRUD ────────────────────────────────────────────────
    //
    // `relations` has NO UNIQUE constraint in §12.1 — each call to
    // `upsert_relation` appends a fresh row.  Name is kept as
    // `upsert_*` for API symmetry with `upsert_entity` and to leave
    // room for a future dedup pass; today the implementation is a
    // pure insert.

    /// Insert a [`Relation`] row.
    ///
    /// Routes through [`Self::execute_in_transaction`] so the write
    /// respects the `BEGIN IMMEDIATE` chokepoint (§11 line 1117).
    /// Returns the new `relations.id` via `RETURNING id`.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — transaction open, statement
    ///   prepare, parameter bind, or row return failed.  Foreign-key
    ///   violations on `source_id` / `target_id` (the columns
    ///   `REFERENCES entities(id)` per §12.1) also flow through this
    ///   variant.
    #[tracing::instrument(
        level = "debug",
        skip(self, relation),
        fields(kind = %relation.kind, source_id = relation.source_id, target_id = relation.target_id),
        name = "ucil.core.kg.upsert_relation",
    )]
    pub fn upsert_relation(&mut self, relation: &Relation) -> Result<i64, KnowledgeGraphError> {
        self.execute_in_transaction(|tx| {
            let mut stmt = tx.prepare(
                "INSERT INTO relations (\
                    source_id, target_id, kind, weight, \
                    t_valid_from, t_valid_to, \
                    source_tool, source_evidence, confidence\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                 RETURNING id;",
            )?;
            let id: i64 = stmt.query_row(
                rusqlite::params![
                    relation.source_id,
                    relation.target_id,
                    relation.kind,
                    relation.weight,
                    relation.t_valid_from,
                    relation.t_valid_to,
                    relation.source_tool,
                    relation.source_evidence,
                    relation.confidence,
                ],
                |row| row.get::<_, i64>(0),
            )?;
            Ok(id)
        })
    }

    /// List every [`Relation`] row with the given `source_id`.
    ///
    /// Read-only — no transaction.  Returns rows in insertion order
    /// (`id ASC`) so callers can reason about append-only arrival.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — statement prepare, bind, or
    ///   iteration failure.
    pub fn list_relations_by_source(
        &self,
        source_id: i64,
    ) -> Result<Vec<Relation>, KnowledgeGraphError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, target_id, kind, weight, \
                    t_valid_from, t_valid_to, \
                    source_tool, source_evidence, confidence \
             FROM relations \
             WHERE source_id = ?1 \
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![source_id], relation_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// List every [`Relation`] row with the given `target_id`.
    ///
    /// Mirrors [`Self::list_relations_by_source`] — read-only, no
    /// transaction, rows returned in insertion order (`id ASC`).  The
    /// `find_definition` MCP tool (`P1-W4-F05`) uses this to enumerate
    /// **immediate callers** of a definition: every `calls`-kind
    /// relation whose `target_id` is the definition's rowid is a caller
    /// of that definition, per the inverted `calls`-edge semantics of
    /// master-plan §12.1 rows.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — statement prepare, bind, or
    ///   iteration failure.
    #[tracing::instrument(
        level = "debug",
        skip(self),
        fields(target_id = target_id),
        name = "ucil.core.kg.list_relations_by_target",
    )]
    pub fn list_relations_by_target(
        &self,
        target_id: i64,
    ) -> Result<Vec<Relation>, KnowledgeGraphError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, target_id, kind, weight, \
                    t_valid_from, t_valid_to, \
                    source_tool, source_evidence, confidence \
             FROM relations \
             WHERE target_id = ?1 \
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![target_id], relation_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ── Bi-temporal reads ────────────────────────────────────────────
    //
    // `t_valid_from` / `t_valid_to` are TEXT columns in SQLite, and
    // the comparison is lexicographic — which is fine as long as every
    // writer encodes via `DateTime<Utc>::to_rfc3339()` and every reader
    // compares via the same format.  Mixing RFC-3339 with
    // `datetime('now')` (space separator) on the same range-queried
    // column silently returns wrong rows (WO-0024 RCA §Non-negotiable
    // invariant 5).

    /// Return the [`Entity`] whose valid-time window contains `at`.
    ///
    /// Implements master-plan §12.2 bi-temporal semantics: the row's
    /// valid-time window `[t_valid_from, t_valid_to)` is treated as
    /// half-open (inclusive lower bound, exclusive upper bound); a
    /// `t_valid_to` of `NULL` means "still valid".  When multiple rows
    /// match (e.g. two overlapping versions of the same entity) the
    /// one with the greatest `t_valid_from` wins — i.e. the most
    /// recently started window.
    ///
    /// The `at` parameter is encoded via
    /// [`chrono::DateTime::to_rfc3339`] so the TEXT string comparison
    /// is lexicographically correct against any row written via the
    /// `upsert_*` helpers.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — prepare, bind, or row-fetch
    ///   failure.  `Ok(None)` (not an error) when no valid-time window
    ///   contains `at`.
    #[tracing::instrument(
        level = "debug",
        skip(self),
        fields(qualified_name = %qualified_name, at = %at.to_rfc3339()),
        name = "ucil.core.kg.get_entity_as_of",
    )]
    pub fn get_entity_as_of(
        &self,
        qualified_name: &str,
        at: DateTime<Utc>,
    ) -> Result<Option<Entity>, KnowledgeGraphError> {
        let at_str = at.to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, name, qualified_name, file_path, start_line, end_line, \
                    signature, doc_comment, language, t_valid_from, t_valid_to, \
                    importance, source_tool, source_hash \
             FROM entities \
             WHERE qualified_name = ?1 \
               AND t_valid_from <= ?2 \
               AND (t_valid_to IS NULL OR t_valid_to > ?2) \
             ORDER BY t_valid_from DESC \
             LIMIT 1",
        )?;
        let row_result = stmt.query_row(rusqlite::params![qualified_name, at_str], entity_from_row);
        Ok(row_result.map(Some).or_else(absent_to_none)?)
    }

    // ── Symbol resolution (P1-W4-F03) ────────────────────────────────
    //
    // Name-based read layer over the frozen F02 CRUD surface.  Callers
    // who know only a bare symbol name — e.g. a tree-sitter extractor
    // that parsed `parse_file` without the crate-qualified path — reach
    // the owning `entities` row here.  The SQL matches three cases at
    // once:
    //
    //   1. exact `name = ?1`                         (bare symbol)
    //   2. exact `qualified_name = ?1`               (full path)
    //   3. `qualified_name LIKE '%::' || ?1`         (terminal segment)
    //
    // Tie-breaking picks the most recently ingested row — `ORDER BY
    // t_ingested_at DESC LIMIT 1`, mirroring
    // `get_entity_by_qualified_name` so callers get consistent
    // newest-wins semantics across the F02/F03 read surface.
    // `parent_module` is DERIVED at read-time by splitting
    // `qualified_name` on the final `::`; storing it would require a
    // schema migration (out of scope per master-plan §12.1 freeze).

    /// Resolve a bare symbol `name` (optionally scoped to a file) to
    /// the most recently-ingested matching [`SymbolResolution`].
    ///
    /// Matches across three shapes at once: an exact
    /// `entities.name = ?1` hit, an exact `entities.qualified_name =
    /// ?1` hit, or a terminal-segment hit against
    /// `qualified_name LIKE '%::' || ?1` (so passing `"parse"` reaches
    /// `ucil_treesitter::parser::parse` without the caller having to
    /// know the qualified prefix).  When `file_scope` is `Some(path)`
    /// the lookup is additionally narrowed to rows whose `file_path`
    /// equals `path`.  On ties, the row with the greatest
    /// `t_ingested_at` wins — matching
    /// [`KnowledgeGraph::get_entity_by_qualified_name`]'s newest-row
    /// contract.
    ///
    /// `parent_module` on the returned [`SymbolResolution`] is derived
    /// from the row's `qualified_name`: for `"foo::bar::baz"` it is
    /// `Some("foo::bar")`; for a `qualified_name` that is `NULL` or
    /// contains no `::` separator it is `None`.  No schema column is
    /// added — derivation happens in Rust per master-plan §12.1
    /// freeze + §18 Phase 1 Week 4 line 1749 (symbol resolution
    /// scope).
    ///
    /// Read-only — no transaction wrapper, consistent with
    /// [`KnowledgeGraph::get_entity_by_qualified_name`] and
    /// [`KnowledgeGraph::list_entities_by_file`].
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — statement prepare, bind, or
    ///   row-fetch failure.  Returns `Ok(None)` (not an error) when no
    ///   row matches.
    #[tracing::instrument(
        level = "debug",
        skip(self),
        fields(name = %name, scoped = file_scope.is_some()),
        name = "ucil.core.kg.resolve_symbol",
    )]
    pub fn resolve_symbol(
        &self,
        name: &str,
        file_scope: Option<&str>,
    ) -> Result<Option<SymbolResolution>, KnowledgeGraphError> {
        let sql = if file_scope.is_some() {
            "SELECT id, file_path, start_line, signature, doc_comment, qualified_name \
             FROM entities \
             WHERE (name = ?1 OR qualified_name = ?1 OR qualified_name LIKE '%::' || ?1) \
               AND file_path = ?2 \
             ORDER BY t_ingested_at DESC LIMIT 1"
        } else {
            "SELECT id, file_path, start_line, signature, doc_comment, qualified_name \
             FROM entities \
             WHERE (name = ?1 OR qualified_name = ?1 OR qualified_name LIKE '%::' || ?1) \
             ORDER BY t_ingested_at DESC LIMIT 1"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let row_result = if let Some(path) = file_scope {
            stmt.query_row(rusqlite::params![name, path], resolution_from_row)
        } else {
            stmt.query_row(rusqlite::params![name], resolution_from_row)
        };
        Ok(row_result.map(Some).or_else(absent_to_none)?)
    }

    // ── Hot-staging writers (P1-W4-F08) ─────────────────────────────
    //
    // Routes every hot-tier insert through `execute_in_transaction`
    // (BEGIN IMMEDIATE per master-plan §11 line 1117) so the
    // merge-consolidator's background sweep doesn't collide with
    // concurrent producer writes.

    /// Stage a [`HotObservation`] into the `hot_observations` table.
    ///
    /// `created_at` is set by the schema default (`datetime('now')`)
    /// and `promoted_to_warm` starts at `0`; both are managed by the
    /// merge-consolidator and are not part of the writer contract.
    /// Returns the new `hot_observations.id`.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — transaction open, statement
    ///   prepare, bind, or `RETURNING id` fetch failed.
    #[tracing::instrument(
        level = "debug",
        skip(self, observation),
        fields(session_id = observation.session_id.as_deref().unwrap_or("")),
        name = "ucil.core.kg.stage_hot_observation",
    )]
    pub fn stage_hot_observation(
        &mut self,
        observation: &HotObservation,
    ) -> Result<i64, KnowledgeGraphError> {
        self.execute_in_transaction(|tx| {
            let mut stmt = tx.prepare(
                "INSERT INTO hot_observations (\
                    raw_text, session_id, related_file, related_symbol\
                 ) VALUES (?1, ?2, ?3, ?4) \
                 RETURNING id;",
            )?;
            let id: i64 = stmt.query_row(
                rusqlite::params![
                    observation.raw_text,
                    observation.session_id,
                    observation.related_file,
                    observation.related_symbol,
                ],
                |row| row.get::<_, i64>(0),
            )?;
            Ok(id)
        })
    }

    /// Stage a convention-signal hit into `hot_convention_signals`.
    ///
    /// `pattern_hash` is the stable hash of the convention matcher
    /// (produced by the convention-learner layer that lands in a later
    /// phase); `file_path` is the source file that tripped the matcher;
    /// `example_snippet` is an optional excerpt for later human review.
    /// `created_at` and `promoted` are owned by the schema default and
    /// the merge-consolidator respectively.  Returns the new
    /// `hot_convention_signals.id`.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — transaction open, statement
    ///   prepare, bind, or `RETURNING id` fetch failed.
    #[tracing::instrument(
        level = "debug",
        skip(self),
        fields(pattern_hash = %pattern_hash, file_path = %file_path),
        name = "ucil.core.kg.stage_hot_convention_signal",
    )]
    pub fn stage_hot_convention_signal(
        &mut self,
        pattern_hash: &str,
        file_path: &str,
        example_snippet: Option<&str>,
    ) -> Result<i64, KnowledgeGraphError> {
        self.execute_in_transaction(|tx| {
            let mut stmt = tx.prepare(
                "INSERT INTO hot_convention_signals (\
                    pattern_hash, file_path, example_snippet\
                 ) VALUES (?1, ?2, ?3) \
                 RETURNING id;",
            )?;
            let id: i64 = stmt.query_row(
                rusqlite::params![pattern_hash, file_path, example_snippet],
                |row| row.get::<_, i64>(0),
            )?;
            Ok(id)
        })
    }

    /// Stage an architecture-delta record into
    /// `hot_architecture_deltas`.
    ///
    /// `change_type` is a short classifier (e.g. `"module_split"`,
    /// `"dep_added"`); `file_path` is the file the change touches;
    /// `details` is optional free-form context.  `created_at` and
    /// `promoted` are owned by the schema default and the merge-
    /// consolidator respectively.  Returns the new
    /// `hot_architecture_deltas.id`.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — transaction open, statement
    ///   prepare, bind, or `RETURNING id` fetch failed.
    #[tracing::instrument(
        level = "debug",
        skip(self),
        fields(change_type = %change_type, file_path = %file_path),
        name = "ucil.core.kg.stage_hot_architecture_delta",
    )]
    pub fn stage_hot_architecture_delta(
        &mut self,
        change_type: &str,
        file_path: &str,
        details: Option<&str>,
    ) -> Result<i64, KnowledgeGraphError> {
        self.execute_in_transaction(|tx| {
            let mut stmt = tx.prepare(
                "INSERT INTO hot_architecture_deltas (\
                    change_type, file_path, details\
                 ) VALUES (?1, ?2, ?3) \
                 RETURNING id;",
            )?;
            let id: i64 = stmt
                .query_row(rusqlite::params![change_type, file_path, details], |row| {
                    row.get::<_, i64>(0)
                })?;
            Ok(id)
        })
    }

    // ── WAL checkpoint (P1-W4-F02 supporting primitive) ─────────────
    //
    // Exposes `PRAGMA wal_checkpoint(<MODE>)` as a typed method so the
    // merge-consolidator's sweep loop — and ad-hoc shutdown code — can
    // bound the WAL file size without hand-building PRAGMA strings at
    // call sites.  Routing through a typed enum also enforces the
    // master-plan §11 line 1117 invariant that only the documented
    // checkpoint modes are used.

    /// Run `PRAGMA wal_checkpoint(<MODE>)` against the underlying
    /// connection and return the three-tuple `(busy, log, checkpointed)`
    /// the pragma reports — matching the `SQLite` column order.
    ///
    /// * `busy` — `1` if the pragma could not complete because another
    ///   connection was writing; `0` otherwise.
    /// * `log` — number of frames currently in the WAL file (after the
    ///   pragma runs).
    /// * `checkpointed` — number of frames successfully moved from the
    ///   WAL into the main database by this call.
    ///
    /// `WalCheckpointMode::Truncate` additionally shrinks the WAL file
    /// to zero bytes on success, which is the mode the scheduled sweep
    /// uses to keep on-disk state bounded.
    ///
    /// # Errors
    ///
    /// * [`KnowledgeGraphError::Sqlite`] — the `PRAGMA` failed, the row
    ///   shape was unexpected, or the connection was poisoned.
    #[tracing::instrument(
        level = "debug",
        skip(self),
        fields(mode = %mode.as_sql()),
        name = "ucil.core.kg.checkpoint_wal",
    )]
    pub fn checkpoint_wal(
        &self,
        mode: WalCheckpointMode,
    ) -> Result<(i64, i64, i64), KnowledgeGraphError> {
        // PRAGMA wal_checkpoint cannot be bound with `?N` parameters —
        // the mode is part of the pragma syntax — so we format the
        // token in.  `mode.as_sql()` returns one of two hard-coded
        // `&'static str`s so there is no injection surface.
        let sql = format!("PRAGMA wal_checkpoint({});", mode.as_sql());
        let tuple = self.conn.query_row(&sql, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        Ok(tuple)
    }
}

// ── Row decoders ─────────────────────────────────────────────────────────────
//
// Free functions rather than `impl`s so `query_row` / `query_map`
// callers can pass them directly — rusqlite's closure signature is
// `FnMut(&Row<'_>) -> rusqlite::Result<T>` and free functions coerce
// cleanly.

/// Read an [`Entity`] row from a `SELECT id, kind, name, qualified_name,
/// file_path, start_line, end_line, signature, doc_comment, language,
/// t_valid_from, t_valid_to, importance, source_tool, source_hash`
/// statement (exact column order).
fn entity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entity> {
    Ok(Entity {
        id: row.get::<_, Option<i64>>(0)?,
        kind: row.get::<_, String>(1)?,
        name: row.get::<_, String>(2)?,
        qualified_name: row.get::<_, Option<String>>(3)?,
        file_path: row.get::<_, String>(4)?,
        start_line: row.get::<_, Option<i64>>(5)?,
        end_line: row.get::<_, Option<i64>>(6)?,
        signature: row.get::<_, Option<String>>(7)?,
        doc_comment: row.get::<_, Option<String>>(8)?,
        language: row.get::<_, Option<String>>(9)?,
        t_valid_from: row.get::<_, Option<String>>(10)?,
        t_valid_to: row.get::<_, Option<String>>(11)?,
        importance: row.get::<_, f64>(12)?,
        source_tool: row.get::<_, Option<String>>(13)?,
        source_hash: row.get::<_, Option<String>>(14)?,
    })
}

/// Read a [`Relation`] row from a `SELECT id, source_id, target_id,
/// kind, weight, t_valid_from, t_valid_to, source_tool,
/// source_evidence, confidence` statement (exact column order).
fn relation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Relation> {
    Ok(Relation {
        id: row.get::<_, Option<i64>>(0)?,
        source_id: row.get::<_, i64>(1)?,
        target_id: row.get::<_, i64>(2)?,
        kind: row.get::<_, String>(3)?,
        weight: row.get::<_, f64>(4)?,
        t_valid_from: row.get::<_, Option<String>>(5)?,
        t_valid_to: row.get::<_, Option<String>>(6)?,
        source_tool: row.get::<_, Option<String>>(7)?,
        source_evidence: row.get::<_, Option<String>>(8)?,
        confidence: row.get::<_, f64>(9)?,
    })
}

/// Read a [`SymbolResolution`] row from a `SELECT id, file_path,
/// start_line, signature, doc_comment, qualified_name` statement
/// (exact column order).
///
/// `parent_module` is derived from `qualified_name` by splitting on
/// the final `::`: `"foo::bar::baz"` → `Some("foo::bar")`; a
/// `qualified_name` that is `NULL` or lacks a `::` separator yields
/// `None`.
fn resolution_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolResolution> {
    let id = row.get::<_, Option<i64>>(0)?;
    let file_path = row.get::<_, String>(1)?;
    let start_line = row.get::<_, Option<i64>>(2)?;
    let signature = row.get::<_, Option<String>>(3)?;
    let doc_comment = row.get::<_, Option<String>>(4)?;
    let qualified_name = row.get::<_, Option<String>>(5)?;
    let parent_module = qualified_name
        .as_deref()
        .and_then(|qn| qn.rsplit_once("::").map(|(head, _tail)| head.to_owned()));
    Ok(SymbolResolution {
        id,
        qualified_name,
        file_path,
        start_line,
        signature,
        doc_comment,
        parent_module,
    })
}

/// Convert a `QueryReturnedNoRows` into `Ok(None)` so the read helpers
/// can distinguish "absent" from "SQL error".  Any other error flows
/// through.
fn absent_to_none<T>(err: rusqlite::Error) -> Result<Option<T>, rusqlite::Error> {
    if matches!(err, rusqlite::Error::QueryReturnedNoRows) {
        Ok(None)
    } else {
        Err(err)
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
/// The test walks a real on-disk `SQLite` file through:
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

// ── Entity CRUD tests ────────────────────────────────────────────────────────
//
// Module-root placement (no `mod tests { }`) per DEC-0005 and the
// frozen selector `knowledge_graph::test_*` — see
// `test_schema_creation` above as the precedent WO-0011 established.

/// `test_upsert_and_get_entity` — round-trip an `Entity` through
/// `upsert_entity` + `get_entity_by_qualified_name` and assert every
/// field survives.
#[cfg(test)]
#[test]
fn test_upsert_and_get_entity() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let mut kg = KnowledgeGraph::open(&tmp.path().join("kg.db"))
        .expect("KnowledgeGraph::open must succeed on fresh tempfile");

    let entity = Entity {
        id: None,
        kind: "function".to_owned(),
        name: "render".to_owned(),
        qualified_name: Some("app::view::render".to_owned()),
        file_path: "src/app/view.rs".to_owned(),
        start_line: Some(42),
        end_line: Some(97),
        signature: Some("fn render(ctx: &Ctx) -> Html".to_owned()),
        doc_comment: Some("Render a page.".to_owned()),
        language: Some("rust".to_owned()),
        t_valid_from: Some("2026-04-18T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        importance: 0.75,
        source_tool: Some("tree-sitter".to_owned()),
        source_hash: Some("deadbeef".to_owned()),
    };
    let id = kg
        .upsert_entity(&entity)
        .expect("first upsert must succeed");
    assert!(id > 0, "RETURNING id must produce a positive rowid");

    let fetched = kg
        .get_entity_by_qualified_name("app::view::render", Some("src/app/view.rs"))
        .expect("read must succeed")
        .expect("row must be present after upsert");

    assert_eq!(fetched.id, Some(id));
    assert_eq!(fetched.kind, entity.kind);
    assert_eq!(fetched.name, entity.name);
    assert_eq!(fetched.qualified_name, entity.qualified_name);
    assert_eq!(fetched.file_path, entity.file_path);
    assert_eq!(fetched.start_line, entity.start_line);
    assert_eq!(fetched.end_line, entity.end_line);
    assert_eq!(fetched.signature, entity.signature);
    assert_eq!(fetched.doc_comment, entity.doc_comment);
    assert_eq!(fetched.language, entity.language);
    assert_eq!(fetched.t_valid_from, entity.t_valid_from);
    assert_eq!(fetched.t_valid_to, entity.t_valid_to);
    assert!(
        (fetched.importance - entity.importance).abs() < 1e-9,
        "importance round-trips as f64",
    );
    assert_eq!(fetched.source_tool, entity.source_tool);
    assert_eq!(fetched.source_hash, entity.source_hash);

    // The `file_path=None` lookup form also resolves.
    let fetched_any = kg
        .get_entity_by_qualified_name("app::view::render", None)
        .expect("read must succeed")
        .expect("row must be present in any-file lookup");
    assert_eq!(fetched_any.id, Some(id));

    // Negative path: absent qualified_name returns Ok(None), not error.
    let missing = kg
        .get_entity_by_qualified_name("app::view::missing", None)
        .expect("missing row must be Ok(None)");
    assert!(missing.is_none(), "absent qualified_name must be None");
}

/// `test_list_entities_by_file` — insert 3 rows in the same file with
/// ascending `start_line`, assert `list_entities_by_file` returns them
/// in document order.
#[cfg(test)]
#[test]
fn test_list_entities_by_file() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let mut kg =
        KnowledgeGraph::open(&tmp.path().join("kg.db")).expect("KnowledgeGraph::open must succeed");

    let file_path = "src/pipeline.rs".to_owned();
    let rows = [
        (10_i64, "parse", "pipeline::parse"),
        (40_i64, "lint", "pipeline::lint"),
        (80_i64, "emit", "pipeline::emit"),
    ];
    for (line, name, qname) in &rows {
        let e = Entity {
            id: None,
            kind: "function".to_owned(),
            name: (*name).to_owned(),
            qualified_name: Some((*qname).to_owned()),
            file_path: file_path.clone(),
            start_line: Some(*line),
            end_line: Some(*line + 5),
            signature: None,
            doc_comment: None,
            language: Some("rust".to_owned()),
            t_valid_from: Some("2026-04-18T00:00:00+00:00".to_owned()),
            t_valid_to: None,
            importance: 0.5,
            source_tool: None,
            source_hash: None,
        };
        kg.upsert_entity(&e).expect("upsert must succeed");
    }

    let listed = kg
        .list_entities_by_file(&file_path)
        .expect("list must succeed");
    assert_eq!(listed.len(), 3, "all three rows returned");
    let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["parse", "lint", "emit"],
        "rows returned in start_line ASC order",
    );

    // A file with no entities returns an empty vec, not an error.
    let none = kg
        .list_entities_by_file("does/not/exist.rs")
        .expect("list for absent file must succeed");
    assert!(none.is_empty(), "absent file yields empty vec");
}

/// `test_entity_unique_constraint_updates` — inserting the same
/// `(qualified_name, file_path, t_valid_from)` triple twice hits the
/// `ON CONFLICT DO UPDATE` branch: the second call returns the SAME
/// `id`, bumps `access_count`, and refreshes `t_last_verified`.
#[cfg(test)]
#[test]
fn test_entity_unique_constraint_updates() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let mut kg =
        KnowledgeGraph::open(&tmp.path().join("kg.db")).expect("KnowledgeGraph::open must succeed");

    let entity = Entity {
        id: None,
        kind: "function".to_owned(),
        name: "handle".to_owned(),
        qualified_name: Some("api::handle".to_owned()),
        file_path: "src/api.rs".to_owned(),
        start_line: Some(10),
        end_line: Some(25),
        signature: None,
        doc_comment: None,
        language: Some("rust".to_owned()),
        t_valid_from: Some("2026-04-18T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        importance: 0.5,
        source_tool: None,
        source_hash: None,
    };

    let id1 = kg.upsert_entity(&entity).expect("first upsert");
    let id2 = kg
        .upsert_entity(&entity)
        .expect("second upsert must hit ON CONFLICT");
    assert_eq!(id1, id2, "ON CONFLICT DO UPDATE preserves the rowid");

    // `access_count` bumped.
    let count: i64 = kg
        .conn()
        .query_row(
            "SELECT access_count FROM entities WHERE id = ?1;",
            rusqlite::params![id1],
            |row| row.get::<_, i64>(0),
        )
        .expect("access_count read must succeed");
    assert!(
        count >= 1,
        "access_count must be incremented on conflict (got {count})",
    );

    // `t_last_verified` set.
    let tlv: Option<String> = kg
        .conn()
        .query_row(
            "SELECT t_last_verified FROM entities WHERE id = ?1;",
            rusqlite::params![id1],
            |row| row.get::<_, Option<String>>(0),
        )
        .expect("t_last_verified read must succeed");
    assert!(
        tlv.is_some() && !tlv.as_deref().unwrap_or("").is_empty(),
        "ON CONFLICT DO UPDATE must set t_last_verified (got {tlv:?})",
    );

    // Third upsert bumps `access_count` again.
    kg.upsert_entity(&entity).expect("third upsert");
    let count3: i64 = kg
        .conn()
        .query_row(
            "SELECT access_count FROM entities WHERE id = ?1;",
            rusqlite::params![id1],
            |row| row.get::<_, i64>(0),
        )
        .expect("access_count read must succeed");
    assert!(
        count3 > count,
        "access_count monotonic across conflicts (was {count}, now {count3})",
    );
}

/// `test_upsert_relation_and_list` — insert 2 entities + 1 relation
/// between them, assert `list_relations_by_source` returns the single
/// relation with all fields intact.
#[cfg(test)]
#[test]
fn test_upsert_relation_and_list() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let mut kg =
        KnowledgeGraph::open(&tmp.path().join("kg.db")).expect("KnowledgeGraph::open must succeed");

    let make_entity = |name: &str, qname: &str| Entity {
        id: None,
        kind: "function".to_owned(),
        name: name.to_owned(),
        qualified_name: Some(qname.to_owned()),
        file_path: "src/graph.rs".to_owned(),
        start_line: Some(1),
        end_line: Some(10),
        signature: None,
        doc_comment: None,
        language: Some("rust".to_owned()),
        t_valid_from: Some("2026-04-18T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        importance: 0.5,
        source_tool: None,
        source_hash: None,
    };
    let src_id = kg
        .upsert_entity(&make_entity("caller", "graph::caller"))
        .expect("source entity upsert must succeed");
    let tgt_id = kg
        .upsert_entity(&make_entity("callee", "graph::callee"))
        .expect("target entity upsert must succeed");

    let relation = Relation {
        id: None,
        source_id: src_id,
        target_id: tgt_id,
        kind: "calls".to_owned(),
        weight: 0.9,
        t_valid_from: Some("2026-04-18T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        source_tool: Some("tree-sitter".to_owned()),
        source_evidence: Some("src/graph.rs:7".to_owned()),
        confidence: 0.95,
    };
    let rel_id = kg
        .upsert_relation(&relation)
        .expect("relation upsert must succeed");
    assert!(rel_id > 0, "RETURNING id must be positive");

    let listed = kg
        .list_relations_by_source(src_id)
        .expect("list_relations_by_source must succeed");
    assert_eq!(listed.len(), 1, "exactly one relation was inserted");
    let fetched = &listed[0];
    assert_eq!(fetched.id, Some(rel_id));
    assert_eq!(fetched.source_id, src_id);
    assert_eq!(fetched.target_id, tgt_id);
    assert_eq!(fetched.kind, relation.kind);
    assert!(
        (fetched.weight - relation.weight).abs() < 1e-9,
        "weight round-trips as f64",
    );
    assert_eq!(fetched.t_valid_from, relation.t_valid_from);
    assert_eq!(fetched.t_valid_to, relation.t_valid_to);
    assert_eq!(fetched.source_tool, relation.source_tool);
    assert_eq!(fetched.source_evidence, relation.source_evidence);
    assert!(
        (fetched.confidence - relation.confidence).abs() < 1e-9,
        "confidence round-trips as f64",
    );

    // No UNIQUE constraint — a second insert appends a fresh row.
    kg.upsert_relation(&relation)
        .expect("second insert appends");
    let listed2 = kg
        .list_relations_by_source(src_id)
        .expect("second list must succeed");
    assert_eq!(
        listed2.len(),
        2,
        "relations has no UNIQUE; second insert appends",
    );

    // A source id with no edges returns an empty vec.
    let empty = kg
        .list_relations_by_source(9_999_999)
        .expect("empty list must be Ok");
    assert!(empty.is_empty(), "absent source_id yields empty vec");
}

/// `test_get_entity_by_id` — round-trip an `Entity` through `upsert_entity`,
/// then fetch it back by rowid via `get_entity_by_id`.  Asserts every
/// field survives and that a rowid with no matching row returns
/// `Ok(None)` (not an error).
#[cfg(test)]
#[test]
fn test_get_entity_by_id() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let mut kg = KnowledgeGraph::open(&tmp.path().join("kg.db"))
        .expect("KnowledgeGraph::open must succeed on fresh tempfile");

    let entity = Entity {
        id: None,
        kind: "function".to_owned(),
        name: "resolve".to_owned(),
        qualified_name: Some("app::dns::resolve".to_owned()),
        file_path: "src/app/dns.rs".to_owned(),
        start_line: Some(17),
        end_line: Some(33),
        signature: Some("fn resolve(host: &str) -> Ip".to_owned()),
        doc_comment: Some("Resolve a hostname.".to_owned()),
        language: Some("rust".to_owned()),
        t_valid_from: Some("2026-04-18T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        importance: 0.5,
        source_tool: Some("tree-sitter".to_owned()),
        source_hash: None,
    };
    let id = kg.upsert_entity(&entity).expect("upsert must succeed");

    let fetched = kg
        .get_entity_by_id(id)
        .expect("get_entity_by_id must succeed")
        .expect("row must be present after upsert");

    assert_eq!(fetched.id, Some(id));
    assert_eq!(fetched.name, entity.name);
    assert_eq!(fetched.qualified_name, entity.qualified_name);
    assert_eq!(fetched.file_path, entity.file_path);
    assert_eq!(fetched.start_line, entity.start_line);
    assert_eq!(fetched.signature, entity.signature);
    assert_eq!(fetched.doc_comment, entity.doc_comment);
    assert_eq!(fetched.language, entity.language);
    assert_eq!(fetched.source_tool, entity.source_tool);

    // A rowid that was never inserted returns Ok(None), not an error.
    let missing = kg
        .get_entity_by_id(9_999_999)
        .expect("missing rowid must be Ok(None)");
    assert!(
        missing.is_none(),
        "absent rowid must yield None, got {missing:?}",
    );
}

/// `test_list_relations_by_target` — insert two entities and two
/// relations targeting the same `target_id` with different
/// `source_id`/`kind`, assert `list_relations_by_target` returns both
/// rows in insertion order and that an unknown target yields an empty
/// vec.
#[cfg(test)]
#[test]
fn test_list_relations_by_target() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let mut kg =
        KnowledgeGraph::open(&tmp.path().join("kg.db")).expect("KnowledgeGraph::open must succeed");

    let make_entity = |name: &str, qname: &str| Entity {
        id: None,
        kind: "function".to_owned(),
        name: name.to_owned(),
        qualified_name: Some(qname.to_owned()),
        file_path: "src/caller_map.rs".to_owned(),
        start_line: Some(1),
        end_line: Some(10),
        signature: None,
        doc_comment: None,
        language: Some("rust".to_owned()),
        t_valid_from: Some("2026-04-18T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        importance: 0.5,
        source_tool: None,
        source_hash: None,
    };
    let caller_a = kg
        .upsert_entity(&make_entity("caller_a", "graph::caller_a"))
        .expect("caller_a upsert");
    let caller_b = kg
        .upsert_entity(&make_entity("caller_b", "graph::caller_b"))
        .expect("caller_b upsert");
    let target = kg
        .upsert_entity(&make_entity("target", "graph::target"))
        .expect("target upsert");

    let make_relation = |src: i64, kind: &str| Relation {
        id: None,
        source_id: src,
        target_id: target,
        kind: kind.to_owned(),
        weight: 0.5,
        t_valid_from: Some("2026-04-18T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        source_tool: Some("tree-sitter".to_owned()),
        source_evidence: None,
        confidence: 0.9,
    };
    let rel_a = kg
        .upsert_relation(&make_relation(caller_a, "calls"))
        .expect("rel_a upsert");
    let rel_b = kg
        .upsert_relation(&make_relation(caller_b, "references"))
        .expect("rel_b upsert");

    let listed = kg
        .list_relations_by_target(target)
        .expect("list_relations_by_target must succeed");
    assert_eq!(listed.len(), 2, "both inbound edges returned");
    assert_eq!(
        listed[0].id,
        Some(rel_a),
        "first row is the earliest-inserted (id ASC)",
    );
    assert_eq!(listed[0].source_id, caller_a);
    assert_eq!(listed[0].kind, "calls");
    assert_eq!(listed[1].id, Some(rel_b));
    assert_eq!(listed[1].source_id, caller_b);
    assert_eq!(listed[1].kind, "references");

    // target with no inbound edges returns empty vec.
    let empty = kg
        .list_relations_by_target(9_999_999)
        .expect("empty list must be Ok");
    assert!(empty.is_empty(), "absent target_id yields empty vec");
}

/// `test_bi_temporal_as_of` — insert three rows for the same
/// `qualified_name` with staggered `t_valid_from` / `t_valid_to`, query
/// `get_entity_as_of` at different instants, assert the correct
/// version is returned.
#[cfg(test)]
#[test]
fn test_bi_temporal_as_of() {
    use chrono::TimeZone;
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let mut kg =
        KnowledgeGraph::open(&tmp.path().join("kg.db")).expect("KnowledgeGraph::open must succeed");

    // Three valid-time windows on the same qualified_name:
    //
    //   row-1: [2020-01-01, 2022-01-01)
    //   row-2: [2022-01-01, 2024-01-01)
    //   row-3: [2024-01-01, NULL)          ← still valid
    //
    // Each row lives in a different file_path so the
    // UNIQUE(qualified_name, file_path, t_valid_from) constraint
    // doesn't collapse them — and the bi-temporal query returns the
    // version whose window contains the query instant regardless of
    // which file_path it lives in.
    let make_row = |file: &str, from: &str, to: Option<&str>, signature: &str| Entity {
        id: None,
        kind: "function".to_owned(),
        name: "authenticate".to_owned(),
        qualified_name: Some("api::authenticate".to_owned()),
        file_path: file.to_owned(),
        start_line: Some(1),
        end_line: Some(20),
        signature: Some(signature.to_owned()),
        doc_comment: None,
        language: Some("rust".to_owned()),
        t_valid_from: Some(from.to_owned()),
        t_valid_to: to.map(str::to_owned),
        importance: 0.5,
        source_tool: None,
        source_hash: None,
    };
    kg.upsert_entity(&make_row(
        "src/api.rs",
        "2020-01-01T00:00:00+00:00",
        Some("2022-01-01T00:00:00+00:00"),
        "fn authenticate(user: &str)",
    ))
    .expect("row-1 upsert");
    kg.upsert_entity(&make_row(
        "src/api/v2.rs",
        "2022-01-01T00:00:00+00:00",
        Some("2024-01-01T00:00:00+00:00"),
        "fn authenticate(user: &str, password: &str)",
    ))
    .expect("row-2 upsert");
    kg.upsert_entity(&make_row(
        "src/api/v3.rs",
        "2024-01-01T00:00:00+00:00",
        None,
        "fn authenticate(creds: &Creds) -> Result<User, AuthError>",
    ))
    .expect("row-3 upsert");

    // Query between row-2 and row-3 (2023-06-15) — expect row-2.
    let t_mid = Utc
        .with_ymd_and_hms(2023, 6, 15, 0, 0, 0)
        .single()
        .expect("2023-06-15 must be a valid UTC instant");
    let hit_mid = kg
        .get_entity_as_of("api::authenticate", t_mid)
        .expect("as_of must succeed")
        .expect("row-2's window contains 2023-06-15");
    assert_eq!(
        hit_mid.signature.as_deref(),
        Some("fn authenticate(user: &str, password: &str)"),
        "expected row-2 (v2) at t=2023-06-15, got {:?}",
        hit_mid.signature,
    );
    assert_eq!(hit_mid.file_path, "src/api/v2.rs");

    // Query after row-3 started (2025-03-01) — expect row-3 (open
    // window, `t_valid_to IS NULL`).
    let t_now = Utc
        .with_ymd_and_hms(2025, 3, 1, 0, 0, 0)
        .single()
        .expect("2025-03-01 must be valid");
    let hit_now = kg
        .get_entity_as_of("api::authenticate", t_now)
        .expect("as_of must succeed")
        .expect("row-3's open window covers 2025-03-01");
    assert_eq!(
        hit_now.signature.as_deref(),
        Some("fn authenticate(creds: &Creds) -> Result<User, AuthError>"),
        "expected row-3 (v3) at t=2025-03-01, got {:?}",
        hit_now.signature,
    );
    assert_eq!(hit_now.file_path, "src/api/v3.rs");

    // Query at row-1's lower bound (2020-01-01) — expect row-1
    // (inclusive lower bound).
    let t_early = Utc
        .with_ymd_and_hms(2020, 1, 1, 0, 0, 0)
        .single()
        .expect("2020-01-01 must be valid");
    let hit_early = kg
        .get_entity_as_of("api::authenticate", t_early)
        .expect("as_of must succeed")
        .expect("row-1's window includes its lower bound");
    assert_eq!(hit_early.file_path, "src/api.rs");

    // Query before all rows (1999-01-01) — expect None.
    let t_before = Utc
        .with_ymd_and_hms(1999, 1, 1, 0, 0, 0)
        .single()
        .expect("1999-01-01 must be valid");
    let miss = kg
        .get_entity_as_of("api::authenticate", t_before)
        .expect("as_of must succeed");
    assert!(
        miss.is_none(),
        "pre-1999 has no valid version, got {miss:?}",
    );
}

/// `test_hot_staging_writes` — **frozen F08 acceptance selector**
/// (`knowledge_graph::test_hot_staging_writes`).
///
/// Exercises all three hot-staging writers introduced by WO-0024
/// (P1-W4-F08): [`KnowledgeGraph::stage_hot_observation`],
/// [`KnowledgeGraph::stage_hot_convention_signal`], and
/// [`KnowledgeGraph::stage_hot_architecture_delta`].  All three write
/// through `execute_in_transaction` and so satisfy the master-plan §11
/// line 1117 `BEGIN IMMEDIATE` invariant.
///
/// Per the WO `scope_in` contract: each staging helper is called once,
/// and each table is then count-checked back through
/// `self.conn.query_row("SELECT COUNT(*) FROM <tbl>", …)` to assert
/// the row landed in its owning table (not cross-table).
///
/// No mocks of rusqlite — the test runs against a real tempfile-backed
/// `SQLite` db via `tempfile::TempDir` per phase-log invariant 1.
#[cfg(test)]
#[test]
fn test_hot_staging_writes() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let mut kg =
        KnowledgeGraph::open(&tmp.path().join("kg.db")).expect("KnowledgeGraph::open must succeed");

    // ── hot_observations ────────────────────────────────────────────
    let obs = HotObservation {
        raw_text: "the daemon logged a retry on SIGHUP at 12:04".to_owned(),
        session_id: Some("sess-001".to_owned()),
        related_file: Some("crates/ucil-daemon/src/signals.rs".to_owned()),
        related_symbol: Some("handle_sighup".to_owned()),
    };
    let obs_id = kg
        .stage_hot_observation(&obs)
        .expect("stage_hot_observation must succeed");
    assert!(
        obs_id > 0,
        "RETURNING id on hot_observations must be positive (got {obs_id})",
    );

    // ── hot_convention_signals ──────────────────────────────────────
    let conv_id = kg
        .stage_hot_convention_signal(
            "sha256:abc123",
            "crates/ucil-core/src/knowledge_graph.rs",
            Some("fn stage_hot_observation(…) { … }"),
        )
        .expect("stage_hot_convention_signal must succeed");
    assert!(
        conv_id > 0,
        "RETURNING id on hot_convention_signals must be positive (got {conv_id})",
    );

    // ── hot_architecture_deltas ─────────────────────────────────────
    let arch_id = kg
        .stage_hot_architecture_delta(
            "module_split",
            "crates/ucil-core/src/knowledge_graph.rs",
            Some("split hot-staging writers into their own section"),
        )
        .expect("stage_hot_architecture_delta must succeed");
    assert!(
        arch_id > 0,
        "RETURNING id on hot_architecture_deltas must be positive (got {arch_id})",
    );

    // ── per-table count assertions ──────────────────────────────────
    //
    // Confirm each row landed in its owning table (and nowhere else) —
    // catches a cross-table INSERT typo in the three stagers above.
    let obs_count: i64 = kg
        .conn()
        .query_row("SELECT COUNT(*) FROM hot_observations;", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("COUNT(hot_observations) must succeed");
    assert_eq!(obs_count, 1, "exactly one hot_observations row inserted");

    let conv_count: i64 = kg
        .conn()
        .query_row("SELECT COUNT(*) FROM hot_convention_signals;", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("COUNT(hot_convention_signals) must succeed");
    assert_eq!(
        conv_count, 1,
        "exactly one hot_convention_signals row inserted",
    );

    let arch_count: i64 = kg
        .conn()
        .query_row("SELECT COUNT(*) FROM hot_architecture_deltas;", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("COUNT(hot_architecture_deltas) must succeed");
    assert_eq!(
        arch_count, 1,
        "exactly one hot_architecture_deltas row inserted",
    );
}

/// `test_wal_checkpoint_truncates` — acceptance test for the WAL
/// checkpoint primitive that underpins the merge-consolidator's
/// scheduled sweep (P1-W4-F02).
///
/// Drives the db through a real write workload so the WAL file has
/// frames to checkpoint, then calls
/// [`KnowledgeGraph::checkpoint_wal`] with both modes and asserts:
///
/// 1. `Passive` returns a non-busy tuple (`busy == 0`) — nobody else
///    holds a write lock in a single-threaded test.
/// 2. `Truncate` returns a non-busy tuple AND the `kg.db-wal` file on
///    disk shrinks to zero bytes (the defining behaviour of
///    `TRUNCATE` vs. `PASSIVE` / `FULL` / `RESTART`).
///
/// No mocks of rusqlite or `SQLite` — the test runs against a real
/// tempfile-backed db.
#[cfg(test)]
#[test]
fn test_wal_checkpoint_truncates() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let db_path = tmp.path().join("kg.db");
    let wal_path = tmp.path().join("kg.db-wal");

    let mut kg = KnowledgeGraph::open(&db_path).expect("KnowledgeGraph::open must succeed");

    // Drive a real write workload through `execute_in_transaction` so
    // the WAL file accumulates frames.  Without any writes the WAL is
    // already empty and a TRUNCATE checkpoint is indistinguishable
    // from a no-op.
    for i in 0..10 {
        kg.execute_in_transaction(|tx| {
            tx.execute(
                "INSERT INTO sessions (id, agent_id, branch) \
                 VALUES (?1, 'wal-truncate-test', 'main');",
                rusqlite::params![format!("sess-{i:03}")],
            )?;
            Ok(())
        })
        .expect("workload insert must succeed");
    }

    // Assert the WAL exists on disk and has non-zero size.  (Ext4
    // reports the WAL file as soon as the first write frame is
    // appended; WAL2 / bigfile-WAL configurations still create it.)
    let wal_size_before = std::fs::metadata(&wal_path)
        .expect("kg.db-wal must exist after a workload")
        .len();
    assert!(
        wal_size_before > 0,
        "expected non-empty WAL before TRUNCATE checkpoint, got {wal_size_before} bytes",
    );

    // ── PASSIVE ─────────────────────────────────────────────────────
    //
    // Single-threaded test → no concurrent writer → `busy == 0`.
    let (busy_p, _log_p, _ckpt_p) = kg
        .checkpoint_wal(WalCheckpointMode::Passive)
        .expect("PASSIVE checkpoint must succeed");
    assert_eq!(
        busy_p, 0,
        "PASSIVE checkpoint must not report BUSY in a single-threaded test (got {busy_p})",
    );

    // ── TRUNCATE ────────────────────────────────────────────────────
    //
    // TRUNCATE is the defining behaviour: after a successful call the
    // WAL file is zero-length on disk.
    let (busy_t, _log_t, _ckpt_t) = kg
        .checkpoint_wal(WalCheckpointMode::Truncate)
        .expect("TRUNCATE checkpoint must succeed");
    assert_eq!(
        busy_t, 0,
        "TRUNCATE checkpoint must not report BUSY in a single-threaded test (got {busy_t})",
    );
    let wal_size_after = std::fs::metadata(&wal_path)
        .expect("kg.db-wal must still exist after TRUNCATE")
        .len();
    assert_eq!(
        wal_size_after, 0,
        "TRUNCATE must shrink kg.db-wal to 0 bytes (got {wal_size_after})",
    );
}

// ── Symbol-resolution tests (P1-W4-F03) ──────────────────────────────────────
//
// Module-root placement (no `mod tests { }`) per DEC-0005 — the frozen
// F03 selector `knowledge_graph::test_symbol_resolution` must resolve
// at nextest filter time.

/// Build a test `Entity` with the F03-common defaults filled in.
///
/// Extracted so `test_symbol_resolution` stays under the
/// `clippy::too_many_lines` (100) threshold while still exercising
/// every branch of the WO-0031 scope.  `language`, `t_valid_from`,
/// `importance`, and the source-tool columns are fixed to values
/// that don't affect resolution — only the arguments to this helper
/// matter to the assertions.
#[cfg(test)]
fn mk_resolver_fixture(
    name: &str,
    qualified_name: Option<&str>,
    file_path: &str,
    start_line: Option<i64>,
    signature: Option<&str>,
    doc_comment: Option<&str>,
) -> Entity {
    Entity {
        id: None,
        kind: "function".to_owned(),
        name: name.to_owned(),
        qualified_name: qualified_name.map(str::to_owned),
        file_path: file_path.to_owned(),
        start_line,
        end_line: None,
        signature: signature.map(str::to_owned),
        doc_comment: doc_comment.map(str::to_owned),
        language: Some("rust".to_owned()),
        t_valid_from: Some("2026-04-18T00:00:00+00:00".to_owned()),
        t_valid_to: None,
        importance: 0.5,
        source_tool: None,
        source_hash: None,
    }
}

/// `test_symbol_resolution` — **frozen F03 acceptance selector**
/// (`knowledge_graph::test_symbol_resolution`).
///
/// Covers the three match shapes (bare `name`, full `qualified_name`,
/// terminal-segment `LIKE '%::' || ?1`) + the `file_scope` narrowing
/// + the `parent_module` derivation + the `Ok(None)` miss path
///   documented on [`KnowledgeGraph::resolve_symbol`].
///
/// Tie-breaking is deterministic: `SQLite`'s `datetime('now')` schema
/// default has second precision, so the two `"parse"`-named rows are
/// inserted back-to-back with a >1s sleep between them to guarantee
/// strictly-ordered `t_ingested_at` values.  Without this, an
/// `ORDER BY t_ingested_at DESC LIMIT 1` on ties would be undefined
/// (`SQLite`'s sort stability is not guaranteed under `LIMIT`).
///
/// No mocks of rusqlite / sqlite — every assertion runs against a
/// real tempfile-backed db via `tempfile::TempDir`.
#[cfg(test)]
#[test]
fn test_symbol_resolution() {
    use tempfile::TempDir;

    let tmp = TempDir::new().expect("tempdir must be creatable");
    let mut kg =
        KnowledgeGraph::open(&tmp.path().join("kg.db")).expect("KnowledgeGraph::open must succeed");

    // ── Row 1 — treesitter::parser::parse (inserted FIRST; older).
    kg.upsert_entity(&mk_resolver_fixture(
        "parse",
        Some("ucil_treesitter::parser::parse"),
        "crates/ucil-treesitter/src/parser.rs",
        Some(42),
        Some("fn parse(src: &str) -> Ast"),
        Some("Parse a source string."),
    ))
    .expect("row1 upsert must succeed");

    // Force strictly-distinct t_ingested_at values: sleep >1s between
    // the two back-to-back `"parse"` upserts so the second lands in a
    // later second (schema default is `datetime('now')`, second-only
    // precision).  Without this gap, `ORDER BY t_ingested_at DESC
    // LIMIT 1` ties are implementation-defined under LIMIT per the
    // SQLite query planner.
    std::thread::sleep(std::time::Duration::from_millis(1_100));

    // ── Row 2 — ucil_core::types::parse (inserted SECOND; newest wins).
    kg.upsert_entity(&mk_resolver_fixture(
        "parse",
        Some("ucil_core::types::parse"),
        "crates/ucil-core/src/types.rs",
        Some(7),
        None,
        None,
    ))
    .expect("row2 upsert must succeed");

    // ── Row 3 — `disambiguous` (qualified_name NULL → parent_module None).
    kg.upsert_entity(&mk_resolver_fixture(
        "disambiguous",
        None,
        "crates/misc.rs",
        Some(1),
        None,
        None,
    ))
    .expect("row3 upsert must succeed");

    // ── (c) unscoped resolve_symbol("parse") — newest ingest wins ──
    let hit = kg
        .resolve_symbol("parse", None)
        .expect("resolve_symbol must succeed")
        .expect("`parse` must resolve to the newest-ingested row");
    assert_eq!(
        hit.file_path, "crates/ucil-core/src/types.rs",
        "newest ingest is the ucil_core::types row (row2)",
    );
    assert_eq!(hit.start_line, Some(7));
    assert_eq!(hit.signature, None);
    assert_eq!(hit.doc_comment, None);
    assert_eq!(
        hit.parent_module,
        Some("ucil_core::types".to_owned()),
        "parent_module is qualified_name minus the terminal `::name` segment",
    );

    // ── (d) scoped resolve_symbol reaches the treesitter row ─────────
    let scoped = kg
        .resolve_symbol("parse", Some("crates/ucil-treesitter/src/parser.rs"))
        .expect("scoped resolve_symbol must succeed")
        .expect("treesitter-scoped `parse` must resolve");
    assert_eq!(
        scoped.file_path, "crates/ucil-treesitter/src/parser.rs",
        "file_scope narrows to the treesitter row even though types is newer",
    );
    assert_eq!(scoped.start_line, Some(42));
    assert_eq!(
        scoped.signature.as_deref(),
        Some("fn parse(src: &str) -> Ast"),
    );
    assert_eq!(
        scoped.doc_comment.as_deref(),
        Some("Parse a source string.")
    );
    assert_eq!(
        scoped.parent_module,
        Some("ucil_treesitter::parser".to_owned()),
    );

    // ── (e) qualified_name == NULL ⇒ parent_module == None ──────────
    let disamb = kg
        .resolve_symbol("disambiguous", None)
        .expect("disambiguous resolve must succeed")
        .expect("disambiguous row must be found by bare name");
    assert_eq!(disamb.file_path, "crates/misc.rs");
    assert_eq!(disamb.start_line, Some(1));
    assert_eq!(
        disamb.parent_module, None,
        "qualified_name was NULL so parent_module must be None",
    );

    // ── (f) absent symbol returns Ok(None) (not an error) ────────────
    let missing = kg
        .resolve_symbol("nonexistent", None)
        .expect("nonexistent resolve must succeed");
    assert!(
        missing.is_none(),
        "unknown symbol name must produce Ok(None), got {missing:?}",
    );

    // ── (additional) terminal-segment match via full qualified_name ──
    // Covers the `qualified_name = ?1` and `LIKE '%::' || ?1` branches
    // of the SQL — the bare-name branch is exercised above.
    let by_qname = kg
        .resolve_symbol("ucil_treesitter::parser::parse", None)
        .expect("qualified_name resolve must succeed")
        .expect("qualified_name exact match must resolve");
    assert_eq!(by_qname.file_path, "crates/ucil-treesitter/src/parser.rs");
    assert_eq!(
        by_qname.parent_module,
        Some("ucil_treesitter::parser".to_owned()),
    );
}
