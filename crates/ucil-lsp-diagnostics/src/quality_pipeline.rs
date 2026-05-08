//! G7 quality-issues pipeline (`P1-W5-F05`, `WO-0016`).
//!
//! This module is the diagnostics → `quality_issues` feed that
//! master-plan §13.5 line 1437 describes: once the
//! [`crate::diagnostics::DiagnosticsClient`] returns an
//! `lsp_types::Diagnostic` payload, [`persist_diagnostics`] projects
//! every diagnostic into a §12.1 `quality_issues` row and writes the
//! whole batch through
//! [`ucil_core::KnowledgeGraph::execute_in_transaction`] — a single
//! `BEGIN IMMEDIATE` scope per call, so the §11 atomicity invariant
//! is preserved.
//!
//! # LSP-4 → quality-5 severity collapse
//!
//! The LSP 3.17 spec defines four `DiagnosticSeverity` values
//! (`Error`, `Warning`, `Information`, `Hint`), but the §12.1
//! `quality_issues` table uses a five-level severity string that the
//! fusion engine ranks by importance when surfacing issues to G7.  At
//! this WO time the mapping collapses the LSP ladder onto the subset
//! the fusion engine already expects:
//!
//! | LSP severity  | `severity` column | `category` column |
//! |---------------|-------------------|-------------------|
//! | `Error`       | `"high"`          | `"type_error"`    |
//! | `Warning`     | `"medium"`        | `"lint"`          |
//! | `Information` | `"low"`           | `"lint"`          |
//! | `Hint`        | `"info"`          | `"lint"`          |
//! | *absent*      | `"medium"`        | `"lint"`          |
//!
//! The fifth level (`"critical"`) is reserved for future promotion by
//! a rule-id allow-list (e.g. `RustBorrowCheckError`) but is not
//! emitted at this WO — see the rustdoc on
//! [`severity_to_quality`] for the rationale.  The absent-severity
//! row mirrors LSP's "no severity indication" fallback, which the
//! spec leaves server-defined; UCIL treats it as `"medium"` to avoid
//! silently downgrading a severity-less diagnostic into `"info"`.
//!
//! The mapping lives in rustdoc rather than an ADR because:
//!
//! * The choice is small, local to this module, and easy to revisit
//!   with a follow-up WO if the fusion engine's rank function is
//!   re-tuned.
//! * `DEC-0008` forbids `ucil-lsp-diagnostics` from taking a
//!   `ucil-daemon` dependency, so the mapping cannot live closer to
//!   the ranker without cycling.
//! * If a reviewer objects to the mapping, this WO's planner should
//!   pause and promote the mapping into an ADR before shipping.
//!
//! # Re-ingest semantics
//!
//! [`persist_diagnostics`] is a SELECT-then-UPSERT (`P3-W11-F06`,
//! `WO-0085`): each call SELECTs the existing row (if any) by
//! `(file_path, line_start, category, severity, message)` and either
//! UPDATEs `last_seen` + `resolved = 0` (preserving `first_seen`) or
//! INSERTs a fresh row.  After the per-row pass, any
//! `(file_path, …)`-matching rows whose `id` was NOT touched
//! transition to `resolved = 1` so the row count of unresolved
//! issues per file is the live truth from the latest LSP scan.
//!
//! Soft-delete of rows past their retention window is a separate
//! step — see [`soft_delete_resolved_quality_issues`] — which the
//! daemon's session-shutdown / periodic-cleanup orchestration calls.
//!
//! # Timeout discipline
//!
//! The `.await` inside [`persist_diagnostics`] goes through
//! [`crate::diagnostics::DiagnosticsClient::diagnostics`], which
//! already wraps the call in
//! `tokio::time::timeout(Duration::from_millis(LSP_REQUEST_TIMEOUT_MS), …)`.
//! This module deliberately adds **no** second timeout layer — a
//! double-wrap would mask the typed
//! [`crate::diagnostics::DiagnosticsClientError::Timeout`] variant
//! behind an opaque outer future and is an explicit anti-pattern per
//! the `WO-0015` surface contract.
//!
//! # Tracing spans
//!
//! The async body of [`persist_diagnostics`] opens a single span
//! named `ucil.lsp.persist_diagnostics` per master-plan §15.2
//! (`ucil.<layer>.<op>`).  Each INSERT loop iteration is a
//! `tracing::debug!` event — not a child span — to keep span
//! cardinality bounded when a file carries hundreds of diagnostics.

// `QualityPipelineError` legitimately repeats the module name — the
// module is named `quality_pipeline` because it scopes the exported
// surface around the G7 quality feed, and the error type would
// otherwise collide with the sibling `BridgeError`, `DiagnosticsClientError`.
// Allowing the lint at module scope keeps the naming consistent
// without per-item `#[allow]` spam, mirroring the decision in
// `diagnostics.rs`.
#![allow(clippy::module_name_repetitions)]

use std::time::{SystemTime, UNIX_EPOCH};

use lsp_types::{Diagnostic as LspDiagnostic, DiagnosticSeverity, NumberOrString, Url};
use thiserror::Error;
use ucil_core::{KnowledgeGraph, KnowledgeGraphError};

