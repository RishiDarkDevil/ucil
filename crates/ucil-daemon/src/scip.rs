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

use std::path::PathBuf;

use thiserror::Error;

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

// ── Acceptance test scaffolding (placeholders for follow-up commits) ─────────

// `index_repo`, `load_index_to_sqlite`, `query_symbol`, `ScipG1Source`
// and the frozen acceptance test land in subsequent commits per the
// DEC-0005 module-coherence-driven commit ladder documented in the
// WO-0055 plan_summary.
