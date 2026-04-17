//! G7 quality-issues pipeline (`P1-W5-F05`, `WO-0016`).
//!
//! This module is the diagnostics → `quality_issues` feed that
//! master-plan §13.5 line 1437 describes: once the
//! [`DiagnosticsClient`](crate::diagnostics::DiagnosticsClient) returns
//! an `lsp_types::Diagnostic` payload, [`persist_diagnostics`] projects
//! every diagnostic into a §12.1 `quality_issues` row and writes the
//! whole batch through
//! [`KnowledgeGraph::execute_in_transaction`](ucil_core::KnowledgeGraph::execute_in_transaction)
//! — a single `BEGIN IMMEDIATE` scope per call, so the §11 atomicity
//! invariant is preserved.
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
//! [`persist_diagnostics`] does **not** upsert.  Calling it twice
//! with identical diagnostics produces two rows.  Dedup / first-seen
//! semantics are `P1-W5-F08` territory and are out of scope here.
//!
//! # Timeout discipline
//!
//! The `.await` inside [`persist_diagnostics`] goes through
//! [`DiagnosticsClient::diagnostics`](crate::diagnostics::DiagnosticsClient::diagnostics),
//! which already wraps the call in
//! `tokio::time::timeout(Duration::from_millis(LSP_REQUEST_TIMEOUT_MS), …)`.
//! This module deliberately adds **no** second timeout layer — a
//! double-wrap would mask the typed
//! [`DiagnosticsClientError::Timeout`](crate::diagnostics::DiagnosticsClientError::Timeout)
//! variant behind an opaque outer future and is an explicit
//! anti-pattern per the `WO-0015` surface contract.
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
    /// [`DiagnosticsClient`](crate::diagnostics::DiagnosticsClient)
    /// failed — timeout, transport, or any other variant surfaced by
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
/// This function does not upsert.  Two calls with the same `file_uri`
/// and the same diagnostic payload produce two rows.  Dedup / first-seen
/// semantics are `P1-W5-F08` territory.
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

    if diagnostics.is_empty() {
        tracing::debug!("no diagnostics returned; skipping transaction");
        return Ok(0);
    }

    let rows: Vec<QualityIssueRow<'_>> = diagnostics
        .iter()
        .map(|diag| QualityIssueRow::from_lsp(file_path.clone(), language, diag))
        .collect();

    let inserted = kg.execute_in_transaction(|tx| {
        let mut stmt = tx.prepare(
            "INSERT INTO quality_issues \
             (file_path, line_start, line_end, category, severity, message, rule_id, source_tool) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
        )?;

        let mut count: usize = 0;
        for row in &rows {
            tracing::debug!(
                file_path = %row.file_path,
                line_start = row.line_start,
                severity = row.severity,
                category = row.category,
                "inserting quality_issues row",
            );
            stmt.execute(rusqlite::params![
                row.file_path,
                row.line_start,
                row.line_end,
                row.category,
                row.severity,
                row.message,
                row.rule_id,
                row.source_tool,
            ])?;
            count += 1;
        }
        Ok(count)
    })?;

    Ok(inserted)
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