use crate::diagnostics::{DiagnosticsClient, DiagnosticsClientError};
use crate::types::Language;

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by [`persist_diagnostics`].
///
/// Marked `#[non_exhaustive]` so downstream crates cannot rely on
/// exhaustive matching — future Phase-1 work-orders will extend this
/// enum (e.g. with a dedicated variant for malformed `file://` URIs if
/// the Serena channel starts forwarding non-file URIs), and that
/// growth must not constitute a `SemVer` break.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum QualityPipelineError {
    /// The LSP dispatch through
    /// [`crate::diagnostics::DiagnosticsClient`] failed — timeout,
    /// transport, or any other variant surfaced by
    /// [`DiagnosticsClientError`].
    #[error("LSP dispatch failed: {0}")]
    Dispatch(#[from] DiagnosticsClientError),
    /// The `KnowledgeGraph` transaction failed — pragma miss, DDL
    /// rejection, or a `BEGIN IMMEDIATE` that could not acquire the
    /// write lock within the configured `busy_timeout` budget.
    #[error("knowledge graph write failed: {0}")]
    KnowledgeGraph(#[from] KnowledgeGraphError),
    /// The diagnostic's `uri` could not be converted into a local
    /// filesystem path.  This happens when the Serena channel
    /// forwards a non-`file://` URI (e.g. `untitled:`), which is
    /// legal per the LSP spec but has no `quality_issues.file_path`
    /// value.  The field carries the offending URI so the caller's
    /// log message can cite it verbatim.
    #[error("diagnostic URI is not a local file path: {uri}")]
    NonFileUri {
        /// The offending URI — typically `untitled:…` or an
        /// in-memory scheme the LSP server emits for unsaved buffers.
        uri: String,
    },
}

// ── Severity / category mapping ──────────────────────────────────────────────

/// Map an LSP [`DiagnosticSeverity`] to the §12.1 `severity` column
/// value.
///
/// See the module-level rustdoc for the full table.  The mapping is
/// intentionally lossy: LSP's four levels collapse onto a four-string
/// subset of the five-level §12.1 ladder — the `"critical"` level is
/// reserved for a future rule-id allow-list.
///
/// The `None` input case is not handled by this function because the
/// `lsp_types::Diagnostic.severity` field is `Option<DiagnosticSeverity>`;
/// [`persist_diagnostics`] unwraps the option with a `"medium"` /
/// `"lint"` default before calling this helper.
#[must_use]
pub const fn severity_to_quality(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::ERROR => "high",
        DiagnosticSeverity::WARNING => "medium",
        DiagnosticSeverity::INFORMATION => "low",
        // `DiagnosticSeverity::HINT` is the last known variant at
        // lsp-types 0.95; any future LSP-spec extension would arrive
        // with a new numeric code and `lsp-types` would emit a new
        // variant — at which point this helper must be extended and
        // a new mapping row documented in the module-level table.
        _ => "info",
    }
}

/// Map an LSP [`DiagnosticSeverity`] to the §12.1 `category` column
/// value.
///
/// See the module-level rustdoc for the full table.  `Error`
/// diagnostics map to `"type_error"` (the fusion engine treats this
/// category as a hard failure when ranking G7 output); every other
/// level maps to `"lint"`.
///
/// The `None` input case is not handled by this function — see the
/// [`severity_to_quality`] rustdoc.
#[must_use]
pub const fn category_from_severity(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::ERROR => "type_error",
        // Warnings / Information / Hint all collapse to `"lint"` at
        // this WO.  A follow-up WO may split the category by
        // rule-id (e.g. a clippy `perf` rule would be `"perf"`),
        // but that requires the rule-id allow-list `F08` will
        // introduce.
        _ => "lint",
    }
}

/// Default LSP server name for a given [`Language`].
///
/// Used by [`persist_diagnostics`] when the
/// [`LspDiagnostic`] carries no `source` field — the §12.1
/// `source_tool` column still needs a non-null string, so we fall
/// back to the canonical LSP server for the language.  The returned
/// value is prefixed with `"lsp:"` by the caller to match the
/// convention `"lsp:<server>"` that the fusion engine expects.
///
/// The mapping matches master-plan §2.2 Layer 1.5 defaults:
///
/// | Language    | Default server          |
/// |-------------|-------------------------|
/// | Python      | `pyright`               |
/// | Rust        | `rust-analyzer`         |
/// | TypeScript  | `tsserver`              |
/// | Go          | `gopls`                 |
/// | Java        | `jdtls`                 |
/// | C           | `clangd`                |
/// | Cpp         | `clangd`                |
#[must_use]
pub const fn language_default_server(language: Language) -> &'static str {
    match language {
        Language::Python => "pyright",
        Language::Rust => "rust-analyzer",
        Language::TypeScript => "tsserver",
        Language::Go => "gopls",
        Language::Java => "jdtls",
        Language::C | Language::Cpp => "clangd",
    }
}

// ── Row projection ───────────────────────────────────────────────────────────

/// `quality_issues` column projection for a single
/// [`LspDiagnostic`] + context.
///
/// Kept as an internal helper struct (rather than a public row type)
/// because `P1-W5-F05` is the only caller — future WOs that read back
/// rows will introduce their own row type in a different module.
struct QualityIssueRow<'a> {
    file_path: String,
    line_start: i64,
    line_end: i64,
    category: &'a str,
    severity: &'a str,
    message: String,
    rule_id: Option<String>,
    source_tool: String,
}

impl<'a> QualityIssueRow<'a> {
    /// Project an `lsp_types::Diagnostic` into a `quality_issues`
    /// row for the given `file_path` and `language`.
    ///
    /// Pure function — no IO.  Separated so the tests for the
    /// mapping + projection can run without an on-disk
    /// `KnowledgeGraph`.
    fn from_lsp(file_path: String, language: Language, diag: &'a LspDiagnostic) -> Self {
        // LSP `range` is zero-indexed per the spec; §12.1's
        // `line_start`/`line_end` are 1-indexed by the master-plan
        // convention (see the `entities` table's `start_line` /
        // `end_line` semantics).  Add 1 and coerce to `i64` so the
        // rusqlite bind type matches the `INTEGER` column.
        let line_start = i64::from(diag.range.start.line) + 1;
        let line_end = i64::from(diag.range.end.line) + 1;

        let severity = diag.severity.map_or("medium", severity_to_quality);
        let category = diag.severity.map_or("lint", category_from_severity);

        let rule_id = diag.code.as_ref().map(|code| match code {
            NumberOrString::Number(n) => n.to_string(),
            NumberOrString::String(s) => s.clone(),
        });

        let source_tool = format!(
            "lsp:{}",
            diag.source
                .as_deref()
                .unwrap_or_else(|| language_default_server(language)),
        );

        Self {
            file_path,
            line_start,
            line_end,
            category,
            severity,
            message: diag.message.clone(),
            rule_id,
            source_tool,
        }
    }
}

// ── URI → file path helper ───────────────────────────────────────────────────

/// Convert a `file://` URI into a local filesystem path string.
///
/// Returns [`QualityPipelineError::NonFileUri`] when the URI scheme
/// is anything other than `file`.  Kept private because no external
/// caller has a reason to surface this conversion.
fn uri_to_file_path(uri: &Url) -> Result<String, QualityPipelineError> {
    uri.to_file_path()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|()| QualityPipelineError::NonFileUri {
            uri: uri.to_string(),
        })
}

// ── persist_diagnostics ──────────────────────────────────────────────────────

