//! SCIP P1 — cross-repo symbol indexer pipeline (`P2-W7-F08`).
//!
//! Master-plan §3 line 284 classifies SCIP's interface verbatim as
//! **"CLI → `SQLite`"** — frozen.  §22 line 616 informs the authority
//! ranking ("Source authority as soft guidance: LSP/AST → SCIP → Dep
//! tools → KG → Text") that places `Scip` at `authority_rank == 4`,
//! below the four pre-existing G1 sources (Serena = 0, `TreeSitter` =
//! 1, `AstGrep` = 2, Diagnostics = 3).  §28 phase-log "external-deps
//! line" lists `scip-rust` and the `scip` CLI as Phase 2 Week 7
//! install prerequisites; §18 Phase 2 Week 7 line 1782 names the
//! verbatim feature description: "SCIP P1 install: scip-rust and scip
//! CLI produce a cross-repo symbol index for the fixture rust-project;
//! index loaded into `SQLite` and queried via G1".  §15.2 prescribes the
//! `ucil.<layer>.<op>` span hierarchy for the tracing-instrument
//! decoration on each public free function.
//!
//! Per `DEC-0014`: SCIP follows the CLI → `SQLite` pipeline pattern,
//! **NOT** the WO-0044 stdio-MCP plugin pattern.  scip-rust is a
//! one-shot language-specific indexer that emits an `index.scip`
//! protobuf file; it does not speak JSON-RPC over stdio.  The `scip`
//! CLI exposes forensic operations (`scip print`, `scip stats`,
//! `scip snapshot`, `scip lint`) — none of which is an MCP surface.
//! UCIL owns the `SQLite` ingest path: this module decodes the `.scip`
//! protobuf via the in-process `scip` crate (`DEC-0009` precedent for
//! in-process protobuf/regex decoding instead of shelling out to a
//! CLI on the hot path), writes rows to a UCIL-owned `SQLite` schema,
//! and exposes a query API.
//!
//! Public surface (consumed via `lib.rs` re-exports):
//!
//! * [`index_repo`] — runs the `scip-rust` indexer subprocess against
//!   a workspace and returns the absolute path of the produced
//!   `index.scip`.
//! * [`load_index_to_sqlite`] — decodes a `.scip` protobuf payload and
//!   writes one row per (document, occurrence) pair to the
//!   [`SCIP_SCHEMA`]-defined `scip_symbols` table.
//! * [`query_symbol`] — reads back rows whose `symbol` column matches
//!   a `LIKE`-pattern; results are sorted deterministically by
//!   `(file_path, start_line)`.
//! * [`ScipReference`] — typed projection of one `scip_symbols` row.
//! * [`ScipG1Source`] — implements the WO-0047
//!   [`crate::executor::G1Source`] trait, producing a
//!   [`crate::executor::G1ToolOutput`] with `kind ==
//!   G1ToolKind::Scip`.
//! * [`SCIP_INDEX_DEADLINE_SECS`] — subprocess deadline budget.
//! * [`ScipError`] — `thiserror::Error` enum with `#[non_exhaustive]`,
//!   covering subprocess spawn / exit / output / decode / sqlite paths.
//!
//! Acceptance test [`test_scip_p1_install`] lives at the module root
//! per `DEC-0007` so the frozen selector
//! `cargo test -p ucil-daemon scip::test_scip_p1_install` resolves
//! cleanly.

#![allow(clippy::module_name_repetitions)]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use protobuf::Message;
use scip::types::{Document, Index, SymbolInformation, SymbolRole};
use thiserror::Error;

use crate::executor::{
    G1FusedLocation, G1FusionEntry, G1Query, G1Source, G1ToolKind, G1ToolOutput, G1ToolStatus,
};

// ── Constants ────────────────────────────────────────────────────────────────

/// Subprocess deadline for [`index_repo`] — the budget within which
/// `scip-rust index --output <path>` must complete before this module
/// returns [`ScipError::IndexerTimedOut`].
///
/// Master-plan §15.2 implicit budget for an offline indexer; conservative
/// — a typical fixture index completes in 1-3 s, the 120 s budget gives
/// roughly 40× headroom for cold-cache `cargo build` paths the indexer
/// triggers internally on first invocation against a fresh workspace.
pub const SCIP_INDEX_DEADLINE_SECS: u64 = 120;