/// Fetch diagnostics for `file_uri` via `client` and persist every
/// returned diagnostic as a row in the §12.1 `quality_issues` table.
///
/// All inserts land inside **one**
/// [`KnowledgeGraph::execute_in_transaction`] call, so the whole
/// batch commits atomically or rolls back on failure.  Returns the
/// number of rows inserted (equal to `diagnostics.len()` on success).
///
/// # Re-ingest behaviour
///
/// SELECT-then-UPSERT (`P3-W11-F06`, `WO-0085`): the per-row dedup key
/// is `(file_path, line_start, category, severity, message)` per
/// master-plan §12.1 row-uniqueness; on hit, the existing row's
/// `last_seen` is updated to `datetime('now')` and `resolved` is reset
/// to 0 (preserving the original `first_seen`).  On miss, a fresh row
/// is inserted with both `first_seen` and `last_seen` defaulted to
/// `datetime('now')`.  After the per-row pass, any unresolved row in
/// the same `file_path` that was NOT touched transitions to
/// `resolved = 1` so the `quality_issues` rows for that file mirror
/// the live LSP scan.
///
/// # Tracing
///
/// Opens a single span named `ucil.lsp.persist_diagnostics` with the
/// `file_uri` and `language` attributes; each row write is a
/// `tracing::debug!` event rather than a child span (so a file with
/// hundreds of diagnostics does not explode span cardinality).
///
/// # Errors
///
/// * [`QualityPipelineError::Dispatch`] — the LSP dispatch through
///   [`DiagnosticsClient`] failed (timeout, transport, etc.).
/// * [`QualityPipelineError::NonFileUri`] — `file_uri` has a scheme
///   other than `file://` (e.g. `untitled:` for an unsaved buffer).
/// * [`QualityPipelineError::KnowledgeGraph`] — the `BEGIN
///   IMMEDIATE` transaction could not be opened, the INSERT
///   statement failed, or the commit was rejected.
pub async fn persist_diagnostics(
    client: &DiagnosticsClient,
    kg: &mut KnowledgeGraph,
    file_uri: Url,
    language: Language,
) -> Result<usize, QualityPipelineError> {
    let span = tracing::info_span!(
        "ucil.lsp.persist_diagnostics",
        file_uri = %file_uri,
        language = ?language,
    );
    let _guard = span.enter();

    // Eagerly convert the URI to a filesystem path so the error
    // surfaces before we pay for the LSP round-trip.  The §12.1
    // `file_path` column is NOT NULL, so a non-file URI would fail
    // the INSERT anyway — we surface the typed error here instead.
    let file_path = uri_to_file_path(&file_uri)?;

    let diagnostics = client.diagnostics(file_uri.clone()).await?;

    let rows: Vec<QualityIssueRow<'_>> = diagnostics
        .iter()
        .map(|diag| QualityIssueRow::from_lsp(file_path.clone(), language, diag))
        .collect();

    let inserted = kg.execute_in_transaction(|tx| {
        // Step (a): SELECT existing row id by the §12.1 dedup key.
        let mut select_stmt = tx.prepare(
            "SELECT id FROM quality_issues \
             WHERE file_path = ?1 \
             AND COALESCE(line_start, -1) = COALESCE(?2, -1) \
             AND category = ?3 AND severity = ?4 AND message = ?5 \
             LIMIT 1;",
        )?;

        // Step (c): UPDATE on hit — preserve `first_seen`, advance
        // `last_seen`, reset `resolved` to 0.  M3 mutation contract
        // targets this statement: changing `SET last_seen = ...,
        // resolved = 0` to also `SET first_seen = datetime('now')`
        // would overwrite first_seen on every re-observation.
        let mut update_stmt = tx.prepare(
            "UPDATE quality_issues \
             SET last_seen = datetime('now'), resolved = 0 \
             WHERE id = ?1;",
        )?;

        // Step (d): INSERT on miss — `first_seen` defaults via the
        // schema; we explicitly set `last_seen = datetime('now')` so
        // SA `last_seen IS NOT NULL` holds on first observation.
        let mut insert_stmt = tx.prepare(
            "INSERT INTO quality_issues \
             (file_path, line_start, line_end, category, severity, message, \
              rule_id, source_tool, last_seen) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'));",
        )?;

        let mut count: usize = 0;
        let mut touched_ids: Vec<i64> = Vec::with_capacity(rows.len());
        for row in &rows {
            tracing::debug!(
                file_path = %row.file_path,
                line_start = row.line_start,
                severity = row.severity,
                category = row.category,
                "upserting quality_issues row",
            );
            let line_start_param: Option<i64> = Some(row.line_start);
            let existing: Option<i64> = select_stmt
                .query_row(
                    rusqlite::params![
                        row.file_path,
                        line_start_param,
                        row.category,
                        row.severity,
                        row.message,
                    ],
                    |r| r.get::<_, i64>(0),
                )
                .ok();

            if let Some(id) = existing {
                update_stmt.execute(rusqlite::params![id])?;
                touched_ids.push(id);
            } else {
                insert_stmt.execute(rusqlite::params![
                    row.file_path,
                    row.line_start,
                    row.line_end,
                    row.category,
                    row.severity,
                    row.message,
                    row.rule_id,
                    row.source_tool,
                ])?;
                touched_ids.push(tx.last_insert_rowid());
            }
            count += 1;
        }

        // Step (e) — resolve-transition: any unresolved row in the
        // same `file_path` that was NOT touched in the per-row pass
        // is no longer present in the latest LSP scan; flip
        // `resolved = 1` for those rows.  When `diagnostics` was
        // empty, `touched_ids` is empty too, and every unresolved row
        // for `file_path` resolves correctly.
        //
        // The `id NOT IN (?, ?, …)` placeholder list is built by
        // `format!`-rendering one `?` per touched id — the values are
        // bound through `params_from_iter` to keep the SQL injection
        // attack surface zero.  When `touched_ids.is_empty()`, we
        // build `id NOT IN ()`, which SQLite rejects, so the
        // statement is rewritten as `1 = 1` (i.e. "match every row")
        // for the empty-touched case.
        if touched_ids.is_empty() {
            tx.execute(
                "UPDATE quality_issues SET resolved = 1 \
                 WHERE file_path = ?1 AND resolved = 0;",
                rusqlite::params![file_path],
            )?;
        } else {
            let placeholders = (0..touched_ids.len())
                .map(|i| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "UPDATE quality_issues SET resolved = 1 \
                 WHERE file_path = ?1 AND resolved = 0 \
                 AND id NOT IN ({placeholders});",
            );
            let mut params: Vec<rusqlite::types::Value> = Vec::with_capacity(touched_ids.len() + 1);
            params.push(rusqlite::types::Value::Text(file_path.clone()));
            for id in &touched_ids {
                params.push(rusqlite::types::Value::Integer(*id));
            }
            tx.execute(&sql, rusqlite::params_from_iter(params.iter()))?;
        }

        Ok(count)
    })?;

    Ok(inserted)
}

// ── soft_delete_resolved_quality_issues ──────────────────────────────────────

/// Hand-rolled UTC formatter for a `SystemTime` rendered as
/// `YYYY-MM-DD HH:MM:SS` — the format `SQLite`'s `datetime('now')`
/// produces, so column comparisons stay byte-for-byte equal under
/// the lexicographic `TEXT` ordering.
///
/// Avoids pulling in `chrono` / `time` as a new direct dep on
/// `ucil-lsp-diagnostics` per `WO-0085` `scope_out` #6.  The
/// algorithm is the canonical civil-from-days proleptic-Gregorian
/// decomposition (no leap-second handling — UCIL never observes
/// `last_seen` outside the `SQLite` `datetime('now')` writer's
/// resolution).
fn format_utc_timestamp(t: SystemTime) -> String {
    // Treat negative offsets (pre-epoch) as `0` — that floor pins
    // any pathological caller-supplied `SystemTime` to the SQLite
    // `1970-01-01 00:00:00` epoch, well below any real `last_seen`
    // value, so the soft-delete cutoff stays sound.
    let secs = t.duration_since(UNIX_EPOCH).map_or(0_u64, |d| d.as_secs());

    let days = secs / 86_400;
    let time_of_day = secs % 86_400;
    let hour = time_of_day / 3_600;
    let minute = (time_of_day % 3_600) / 60;
    let second = time_of_day % 60;

    // Civil-from-days (Howard Hinnant's algorithm, MIT-licensed),
    // shifted so day 0 = 1970-01-01 (Unix epoch).  Yields a calendar
    // date for every non-negative `days` value.  We work entirely in
    // u64 because `secs` is non-negative and the algorithm only
    // overflows past year ~292 billion CE.
    let z_days: u64 = days + 719_468;
    let era: u64 = z_days / 146_097;
    let day_of_era: u64 = z_days - era * 146_097; // 0..=146096
    let yoe: u64 =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_civil: u64 = yoe + era * 400;
    let day_of_year: u64 = day_of_era - (365 * yoe + yoe / 4 - yoe / 100);
    let month_phase: u64 = (5 * day_of_year + 2) / 153;
    let day_of_month: u64 = day_of_year - (153 * month_phase + 2) / 5 + 1;
    let month_civil: u64 = if month_phase < 10 {
        month_phase + 3
    } else {
        month_phase - 9
    };
    let year_civil: u64 = year_civil + u64::from(month_civil <= 2);

    format!("{year_civil:04}-{month_civil:02}-{day_of_month:02} {hour:02}:{minute:02}:{second:02}")
}

/// Soft-delete `quality_issues` rows whose `resolved = 1` AND
/// `last_seen` is older than `now - retention_days` days.
///
/// Master-plan §12.1 + WO-0085 F06 prescribes the trend-tracking
/// retention contract: rows that have transitioned to `resolved = 1`
/// (via [`persist_diagnostics`]'s resolve-transition) stay in the
/// table for `retention_days` so historical queries can surface them;
/// once the retention window passes, this helper deletes them in a
/// single transaction.  Returns the number of rows deleted.
///
/// `now` and `retention_days` are caller-supplied so tests can drive
/// the cutoff deterministically without leaking the system clock into
/// the helper.
///
/// # Tracing
///
/// Opens a single span named
/// `ucil.lsp.soft_delete_resolved_quality_issues` per master-plan
/// §15.2 (`ucil.<layer>.<op>`).
///
/// # Errors
///
/// * [`QualityPipelineError::KnowledgeGraph`] — the `BEGIN IMMEDIATE`
///   transaction could not be opened, the DELETE statement failed, or
///   the commit was rejected.
#[tracing::instrument(
    name = "ucil.lsp.soft_delete_resolved_quality_issues",
    level = "debug",
    skip(kg),
    fields(retention_days)
)]
pub async fn soft_delete_resolved_quality_issues(
    kg: &mut KnowledgeGraph,
    now: SystemTime,
    retention_days: u32,
) -> Result<usize, QualityPipelineError> {
    let cutoff = now
        .checked_sub(std::time::Duration::from_secs(
            u64::from(retention_days) * 86_400,
        ))
        .unwrap_or(UNIX_EPOCH);
    let cutoff_str = format_utc_timestamp(cutoff);

    let deleted = kg.execute_in_transaction(|tx| {
        let mut stmt = tx.prepare(
            "DELETE FROM quality_issues \
             WHERE resolved = 1 AND last_seen IS NOT NULL AND last_seen < ?1;",
        )?;
        let n = stmt.execute(rusqlite::params![cutoff_str])?;
        Ok(n)
    })?;

    tracing::debug!(
        deleted,
        cutoff = %cutoff_str,
        retention_days,
        "soft-deleted resolved quality_issues rows past retention",
    );

    Ok(deleted)
}

// ── Test-side helpers ────────────────────────────────────────────────────────
//
// The nested `#[cfg(test)]` submodules below support the module-root
// tests (`test_*`).  `fake_serena_client` houses the real
// `SerenaClient` impl the tests drive per DEC-0008's
// dependency-inversion seam — it is **not** a mock of Serena's MCP
// wire format, just a concrete impl of UCIL's own trait.
// `test_fixtures` houses pure constructors for `lsp_types::Diagnostic`
// values and a `TempDir` + `KnowledgeGraph` opener so the tests stay
// under `clippy::too_many_lines` while still asserting column-for-column
// against `quality_issues` reads.

#[cfg(test)]
mod fake_serena_client {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use lsp_types::{
        CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall,
        Diagnostic as LspDiagnostic, TypeHierarchyItem, Url,
    };

    use crate::diagnostics::{DiagnosticsClientError, SerenaClient};