/// `SQLite` schema for the SCIP symbol cache.
///
/// One row per `(document, occurrence)` pair from the decoded `.scip`
/// payload.  `symbol` is the SCIP symbol identifier (e.g.
/// `rust-analyzer cargo ucil_treesitter 0.1.0 parser/`); `kind` is a
/// lowercase string projection of the `SymbolInformation.kind` enum
/// (`function`, `class`, `local`, ...); `file_path` is the
/// `Document.relative_path` field; `(start_line, end_line)` are
/// 1-based half-open line numbers extracted from the
/// `Occurrence.range` triple/quadruple; `role` is the lowercase string
/// projection of the `Occurrence.symbol_roles` bitset
/// (`definition`, `import`, `write_access`, `read_access`).
///
/// The `symbol`-column index speeds up the hot
/// [`query_symbol`] `LIKE` lookup; the `file_path`-column index speeds
/// up future find-references-by-file queries.
pub const SCIP_SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS scip_symbols (
        symbol     TEXT NOT NULL,
        kind       TEXT NOT NULL,
        file_path  TEXT NOT NULL,
        start_line INTEGER NOT NULL,
        end_line   INTEGER NOT NULL,
        role       TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS scip_symbols_symbol_idx ON scip_symbols(symbol);
    CREATE INDEX IF NOT EXISTS scip_symbols_file_idx   ON scip_symbols(file_path);
";

// ── Error type ───────────────────────────────────────────────────────────────

/// Errors emitted by the SCIP P1 pipeline.
///
/// `#[non_exhaustive]` per `.claude/rules/rust-style.md` so future
/// failure modes (`ScipError::FixtureNotFound`, ...) can land without
/// breaking downstream `match` sites that already have a `_` arm.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ScipError {
    /// `scip-rust` subprocess could not be spawned at all (binary not
    /// on `PATH`, permissions, etc.).
    #[error("failed to spawn `{command}`: {source}")]
    IndexerSpawn {
        /// Command-line invocation that failed to spawn.
        command: String,
        /// Underlying tokio/std I/O error.
        source: std::io::Error,
    },
    /// `scip-rust` exited with a non-zero status code.
    #[error("`{command}` exited with code {code}; stderr={stderr:?}")]
    IndexerExitCode {
        /// Command-line invocation that produced the failure.
        command: String,
        /// Exit code from the indexer.
        code: i32,
        /// Captured `stderr` payload (UTF-8 lossy decode).
        stderr: String,
    },
    /// `scip-rust` did not exit within [`SCIP_INDEX_DEADLINE_SECS`].
    #[error("`{command}` timed out after {secs} s")]
    IndexerTimedOut {
        /// Command-line invocation that hung.
        command: String,
        /// Wall-clock deadline applied via `tokio::time::timeout`.
        secs: u64,
    },
    /// `scip-rust` exited cleanly but did not produce the expected
    /// `index.scip` output file.
    #[error("indexer output file missing: {path}")]
    OutputMissing {
        /// Absolute path the indexer was supposed to populate.
        path: PathBuf,
    },
    /// Decoding the `.scip` protobuf payload failed.
    #[error("scip protobuf decode failed: {source}")]
    ProtobufDecode {
        /// Underlying `protobuf::Error` from the decoder.
        source: protobuf::Error,
    },
    /// `rusqlite` rejected a transaction operation (open / schema /
    /// insert / commit).
    #[error("sqlite error: {source}")]
    Sqlite {
        /// Underlying `rusqlite::Error`.
        source: rusqlite::Error,
    },
    /// Generic I/O failure (filesystem read, `tokio::fs::read`, ...).
    #[error("io error: {source}")]
    Io {
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// A path could not be encoded as UTF-8 for forwarding to the
    /// indexer (which accepts only UTF-8 CLI args).
    #[error("path is not valid UTF-8: {path}")]
    NonUtf8Path {
        /// Offending path.
        path: PathBuf,
    },
}

// ── Reference projection ─────────────────────────────────────────────────────

/// One row from the `scip_symbols` table — the typed projection
/// returned by [`query_symbol`] and consumed by [`ScipG1Source`].
///
/// Field shapes mirror the SCIP protobuf surface verbatim:
/// `symbol` is the SCIP symbol identifier;
/// `kind` is the lowercase string projection of
/// `scip::types::SymbolInformation.kind`; `file_path` is
/// `Document.relative_path`; `(start_line, end_line)` are 1-based
/// half-open line numbers; `role` is a comma-separated lowercase
/// string of the bits set in `Occurrence.symbol_roles` (e.g.
/// `"definition"`, `"definition,write_access"`, `""` for a plain
/// reference).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScipReference {
    /// SCIP symbol identifier (opaque string).
    pub symbol: String,
    /// Lowercase symbol-information kind.
    pub kind: String,
    /// File path relative to the workspace root.
    pub file_path: String,
    /// 1-based start line.
    pub start_line: u32,
    /// 1-based end line (equal to `start_line` for single-line
    /// occurrences).
    pub end_line: u32,
    /// Lowercase symbol-roles bitset projection.
    pub role: String,
}

// ── Indexer subprocess wrapper ───────────────────────────────────────────────

/// Run the `scip-rust` indexer over `repo_root` and write the produced
/// `.scip` payload into `output_dir/index.scip`.
///
/// The indexer is invoked with `current_dir = repo_root` so any
/// workspace-relative paths it writes into the protobuf payload are
/// expressed relative to the workspace, matching the
/// `Document.relative_path` shape `query_symbol` later projects.
///
/// # Behaviour
///
/// 1. Ensure `output_dir` exists via `tokio::fs::create_dir_all`.
/// 2. Build the absolute target path `output_dir/index.scip`.
/// 3. Spawn `scip-rust index --output <target>` with stdout/stderr
///    captured + `kill_on_drop` so a panicking caller cannot leak the
///    child.
/// 4. Wait under [`SCIP_INDEX_DEADLINE_SECS`].  Timeout →
///    [`ScipError::IndexerTimedOut`]; non-zero exit →
///    [`ScipError::IndexerExitCode`] with captured stderr.
/// 5. Validate the output file exists; absent →
///    [`ScipError::OutputMissing`].
/// 6. Return the absolute path of the produced `.scip` file.
///
/// # Errors
///
/// * [`ScipError::Io`] — the output directory could not be created.
/// * [`ScipError::NonUtf8Path`] — `output_dir` contains a non-UTF-8
///   component (the indexer accepts only UTF-8 CLI args).
/// * [`ScipError::IndexerSpawn`] — `scip-rust` is not on `PATH` or
///   spawn failed.
/// * [`ScipError::IndexerTimedOut`] — the subprocess did not exit
///   within [`SCIP_INDEX_DEADLINE_SECS`].
/// * [`ScipError::IndexerExitCode`] — the subprocess exited non-zero.
/// * [`ScipError::OutputMissing`] — the subprocess exited cleanly but
///   produced no output file.
///
/// # Examples
///
/// ```no_run
/// # use ucil_daemon::scip::index_repo;
/// # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
/// let repo = std::path::Path::new(".");
/// let outdir = std::path::Path::new("/tmp/scip-out");
/// let scip_path = index_repo(repo, outdir).await?;
/// assert!(scip_path.exists());
/// # Ok(()) }
/// ```
#[tracing::instrument(
    name = "ucil.daemon.scip.index_repo",
    level = "debug",
    skip(repo_root, output_dir),
    fields(
        repo_root = %repo_root.display(),
        output_dir = %output_dir.display(),
    ),
)]
pub async fn index_repo(repo_root: &Path, output_dir: &Path) -> Result<PathBuf, ScipError> {
    tokio::fs::create_dir_all(output_dir)
        .await
        .map_err(|source| ScipError::Io { source })?;

    let output_path = output_dir.join("index.scip");
    let output_str = output_path.to_str().ok_or_else(|| ScipError::NonUtf8Path {
        path: output_path.clone(),
    })?;

    let mut cmd = tokio::process::Command::new("scip-rust");
    cmd.current_dir(repo_root)
        .args(["index", "--output", output_str])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let child = cmd.spawn().map_err(|source| ScipError::IndexerSpawn {
        command: "scip-rust".to_owned(),
        source,
    })?;

    let wait = tokio::time::timeout(
        Duration::from_secs(SCIP_INDEX_DEADLINE_SECS),
        child.wait_with_output(),
    )
    .await;

    let output = match wait {
        Ok(Ok(o)) => o,
        Ok(Err(source)) => {
            return Err(ScipError::IndexerSpawn {
                command: "scip-rust".to_owned(),
                source,
            });
        }
        Err(_) => {
            return Err(ScipError::IndexerTimedOut {
                command: "scip-rust".to_owned(),
                secs: SCIP_INDEX_DEADLINE_SECS,
            });
        }
    };

    if !output.status.success() {
        return Err(ScipError::IndexerExitCode {
            command: "scip-rust".to_owned(),
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    if !output_path.exists() {
        return Err(ScipError::OutputMissing {
            path: output_path.clone(),
        });
    }

    Ok(output_path)
}

// ── SCIP → SQLite ingest ─────────────────────────────────────────────────────

/// Decode a `.scip` payload at `scip_path` and write one row per
/// `(document, occurrence)` pair into the [`SCIP_SCHEMA`]-defined
/// `scip_symbols` table at `db_path`.
///
/// # Behaviour
///
/// 1. `tokio::fs::read` the `.scip` payload off disk.
/// 2. Decode via the in-process `scip` Rust crate
///    (`Index::parse_from_bytes`) — DEC-0009 in-process precedent;
///    avoids a `scip print --json` shell-out on the hot path.
/// 3. Open `rusqlite::Connection` at `db_path` inside a
///    `tokio::task::spawn_blocking` so the synchronous `rusqlite`
///    work does not block the tokio worker pool.
/// 4. Execute the [`SCIP_SCHEMA`] DDL (idempotent — `CREATE TABLE IF
///    NOT EXISTS`).
/// 5. Open a single transaction and `INSERT` one row per
///    `(document, occurrence)` pair.  `kind` is looked up against the
///    document's `symbols` list (and as a fallback `external_symbols`
///    on the index); `role` is a comma-separated lowercase
///    projection of the `SymbolRole` bitset.  Lines are converted
///    from SCIP's 0-based `range` representation to 1-based
///    `(start_line, end_line)` to match other UCIL line-numbering
///    conventions (master-plan §11.2).
/// 6. Commit the transaction and return the inserted row count.
///
/// # Errors
///
/// * [`ScipError::Io`] — payload read failed.
/// * [`ScipError::ProtobufDecode`] — decoder rejected the payload.
/// * [`ScipError::Sqlite`] — open / DDL / insert / commit failed.
///
/// # Examples
///
/// ```no_run
/// # use ucil_daemon::scip::{load_index_to_sqlite};
/// # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
/// let scip = std::path::Path::new("/tmp/index.scip");
/// let db   = std::path::Path::new("/tmp/scip.db");
/// let row_count = load_index_to_sqlite(scip, db).await?;
/// assert!(row_count > 0);
/// # Ok(()) }
/// ```
#[tracing::instrument(
    name = "ucil.daemon.scip.load_index_to_sqlite",
    level = "debug",
    skip(scip_path, db_path),
    fields(
        scip_path = %scip_path.display(),
        db_path = %db_path.display(),
    ),
)]
pub async fn load_index_to_sqlite(scip_path: &Path, db_path: &Path) -> Result<usize, ScipError> {
    let bytes = tokio::fs::read(scip_path)
        .await
        .map_err(|source| ScipError::Io { source })?;

    let index =
        Index::parse_from_bytes(&bytes).map_err(|source| ScipError::ProtobufDecode { source })?;

    let db_path = db_path.to_owned();
    let row_count = tokio::task::spawn_blocking(move || -> Result<usize, ScipError> {
        let mut conn =
            rusqlite::Connection::open(&db_path).map_err(|source| ScipError::Sqlite { source })?;
        conn.execute_batch(SCIP_SCHEMA)
            .map_err(|source| ScipError::Sqlite { source })?;

        // Build the cross-document symbol → kind lookup once. Index
        // external_symbols first; document-local entries override on
        // the same key.
        let mut kind_index: HashMap<String, String> = HashMap::new();
        for ext in &index.external_symbols {
            kind_index.insert(ext.symbol.clone(), kind_string(ext));
        }
        for doc in &index.documents {
            for sym in &doc.symbols {
                kind_index.insert(sym.symbol.clone(), kind_string(sym));
            }
        }

        let tx = conn
            .transaction()
            .map_err(|source| ScipError::Sqlite { source })?;
        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO scip_symbols \
                       (symbol, kind, file_path, start_line, end_line, role) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .map_err(|source| ScipError::Sqlite { source })?;
            for doc in &index.documents {
                let file_path = doc.relative_path.as_str();
                for occ in &doc.occurrences {
                    if occ.symbol.is_empty() {
                        continue;
                    }
                    let (start_line, end_line) = decode_range(&occ.range);
                    let kind = kind_index.get(&occ.symbol).cloned().unwrap_or_default();
                    let role = role_string(occ.symbol_roles);
                    stmt.execute(rusqlite::params![
                        occ.symbol, kind, file_path, start_line, end_line, role,
                    ])
                    .map_err(|source| ScipError::Sqlite { source })?;
                    count += 1;
                }
            }
        }
        tx.commit().map_err(|source| ScipError::Sqlite { source })?;
        Ok(count)
    })
    .await
    .map_err(|join_err| ScipError::Io {
        source: std::io::Error::other(format!("spawn_blocking join error: {join_err}")),
    })??;

    Ok(row_count)
}