    /// `FakeSerenaClient` scripted to return a fixed
    /// diagnostics-by-URI map.  Any unscripted URI resolves to an
    /// empty diagnostic vector (mirroring LSP semantics where a file
    /// with no issues returns `[]`).
    ///
    /// The `call_hierarchy_*` / `type_hierarchy_supertypes` methods
    /// return empty vectors — they are unused by the `persist_diagnostics`
    /// tests but must still be implemented because the trait is
    /// object-safe and the impl is exhaustive.
    pub(super) struct ScriptedFakeSerenaClient {
        pub(super) diagnostics_by_uri: Mutex<Vec<(Url, Vec<LspDiagnostic>)>>,
    }

    impl ScriptedFakeSerenaClient {
        pub(super) fn new(scripted: Vec<(Url, Vec<LspDiagnostic>)>) -> Self {
            Self {
                diagnostics_by_uri: Mutex::new(scripted),
            }
        }
    }

    #[async_trait]
    impl SerenaClient for ScriptedFakeSerenaClient {
        async fn diagnostics(
            &self,
            uri: Url,
        ) -> Result<Vec<LspDiagnostic>, DiagnosticsClientError> {
            // Clone-out under the lock so the guard drops before the
            // `await` point (there is none here, but the discipline
            // keeps `clippy::await_holding_lock` happy regardless).
            let script = self
                .diagnostics_by_uri
                .lock()
                .expect("ScriptedFakeSerenaClient mutex poisoned")
                .clone();
            for (scripted_uri, diags) in script {
                if scripted_uri == uri {
                    return Ok(diags);
                }
            }
            Ok(Vec::new())
        }

        async fn call_hierarchy_incoming(
            &self,
            _item: CallHierarchyItem,
        ) -> Result<Vec<CallHierarchyIncomingCall>, DiagnosticsClientError> {
            Ok(Vec::new())
        }

        async fn call_hierarchy_outgoing(
            &self,
            _item: CallHierarchyItem,
        ) -> Result<Vec<CallHierarchyOutgoingCall>, DiagnosticsClientError> {
            Ok(Vec::new())
        }