/// Convert a SCIP 0-based `range` triple/quadruple into 1-based
/// `(start_line, end_line)`.
///
/// Per the SCIP protobuf comment:
///
/// * 4-element range: `[startLine, startCharacter, endLine, endCharacter]`
/// * 3-element range: `[startLine, startCharacter, endCharacter]` —
///   `endLine == startLine`.
///
/// All values are 0-based on the wire; this helper increments to
/// 1-based to match other UCIL line-numbering conventions
/// (master-plan §11.2).  Empty / malformed ranges fall back to
/// `(0, 0)` — defensive default; the protobuf spec guarantees
/// well-formed ranges so this is reachable only on a corrupt
/// payload.
fn decode_range(range: &[i32]) -> (u32, u32) {
    match range.len() {
        4 => {
            let start = u32::try_from(range[0]).unwrap_or(0).saturating_add(1);
            let end = u32::try_from(range[2]).unwrap_or(0).saturating_add(1);
            (start, end)
        }
        3 => {
            let line = u32::try_from(range[0]).unwrap_or(0).saturating_add(1);
            (line, line)
        }
        _ => (0, 0),
    }
}

/// Lowercase string projection of a [`SymbolInformation`]'s `kind`
/// enum.  Returns `""` for `UnspecifiedKind`.
///
/// The full SCIP `Kind` enum has dozens of variants
/// (`Function`, `Method`, `Class`, `Trait`, ...); this helper
/// projects via the protobuf-generated `EnumOrUnknown::enum_value`
/// path rather than enumerating them by hand so new SCIP enum
/// values land automatically.
fn kind_string(sym: &SymbolInformation) -> String {
    sym.kind.enum_value().map_or_else(
        |_| String::new(),
        |k| {
            let s = format!("{k:?}");
            if s == "UnspecifiedKind" {
                String::new()
            } else {
                s.to_lowercase()
            }
        },
    )
}