        async fn type_hierarchy_supertypes(
            &self,
            _item: TypeHierarchyItem,
        ) -> Result<Vec<TypeHierarchyItem>, DiagnosticsClientError> {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod test_fixtures {
    use std::path::PathBuf;
    use std::sync::Arc;

    use lsp_types::{Diagnostic as LspDiagnostic, NumberOrString, Position, Range, Url};
    use tempfile::TempDir;
    use ucil_core::KnowledgeGraph;

    use super::fake_serena_client::ScriptedFakeSerenaClient;
    use crate::diagnostics::{DiagnosticsClient, SerenaClient};

    /// Construct an `lsp_types::Diagnostic` from a compact set of
    /// fields.  Saves ~15 lines per fixture relative to building the
    /// struct literal inline.
    pub(super) fn make_diag(
        start_line: u32,
        end_line: u32,
        severity: Option<lsp_types::DiagnosticSeverity>,
        code: Option<NumberOrString>,
        source: Option<&str>,
        message: &str,
    ) -> LspDiagnostic {
        LspDiagnostic {
            range: Range::new(Position::new(start_line, 0), Position::new(end_line, 1)),
            severity,
            code,
            code_description: None,
            source: source.map(str::to_owned),
            message: message.to_owned(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    /// Open a fresh on-disk `KnowledgeGraph` in a tempdir.  Returns
    /// the owner `TempDir` (drop-order-preserving — the caller must
    /// hold onto it for the lifetime of the test) alongside the
    /// opened `KnowledgeGraph`.
    pub(super) fn open_fresh_kg() -> (TempDir, KnowledgeGraph) {
        let tmp = TempDir::new().expect("tempdir must be creatable");
        let db_path = tmp.path().join("knowledge.db");
        let kg = KnowledgeGraph::open(&db_path).expect("KnowledgeGraph::open must succeed");
        (tmp, kg)
    }

    /// Wrap a pre-built `ScriptedFakeSerenaClient` into a
    /// `DiagnosticsClient`.  Hides the `Arc<dyn SerenaClient + Send +
    /// Sync>` coercion boilerplate from every test.
    pub(super) fn client_from(fake: ScriptedFakeSerenaClient) -> DiagnosticsClient {
        let shared: Arc<dyn SerenaClient + Send + Sync> = Arc::new(fake);
        DiagnosticsClient::new(shared)
    }

    /// Convert a `file://` URI into the `String` that
    /// `persist_diagnostics` will write into `quality_issues.file_path`.
    pub(super) fn uri_to_path_string(uri: &Url) -> String {
        let pb: PathBuf = uri.to_file_path().expect("file URI must convert to path");
        pb.to_string_lossy().into_owned()
    }

    /// Tuple alias for a full `quality_issues` row readback.
    pub(super) type IssueRow = (
        String,
        i64,
        i64,
        String,
        String,
        String,
        Option<String>,
        String,
    );

    /// Fetch the single `quality_issues` row matching `severity`.
    ///
    /// Used by the header test to assert every column of every row
    /// without repeating the 8-column projection inline three times.
    pub(super) fn fetch_row_by_severity(kg: &KnowledgeGraph, severity: &str) -> IssueRow {
        kg.conn()
            .query_row(
                "SELECT file_path, line_start, line_end, category, severity, message, rule_id, source_tool \
                 FROM quality_issues WHERE severity = ?1;",
                rusqlite::params![severity],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .unwrap_or_else(|e| panic!("row with severity={severity} must exist: {e}"))
    }

    /// Fetch `source_tool` for the (unique) row matching `file_path`.
    pub(super) fn fetch_source_tool_by_path(kg: &KnowledgeGraph, path: &str) -> String {
        kg.conn()
            .query_row(
                "SELECT source_tool FROM quality_issues WHERE file_path = ?1;",
                rusqlite::params![path],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|e| panic!("row with file_path={path} must exist: {e}"))
    }
}

// ── Module-root acceptance tests (F05 oracle) ────────────────────────────────
//
// The five tests below live at module root (NOT under `mod tests { … }`)
// per DEC-0005: the frozen nextest selector for `P1-W5-F05` is
// `test_diagnostics_to_quality_issues` (exact match), and keeping
// module-root placement means a future planner who tightens the
// selector gets `quality_pipeline::test_diagnostics_to_quality_issues`
// rather than `quality_pipeline::tests::…`.  The same rule cascades
// to the four supporting tests for consistency.

#[cfg(test)]
#[tokio::test]
async fn test_diagnostics_to_quality_issues() {
    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{
        client_from, fetch_row_by_severity, make_diag, open_fresh_kg, uri_to_path_string,
    };

    // Fixture: three canned diagnostics across two files.
    let uri_a = Url::parse("file:///fixture/main.rs").expect("file URI must parse");
    let uri_b = Url::parse("file:///fixture/lib.rs").expect("file URI must parse");

    let diag_error = make_diag(
        4,
        4,
        Some(DiagnosticSeverity::ERROR),
        Some(NumberOrString::String("E0308".to_owned())),
        Some("rust-analyzer"),
        "mismatched types",
    );
    let diag_warning = make_diag(
        10,
        10,
        Some(DiagnosticSeverity::WARNING),
        Some(NumberOrString::Number(42)),
        Some("clippy"),
        "unused variable",
    );
    let diag_hint = make_diag(
        0,
        0,
        Some(DiagnosticSeverity::HINT),
        None,
        None,
        "inlay-hint candidate",
    );

    let client = client_from(ScriptedFakeSerenaClient::new(vec![
        (uri_a.clone(), vec![diag_error, diag_warning]),
        (uri_b.clone(), vec![diag_hint]),
    ]));
    let (_tmp, mut kg) = open_fresh_kg();

    let rows_a = persist_diagnostics(&client, &mut kg, uri_a.clone(), Language::Rust)
        .await
        .expect("persist_diagnostics for uri_a must succeed");
    assert_eq!(rows_a, 2, "two rows expected from uri_a");

    let rows_b = persist_diagnostics(&client, &mut kg, uri_b.clone(), Language::Rust)
        .await
        .expect("persist_diagnostics for uri_b must succeed");
    assert_eq!(rows_b, 1, "one row expected from uri_b");

    let total_rows: i64 = kg
        .conn()
        .query_row("SELECT COUNT(*) FROM quality_issues;", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("SELECT COUNT(*) must succeed");
    assert_eq!(
        total_rows, 3,
        "exactly three rows must land in quality_issues"
    );

    let path_a = uri_to_path_string(&uri_a);
    let path_b = uri_to_path_string(&uri_b);

    let (fp, ls, le, cat, sev, msg, rid, src) = fetch_row_by_severity(&kg, "high");
    assert_eq!(fp, path_a);
    assert_eq!(ls, 5, "LSP line 4 projects to 1-indexed 5");
    assert_eq!(le, 5);
    assert_eq!(cat, "type_error");
    assert_eq!(sev, "high");
    assert_eq!(msg, "mismatched types");
    assert_eq!(rid.as_deref(), Some("E0308"));
    assert_eq!(src, "lsp:rust-analyzer");

    let (fp, ls, le, cat, sev, msg, rid, src) = fetch_row_by_severity(&kg, "medium");
    assert_eq!(fp, path_a);
    assert_eq!(ls, 11, "LSP line 10 projects to 1-indexed 11");
    assert_eq!(le, 11);
    assert_eq!(cat, "lint");
    assert_eq!(sev, "medium");
    assert_eq!(msg, "unused variable");
    assert_eq!(rid.as_deref(), Some("42"));
    assert_eq!(src, "lsp:clippy");

    let (fp, ls, le, cat, sev, msg, rid, src) = fetch_row_by_severity(&kg, "info");
    assert_eq!(fp, path_b);
    assert_eq!(ls, 1, "LSP line 0 projects to 1-indexed 1");
    assert_eq!(le, 1);
    assert_eq!(cat, "lint");
    assert_eq!(sev, "info");
    assert_eq!(msg, "inlay-hint candidate");
    assert_eq!(rid, None);
    assert_eq!(src, "lsp:rust-analyzer");

    let null_first_seen: i64 = kg
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM quality_issues WHERE first_seen IS NULL OR first_seen = '';",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("first_seen COUNT(*) must succeed");
    assert_eq!(
        null_first_seen, 0,
        "every row must have a non-empty first_seen timestamp",
    );
}

#[cfg(test)]
#[tokio::test]
async fn test_persist_empty_diagnostics_returns_zero() {
    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{client_from, open_fresh_kg};

    let uri = Url::parse("file:///fixture/empty.py").expect("file URI must parse");
    let client = client_from(ScriptedFakeSerenaClient::new(vec![(
        uri.clone(),
        Vec::new(),
    )]));
    let (_tmp, mut kg) = open_fresh_kg();

    let inserted = persist_diagnostics(&client, &mut kg, uri, Language::Python)
        .await
        .expect("persist_diagnostics on empty diagnostics must succeed");
    assert_eq!(
        inserted, 0,
        "empty diagnostic list must insert zero rows and return 0",
    );

    let rows: i64 = kg
        .conn()
        .query_row("SELECT COUNT(*) FROM quality_issues;", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("SELECT COUNT(*) must succeed");
    assert_eq!(rows, 0, "no rows must land when diagnostic list is empty");
}

#[cfg(test)]
#[test]
fn test_severity_mapping_covers_all_lsp_levels() {
    assert_eq!(severity_to_quality(DiagnosticSeverity::ERROR), "high");
    assert_eq!(severity_to_quality(DiagnosticSeverity::WARNING), "medium");
    assert_eq!(severity_to_quality(DiagnosticSeverity::INFORMATION), "low");
    assert_eq!(severity_to_quality(DiagnosticSeverity::HINT), "info");
}

#[cfg(test)]
#[test]
fn test_category_mapping_covers_all_lsp_levels() {
    assert_eq!(
        category_from_severity(DiagnosticSeverity::ERROR),
        "type_error"
    );
    assert_eq!(category_from_severity(DiagnosticSeverity::WARNING), "lint");
    assert_eq!(
        category_from_severity(DiagnosticSeverity::INFORMATION),
        "lint"
    );
    assert_eq!(category_from_severity(DiagnosticSeverity::HINT), "lint");
}

#[cfg(test)]
#[tokio::test]
async fn test_source_tool_falls_back_to_language_default() {
    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{
        client_from, fetch_source_tool_by_path, make_diag, open_fresh_kg, uri_to_path_string,
    };

    // Diagnostic 1: source = None → fall back to Python default
    // (`pyright`).
    let uri_py = Url::parse("file:///fixture/app.py").expect("file URI must parse");
    let diag_no_source = make_diag(
        2,
        2,
        Some(DiagnosticSeverity::ERROR),
        None,
        None,
        "undefined name",
    );

    // Diagnostic 2: source = Some("ruff") → preserved verbatim.
    let uri_py_b = Url::parse("file:///fixture/ruff.py").expect("file URI must parse");
    let diag_ruff = make_diag(
        0,
        0,
        Some(DiagnosticSeverity::WARNING),
        Some(NumberOrString::String("F401".to_owned())),
        Some("ruff"),
        "imported but unused",
    );

    // Diagnostic 3: source = None on a Rust URI → fall back to
    // `rust-analyzer`.  Exercised via a separate call because the
    // Language parameter is per-call.
    let uri_rs = Url::parse("file:///fixture/lib.rs").expect("file URI must parse");
    let diag_rs_no_source = make_diag(
        7,
        7,
        Some(DiagnosticSeverity::ERROR),
        None,
        None,
        "use of unstable feature",
    );

    let client = client_from(ScriptedFakeSerenaClient::new(vec![
        (uri_py.clone(), vec![diag_no_source]),
        (uri_py_b.clone(), vec![diag_ruff]),
        (uri_rs.clone(), vec![diag_rs_no_source]),
    ]));
    let (_tmp, mut kg) = open_fresh_kg();

    persist_diagnostics(&client, &mut kg, uri_py.clone(), Language::Python)
        .await
        .expect("persist_diagnostics for uri_py must succeed");
    persist_diagnostics(&client, &mut kg, uri_py_b.clone(), Language::Python)
        .await
        .expect("persist_diagnostics for uri_py_b must succeed");
    persist_diagnostics(&client, &mut kg, uri_rs.clone(), Language::Rust)
        .await
        .expect("persist_diagnostics for uri_rs must succeed");

    assert_eq!(
        fetch_source_tool_by_path(&kg, &uri_to_path_string(&uri_py)),
        "lsp:pyright",
        "source=None on a Python URI must fall back to `lsp:pyright`",
    );
    assert_eq!(
        fetch_source_tool_by_path(&kg, &uri_to_path_string(&uri_py_b)),
        "lsp:ruff",
        "source=Some(\"ruff\") must be preserved as `lsp:ruff`",
    );
    assert_eq!(
        fetch_source_tool_by_path(&kg, &uri_to_path_string(&uri_rs)),
        "lsp:rust-analyzer",
        "source=None on a Rust URI must fall back to `lsp:rust-analyzer`",
    );
}

// ── Module-root acceptance test (P3-W11-F06 oracle) ──────────────────────────
//
// Per `DEC-0007` (frozen-selector module-root placement), the
// acceptance test `test_quality_issues_tracking` lives at MODULE ROOT
// (NOT inside `mod tests {}`) so the
// `feature-list.json` selector
// `-p ucil-lsp-diagnostics quality_pipeline::test_quality_issues_tracking`
// resolves cleanly without a `tests::` intermediate.

/// Frozen acceptance selector for feature `P3-W11-F06` — see
/// `ucil-build/feature-list.json` entry for
/// `-p ucil-lsp-diagnostics quality_pipeline::test_quality_issues_tracking`.
///
/// Drives [`persist_diagnostics`] + [`soft_delete_resolved_quality_issues`]
/// over a `ScriptedFakeSerenaClient` and asserts six SA-numbered
/// properties:
///
/// * **SA1 — First observation INSERTs**: 1 diagnostic →
///   `inserted == 1`, the row has `first_seen != NULL`,
///   `last_seen != NULL`, `resolved == 0`.
/// * **SA2 — Re-observation UPDATEs (preserves `first_seen`)**:
///   re-running with the SAME diagnostic → `inserted == 1`,
///   `first_seen` UNCHANGED, `resolved == 0`, `COUNT(*) == 1` (no
///   duplicate row).  This is the M3 mutation target.
/// * **SA3 — `last_seen` advances on re-observation**:
///   `last_seen >= first_seen`.
/// * **SA4 — Empty diagnostics resolve outstanding rows**: running
///   `persist_diagnostics` with an empty diagnostic vector for the
///   same file → row transitions to `resolved == 1`.
/// * **SA5 — Soft-delete past retention**: `now + 8 days` with
///   `retention_days = 7` → returns `1` (the resolved row is
///   deleted), `COUNT(*) == 0`.
/// * **SA6 — Soft-delete preserves rows within retention**:
///   re-running soft-delete with `now + 5 days` and the same
///   `retention_days = 7` (after re-INSERTing the row first) →
///   returns `0` (resolved row within retention is preserved).
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
#[tokio::test]
pub async fn test_quality_issues_tracking() {
    use std::time::{Duration as StdDuration, SystemTime};

    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{client_from, make_diag, open_fresh_kg, uri_to_path_string};

    let uri = Url::parse("file:///fixture/track.rs").expect("file URI must parse");
    let path = uri_to_path_string(&uri);
    let diag = make_diag(
        4,
        4,
        Some(DiagnosticSeverity::ERROR),
        Some(NumberOrString::String("E0308".to_owned())),
        Some("rust-analyzer"),
        "mismatched types",
    );

    let (_tmp, mut kg) = open_fresh_kg();

    // Step (c) — first observation INSERTs.
    let client_a = client_from(ScriptedFakeSerenaClient::new(vec![(
        uri.clone(),
        vec![diag.clone()],
    )]));
    let inserted_a = persist_diagnostics(&client_a, &mut kg, uri.clone(), Language::Rust)
        .await
        .expect("first persist_diagnostics must succeed");
    assert_eq!(
        inserted_a, 1,
        "(SA1a) inserted == 1 on first observation; left: {inserted_a}, right: 1"
    );

    let row_a: (i64, Option<String>, Option<String>, i64) = kg
        .conn()
        .query_row(
            "SELECT id, first_seen, last_seen, resolved FROM quality_issues \
             WHERE file_path = ?1 LIMIT 1;",
            rusqlite::params![path],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("SELECT after first INSERT must succeed");
    let first_seen_a = row_a.1.expect("(SA1b) first_seen must NOT be NULL");
    let last_seen_a = row_a.2.expect("(SA1c) last_seen must NOT be NULL");
    assert!(
        !first_seen_a.is_empty(),
        "(SA1b) first_seen must be non-empty; left: {first_seen_a:?}"
    );
    assert!(
        !last_seen_a.is_empty(),
        "(SA1c) last_seen must be non-empty; left: {last_seen_a:?}"
    );
    assert_eq!(
        row_a.3, 0,
        "(SA1d) resolved == 0 on first observation; left: {}, right: 0",
        row_a.3
    );
    let id_a = row_a.0;

    // Sleep just over 1 s so SQLite's `datetime('now')` (1-second
    // resolution) advances on the next call.  This is the
    // load-bearing wait that keeps SA3 (`last_seen >= first_seen` AND
    // observably advances) honest under the M3 mutation contract.
    tokio::time::sleep(StdDuration::from_millis(1_100)).await;

    // Step (d) — re-observation UPDATEs (preserves first_seen).
    let client_b = client_from(ScriptedFakeSerenaClient::new(vec![(
        uri.clone(),
        vec![diag.clone()],
    )]));
    let inserted_b = persist_diagnostics(&client_b, &mut kg, uri.clone(), Language::Rust)
        .await
        .expect("second persist_diagnostics must succeed");
    assert_eq!(
        inserted_b, 1,
        "(SA2a) inserted == 1 on UPSERT (UPDATE counts as 1); \
         left: {inserted_b}, right: 1"
    );

    let row_b: (i64, Option<String>, Option<String>, i64) = kg
        .conn()
        .query_row(
            "SELECT id, first_seen, last_seen, resolved FROM quality_issues \
             WHERE file_path = ?1 LIMIT 1;",
            rusqlite::params![path],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("SELECT after re-observation UPDATE must succeed");
    assert_eq!(
        row_b.0, id_a,
        "(SA2b) UPSERT keeps the same row id; left: {}, right: {}",
        row_b.0, id_a
    );
    let first_seen_b = row_b.1.expect("(SA2c) first_seen must NOT be NULL");
    assert_eq!(
        first_seen_b, first_seen_a,
        "(SA2c) first_seen UNCHANGED across re-observation; \
         left: {first_seen_b:?}, right: {first_seen_a:?}"
    );
    let last_seen_b = row_b.2.expect("(SA3) last_seen must NOT be NULL");
    assert!(
        last_seen_b.as_str() >= first_seen_b.as_str(),
        "(SA3) last_seen >= first_seen on re-observation; \
         left: {last_seen_b:?}, right: {first_seen_b:?}"
    );
    assert_eq!(
        row_b.3, 0,
        "(SA2d) resolved == 0 after re-observation; left: {}, right: 0",
        row_b.3
    );

    let count_b: i64 = kg
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM quality_issues WHERE file_path = ?1;",
            rusqlite::params![path],
            |row| row.get::<_, i64>(0),
        )
        .expect("COUNT after re-observation must succeed");
    assert_eq!(
        count_b, 1,
        "(SA2e) COUNT(*) == 1 — UPSERT did not insert duplicate; \
         left: {count_b}, right: 1"
    );

    // Step (e) — empty diagnostics resolve outstanding rows.
    let client_c = client_from(ScriptedFakeSerenaClient::new(vec![(
        uri.clone(),
        Vec::new(),
    )]));
    let inserted_c = persist_diagnostics(&client_c, &mut kg, uri.clone(), Language::Rust)
        .await
        .expect("empty-diagnostics persist_diagnostics must succeed");
    assert_eq!(
        inserted_c, 0,
        "(SA4a) empty diagnostics → inserted == 0; \
         left: {inserted_c}, right: 0"
    );
    let resolved_c: i64 = kg
        .conn()
        .query_row(
            "SELECT resolved FROM quality_issues WHERE id = ?1;",
            rusqlite::params![id_a],
            |row| row.get::<_, i64>(0),
        )
        .expect("SELECT resolved after empty-scan must succeed");
    assert_eq!(
        resolved_c, 1,
        "(SA4b) row transitions to resolved=1 when omitted from a fresh scan; \
         left: {resolved_c}, right: 1"
    );

    // Step (f) — soft-delete past retention.  We can't easily set
    // SQLite's `last_seen` to a controllable past timestamp without
    // raw `UPDATE`s, so fast-forward `now` 8 days INTO THE FUTURE
    // relative to the row's `last_seen` and run with
    // `retention_days = 7`.
    let now_real = SystemTime::now();
    let now_8d = now_real + StdDuration::from_secs(8 * 86_400);
    let deleted_d = soft_delete_resolved_quality_issues(&mut kg, now_8d, 7)
        .await
        .expect("soft_delete_resolved_quality_issues must succeed");
    assert_eq!(
        deleted_d, 1,
        "(SA5a) one resolved row past retention is deleted; \
         left: {deleted_d}, right: 1"
    );
    let count_d: i64 = kg
        .conn()
        .query_row("SELECT COUNT(*) FROM quality_issues;", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("COUNT after soft-delete must succeed");
    assert_eq!(
        count_d, 0,
        "(SA5b) COUNT(*) == 0 after soft-delete; left: {count_d}, right: 0"
    );

    // Step (g) — sentinel: re-INSERT, then soft-delete with `now + 5
    // days` (within retention) returns 0.
    let client_e = client_from(ScriptedFakeSerenaClient::new(vec![(
        uri.clone(),
        vec![diag],
    )]));
    persist_diagnostics(&client_e, &mut kg, uri.clone(), Language::Rust)
        .await
        .expect("re-insert must succeed");
    // Resolve the freshly-inserted row by running an empty scan.
    let client_f = client_from(ScriptedFakeSerenaClient::new(vec![(
        uri.clone(),
        Vec::new(),
    )]));
    persist_diagnostics(&client_f, &mut kg, uri.clone(), Language::Rust)
        .await
        .expect("resolve-transition must succeed");

    let now_5d = now_real + StdDuration::from_secs(5 * 86_400);
    let deleted_f = soft_delete_resolved_quality_issues(&mut kg, now_5d, 7)
        .await
        .expect("soft_delete within retention must succeed");
    assert_eq!(
        deleted_f, 0,
        "(SA6) resolved row within retention is preserved; \
         left: {deleted_f}, right: 0"
    );
    let count_f: i64 = kg
        .conn()
        .query_row("SELECT COUNT(*) FROM quality_issues;", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("COUNT after within-retention soft-delete must succeed");
    assert_eq!(
        count_f, 1,
        "(SA6) row preserved within retention; left: {count_f}, right: 1"
    );
}