/// Comma-separated lowercase projection of the [`SymbolRole`] bitset
/// stored on `Occurrence.symbol_roles`.
///
/// Empty string when no role bits are set (a plain reference).
/// Multiple bits stable-ordered by the wire enum order:
/// `definition,import,write_access,read_access,generated,test,forward_definition`.
fn role_string(bits: i32) -> String {
    const ALL: &[(SymbolRole, &str)] = &[
        (SymbolRole::Definition, "definition"),
        (SymbolRole::Import, "import"),
        (SymbolRole::WriteAccess, "write_access"),
        (SymbolRole::ReadAccess, "read_access"),
        (SymbolRole::Generated, "generated"),
        (SymbolRole::Test, "test"),
        (SymbolRole::ForwardDefinition, "forward_definition"),
    ];
    let mut out = String::new();
    for (role, name) in ALL {
        let mask = *role as i32;
        if bits & mask != 0 {
            if !out.is_empty() {
                out.push(',');
            }
            out.push_str(name);
        }
    }
    out
}

// `Document` is named so a future commit can ergonomically build a
// per-document filter on top of `load_index_to_sqlite` without
// re-importing the symbol.
const _: fn() = || {
    let _ = std::mem::size_of::<Document>();
};

// ── Query API ────────────────────────────────────────────────────────────────

/// Read back rows whose `symbol` column matches the `LIKE`-pattern
/// `%symbol%` from the `scip_symbols` table at `db_path`.
///
/// Results are sorted deterministically by `(file_path, start_line)`
/// so callers and the verifier never observe row-order flakes.
///
/// The `LIKE`-with-wildcards shape is the cross-repo equivalent of the
/// existing `KnowledgeGraph::resolve_symbol` substring lookup: SCIP
/// symbol identifiers are opaque structured strings (e.g.
/// `rust-analyzer cargo ucil_treesitter 0.1.0 parser/Parser#parse().`)
/// where the human-readable name lives in the middle, so a substring
/// match against an unqualified name (e.g. `evaluate`) surfaces every
/// definition + reference that has that token.  Production wiring of
/// a structured-symbol lookup is deferred to a future WO + ADR.
///
/// # Errors
///
/// * [`ScipError::Sqlite`] — open / prepare / query failed.
/// * [`ScipError::Io`] — `spawn_blocking` join failure.
///
/// # Examples
///
/// ```no_run
/// # use ucil_daemon::scip::query_symbol;
/// # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
/// let db = std::path::Path::new("/tmp/scip.db");
/// let refs = query_symbol(db, "evaluate").await?;
/// assert!(!refs.is_empty());
/// # Ok(()) }
/// ```
#[tracing::instrument(
    name = "ucil.daemon.scip.query_symbol",
    level = "debug",
    skip(db_path),
    fields(db_path = %db_path.display(), symbol = %symbol),
)]
pub async fn query_symbol(db_path: &Path, symbol: &str) -> Result<Vec<ScipReference>, ScipError> {
    let db_path = db_path.to_owned();
    let pattern = format!("%{symbol}%");

    let refs = tokio::task::spawn_blocking(move || -> Result<Vec<ScipReference>, ScipError> {
        let conn =
            rusqlite::Connection::open(&db_path).map_err(|source| ScipError::Sqlite { source })?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol, kind, file_path, start_line, end_line, role \
                 FROM scip_symbols \
                 WHERE symbol LIKE ?1 \
                 ORDER BY file_path ASC, start_line ASC",
            )
            .map_err(|source| ScipError::Sqlite { source })?;
        let rows = stmt
            .query_map(rusqlite::params![pattern], |row| {
                Ok(ScipReference {
                    symbol: row.get::<_, String>(0)?,
                    kind: row.get::<_, String>(1)?,
                    file_path: row.get::<_, String>(2)?,
                    start_line: row.get::<_, u32>(3)?,
                    end_line: row.get::<_, u32>(4)?,
                    role: row.get::<_, String>(5)?,
                })
            })
            .map_err(|source| ScipError::Sqlite { source })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|source| ScipError::Sqlite { source })?);
        }
        Ok(out)
    })
    .await
    .map_err(|join_err| ScipError::Io {
        source: std::io::Error::other(format!("spawn_blocking join error: {join_err}")),
    })??;

    Ok(refs)
}

// ── G1Source impl ────────────────────────────────────────────────────────────

/// `G1Source` impl backed by a SCIP `scip_symbols` `SQLite` store.
///
/// Holds an owned `db_path` so the same source can be cloned across
/// the orchestrator's fan-out without forcing the caller to share an
/// `Arc`.  `kind()` always reports [`G1ToolKind::Scip`]; `execute()`
/// invokes [`query_symbol`] against `db_path` and projects the
/// returned `Vec<ScipReference>` into a [`G1ToolOutput`] with
/// status [`G1ToolStatus::Available`] on success or
/// [`G1ToolStatus::Errored`] on a `SQLite` failure.
///
/// Per `DEC-0008`, this is **not** a critical-dep mock — it is a
/// real `G1Source` impl backed by a real on-disk `SQLite` store.  The
/// upstream subprocess (`scip-rust`) and the protobuf decode happen
/// upstream of this struct (in [`index_repo`] +
/// [`load_index_to_sqlite`]); the fan-out path is a real `SQLite`
/// `SELECT`.
#[derive(Debug, Clone)]
pub struct ScipG1Source {
    db_path: PathBuf,
}

impl ScipG1Source {
    /// Construct a new [`ScipG1Source`] backed by the `SQLite` store at
    /// `db_path`.  The path is not validated here — the first
    /// [`G1Source::execute`] call surfaces any failure as a
    /// [`G1ToolStatus::Errored`] output.
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }
}

#[async_trait::async_trait]
impl G1Source for ScipG1Source {
    fn kind(&self) -> G1ToolKind {
        G1ToolKind::Scip
    }

    async fn execute(&self, query: &G1Query) -> G1ToolOutput {
        let start = std::time::Instant::now();
        match query_symbol(&self.db_path, &query.symbol).await {
            Ok(refs) => {
                let entries: Vec<G1FusionEntry> = refs
                    .into_iter()
                    .map(|r| {
                        let mut fields = serde_json::Map::new();
                        fields.insert("symbol".to_owned(), serde_json::Value::String(r.symbol));
                        fields.insert("kind".to_owned(), serde_json::Value::String(r.kind));
                        fields.insert("role".to_owned(), serde_json::Value::String(r.role));
                        G1FusionEntry {
                            location: G1FusedLocation {
                                file_path: PathBuf::from(r.file_path),
                                start_line: r.start_line,
                                end_line: r.end_line,
                            },
                            fields,
                        }
                    })
                    .collect();
                let payload = serde_json::to_value(&entries).unwrap_or(serde_json::Value::Null);
                let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
                G1ToolOutput {
                    kind: G1ToolKind::Scip,
                    status: G1ToolStatus::Available,
                    elapsed_ms,
                    payload,
                    error: None,
                }
            }
            Err(e) => {
                let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
                G1ToolOutput {
                    kind: G1ToolKind::Scip,
                    status: G1ToolStatus::Errored,
                    elapsed_ms,
                    payload: serde_json::Value::Null,
                    error: Some(format!("scip query_symbol error: {e}")),
                }
            }
        }
    }
}

// ── Frozen acceptance test (DEC-0007 module-root placement) ──────────────────

/// Frozen acceptance test for `P2-W7-F08` (WO-0055 / DEC-0014).
///
/// Selector: `cargo test -p ucil-daemon scip::test_scip_p1_install` —
/// resolves to `ucil_daemon::scip::test_scip_p1_install` because the
/// test lives at MODULE ROOT (not inside `mod tests {}`) per
/// `DEC-0007`.
///
/// Six sub-assertions in order:
///
/// * **SA1 (`index_repo` round-trip)** — runs the real `scip-rust`
///   indexer subprocess against `tests/fixtures/rust-project`,
///   asserts a non-empty `index.scip` is produced.
/// * **SA2 (`load_index_to_sqlite`)** — decodes the `.scip` payload
///   via the real `scip` Rust crate, writes rows to a real `SQLite`
///   store at a `tempfile::TempDir`-managed path, asserts row count
///   `> 0` and the `scip_symbols` table exists.
/// * **SA3 (`query_symbol` against fixture symbol)** — queries
///   `evaluate` (the load-bearing fixture-anchor `pub fn evaluate` at
///   `tests/fixtures/rust-project/src/util.rs:128` per WO-0044
///   lessons line 165), asserts the result contains a `util.rs`
///   reference.
/// * **SA4 (`ScipG1Source` standalone)** — invokes
///   `G1Source::execute(&query)` on a `ScipG1Source` constructed
///   over the same db, asserts `kind == G1ToolKind::Scip`,
///   `status == G1ToolStatus::Available`, and entries non-empty.
/// * **SA5 (fan-into `execute_g1` orchestrator)** — fans the source
///   through `execute_g1` with the `G1_MASTER_DEADLINE` budget,
///   asserts the orchestrator outcome includes a `Scip`-kind result
///   with `status == Available`.
/// * **SA6 (`authority_rank` regression sentinel)** — calls
///   `crate::executor::authority_rank(G1ToolKind::Scip)` and asserts
///   it equals `4` (the rank value is load-bearing for fusion
///   ordering).
///
/// The `which::which("scip-rust").is_err()` guard at the top mirrors
/// the WO-0044 `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS` opt-out spirit but
/// does not consult an env-var: the verifier's job is to ensure
/// `scip-rust` IS on `PATH`; the guard exists only so a developer
/// without `scip-rust` can `cargo build --tests` cleanly without
/// failure spam.
#[cfg(test)]
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::uninlined_format_args)]
async fn test_scip_p1_install() {
    use crate::executor::{execute_g1, G1_MASTER_DEADLINE};

    if which::which("scip-rust").is_err() {
        eprintln!("[skip] scip-rust not on PATH — see scripts/devtools/install-scip-rust.sh");
        return;
    }

    let tmp = tempfile::TempDir::new().expect("tmpdir");
    let workspace_root: PathBuf = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("CARGO_MANIFEST_DIR");
    let repo_root = workspace_root
        .parent()
        .expect("crate parent")
        .parent()
        .expect("workspace root")
        .to_owned();
    let fixture = repo_root.join("tests/fixtures/rust-project");
    let scip_out_dir = tmp.path().join("scip");
    let db_path = tmp.path().join("scip.db");

    assert!(
        fixture.exists(),
        "fixture must exist; got path={:?}",
        fixture
    );

    // ── SA1: index_repo round-trip ───────────────────────────────────────
    let scip_path = index_repo(&fixture, &scip_out_dir)
        .await
        .expect("index_repo");
    assert!(
        scip_path.exists(),
        "index.scip must exist; got path={:?}",
        scip_path
    );
    let scip_size = scip_path.metadata().expect("metadata").len();
    assert!(
        scip_size > 0,
        "index.scip must be non-empty; got len={}",
        scip_size
    );

    // ── SA2: load_index_to_sqlite ────────────────────────────────────────
    let row_count = load_index_to_sqlite(&scip_path, &db_path)
        .await
        .expect("load_index_to_sqlite");
    assert!(
        row_count > 0,
        "row count must be > 0 from a real fixture index; got {}",
        row_count
    );
    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    let table_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='scip_symbols'",
            [],
            |r| r.get(0),
        )
        .expect("query sqlite_master");
    assert_eq!(
        table_count, 1,
        "scip_symbols table must exist; got {}",
        table_count
    );
    drop(conn);

    // ── SA3: query_symbol against fixture symbol ─────────────────────────
    let refs = query_symbol(&db_path, "evaluate")
        .await
        .expect("query_symbol");
    assert!(
        !refs.is_empty(),
        "query for 'evaluate' must return at least one ref against \
         tests/fixtures/rust-project/src/util.rs:128 (`pub fn evaluate`); got {:?}",
        refs
    );
    assert!(
        refs.iter().any(|r| r.file_path.ends_with("util.rs")),
        "at least one ref must be in util.rs; got file_paths={:?}",
        refs.iter().map(|r| &r.file_path).collect::<Vec<_>>()
    );

    // ── SA4: ScipG1Source standalone ─────────────────────────────────────
    let g1_source = ScipG1Source::new(db_path.clone());
    let query = G1Query {
        symbol: "evaluate".to_owned(),
        file_path: PathBuf::from("tests/fixtures/rust-project/src/util.rs"),
        line: 128,
        column: 8,
    };
    let output = G1Source::execute(&g1_source, &query).await;
    assert_eq!(
        output.kind,
        G1ToolKind::Scip,
        "output kind must be Scip; got {:?}",
        output.kind
    );
    assert_eq!(
        output.status,
        G1ToolStatus::Available,
        "ScipG1Source status must be Available; got {:?} (error={:?})",
        output.status,
        output.error
    );
    let entries: Vec<G1FusionEntry> = serde_json::from_value(output.payload.clone())
        .expect("payload deserialises into Vec<G1FusionEntry>");
    assert!(
        !entries.is_empty(),
        "entries non-empty; got count={}",
        entries.len()
    );

    // ── SA5: fan into execute_g1 orchestrator ────────────────────────────
    let outcome = execute_g1(
        query.clone(),
        vec![Box::new(g1_source) as Box<dyn G1Source + Send + Sync>],
        G1_MASTER_DEADLINE,
    )
    .await;
    assert!(
        outcome.results.iter().any(|r| r.kind == G1ToolKind::Scip),
        "orchestrator must include Scip in outcome results; got kinds={:?}",
        outcome.results.iter().map(|r| r.kind).collect::<Vec<_>>()
    );
    let scip_result = outcome
        .results
        .iter()
        .find(|r| r.kind == G1ToolKind::Scip)
        .expect("Scip result");
    assert_eq!(
        scip_result.status,
        G1ToolStatus::Available,
        "orchestrator-fanned Scip status must be Available; got {:?} (error={:?})",
        scip_result.status,
        scip_result.error
    );

    // ── SA6: authority_rank regression sentinel ──────────────────────────
    let rank = crate::executor::authority_rank(G1ToolKind::Scip);
    assert_eq!(
        rank, 4,
        "Scip authority rank must be 4 (lower than the 4 existing sources); got {}",
        rank
    );
}
