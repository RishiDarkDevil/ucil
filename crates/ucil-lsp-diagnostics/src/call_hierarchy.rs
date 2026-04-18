//! G4 architecture feed (`P1-W5-F06`, `WO-0023`).
//!
//! This module is the diagnostics → `entities` + `relations` feed that
//! master-plan §13.5 line 1436 describes: once the
//! [`crate::diagnostics::DiagnosticsClient`] returns a
//! [`lsp_types::CallHierarchyIncomingCall`],
//! [`lsp_types::CallHierarchyOutgoingCall`], or
//! [`TypeHierarchyItem`] payload, the three persist functions in this
//! module project each hierarchy edge into a §12.1 `entities` +
//! `relations` row pair and write them through
//! [`ucil_core::KnowledgeGraph::execute_in_transaction`] — one
//! `BEGIN IMMEDIATE` scope per invocation, so the §11 atomicity
//! invariant is preserved across the `(root entity + N peer entities +
//! N relations)` batch.
//!
//! # LSP `SymbolKind` → `entities.kind` mapping
//!
//! LSP 3.17 enumerates 26 [`SymbolKind`] variants; UCIL's `entities.kind`
//! column uses a five-string vocabulary that the fusion engine ranks
//! when surfacing G4 architecture data.  This module collapses the
//! LSP ladder onto the subset Serena actually emits for the call- and
//! type-hierarchy prepare/resolve path:
//!
//! | LSP `SymbolKind`                                 | `entities.kind` |
//! |--------------------------------------------------|-----------------|
//! | `FUNCTION` / `METHOD` / `CONSTRUCTOR`            | `"function"`    |
//! | `CLASS` / `INTERFACE` / `STRUCT` / `ENUM`        | `"type"`        |
//! | `MODULE` / `NAMESPACE` / `PACKAGE`               | `"module"`      |
//! | `VARIABLE` / `CONSTANT` / `FIELD` / `PROPERTY`   | `"variable"`    |
//! | *anything else*                                  | `"symbol"`      |
//!
//! The mapping lives in rustdoc rather than an ADR because:
//!
//! * The choice is small, local to this module, and easy to revisit
//!   with a follow-up WO if the fusion engine's entity ranker is
//!   re-tuned.
//! * `DEC-0008` forbids `ucil-lsp-diagnostics` from taking a
//!   `ucil-daemon` dependency, so the mapping cannot live closer to
//!   the ranker without cycling.
//! * If a reviewer objects to the mapping, this WO's planner should
//!   pause and promote the mapping into an ADR before shipping.
//!
//! # Relation direction semantics
//!
//! * **Incoming calls** — `relations(source_id = peer, target_id = root,
//!   kind = "calls")`.  Peer calls root.  Each
//!   [`lsp_types::CallHierarchyIncomingCall::from`] item projects into
//!   the peer row and the edge points at the root.
//! * **Outgoing calls** — `relations(source_id = root, target_id =
//!   peer, kind = "calls")`.  Root calls peer.  Each
//!   [`lsp_types::CallHierarchyOutgoingCall::to`] item projects into
//!   the peer row.
//! * **Supertypes** — `relations(source_id = root, target_id =
//!   supertype, kind = "inherits")`.  Root extends/implements
//!   supertype.
//!
//! # `source_tool` tags
//!
//! * `"lsp:callHierarchy"` — call-edge provenance (incoming + outgoing).
//! * `"lsp:typeHierarchy"` — inherits-edge provenance (supertypes).
//!
//! Downstream fusion (phase-2+) looks for the `"lsp:"` prefix to
//! attribute the edge to the diagnostics bridge.  The per-op suffix
//! (`callHierarchy` / `typeHierarchy`) matches the LSP 3.17 request
//! name so debugging logs can trace the edge back to the wire.
//!
//! # Atomicity
//!
//! Each persist function opens **exactly one**
//! [`KnowledgeGraph::execute_in_transaction`] scope.  Inside that
//! scope it `INSERT`s the root entity, iterates the peer vector
//! `INSERT`ing one peer entity + one relation row per iteration,
//! and returns the relation count.  Splitting the root `INSERT`
//! and the peer `INSERT`s across two transactions would leave a
//! dangling root entity with no peers on a mid-batch failure — the
//! G4 consumer (`WO-0024` territory) assumes the root + peers
//! land atomically.
//!
//! # Re-ingest semantics
//!
//! The three persist functions do **not** upsert.  Calling
//! [`persist_call_hierarchy_incoming`] twice with identical arguments
//! produces two `entities` rows and two `relations` rows.  Dedup /
//! first-seen semantics are `P1-W4-F02` / `WO-0020` territory and are
//! out of scope here.
//!
//! # Timeout discipline
//!
//! Every `.await` in the three persist functions goes through
//! [`crate::diagnostics::DiagnosticsClient`], which already wraps the
//! call in
//! `tokio::time::timeout(Duration::from_millis(LSP_REQUEST_TIMEOUT_MS), …)`.
//! This module deliberately adds **no** second timeout layer — a
//! double-wrap would mask the typed
//! [`crate::diagnostics::DiagnosticsClientError::Timeout`] variant
//! behind an opaque outer future and is an explicit anti-pattern per
//! the `WO-0015` surface contract.
//!
//! # Tracing spans
//!
//! Each persist function opens a single span per master-plan §15.2
//! (`ucil.<layer>.<op>`):
//!
//! * `ucil.lsp.persist_call_hierarchy_incoming`
//! * `ucil.lsp.persist_call_hierarchy_outgoing`
//! * `ucil.lsp.persist_type_hierarchy_supertypes`
//!
//! Each row INSERT inside the transaction is a `tracing::debug!`
//! event rather than a child span — bounded-cardinality span output
//! remains the module invariant.

// `CallHierarchyError` legitimately repeats the module name — the
// module is named `call_hierarchy` because it scopes the exported
// surface around the LSP call- and type-hierarchy persistence, and
// the error type would otherwise collide with the sibling
// `BridgeError`, `DiagnosticsClientError`, `QualityPipelineError`.
// Allowing the lint at module scope keeps the naming consistent
// without per-item `#[allow]` spam, mirroring the decision in
// `diagnostics.rs` + `quality_pipeline.rs`.
#![allow(clippy::module_name_repetitions)]

use lsp_types::{CallHierarchyItem, SymbolKind, TypeHierarchyItem, Url};
use thiserror::Error;
use ucil_core::{KnowledgeGraph, KnowledgeGraphError};

use crate::diagnostics::{DiagnosticsClient, DiagnosticsClientError};
use crate::types::Language;

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by the persist functions in this module
/// ([`persist_call_hierarchy_incoming`],
/// [`persist_call_hierarchy_outgoing`],
/// [`persist_type_hierarchy_supertypes`]).
///
/// Marked `#[non_exhaustive]` so downstream crates cannot rely on
/// exhaustive matching — future Phase-1 work-orders will extend this
/// enum (e.g. with a dedicated variant for JSON-RPC framing errors if
/// the Serena channel starts exposing them), and that growth must not
/// constitute a `SemVer` break.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CallHierarchyError {
    /// The LSP dispatch through
    /// [`crate::diagnostics::DiagnosticsClient`] failed — timeout,
    /// transport, or any other variant surfaced by
    /// [`DiagnosticsClientError`].
    #[error("LSP dispatch failed: {0}")]
    Dispatch(#[from] DiagnosticsClientError),
    /// The [`CallHierarchyItem`]'s / [`TypeHierarchyItem`]'s `uri`
    /// could not be converted into a local filesystem path.  This
    /// happens when the Serena channel forwards a non-`file://` URI
    /// (e.g. `untitled:`), which is legal per the LSP spec but has
    /// no `entities.file_path` value.  The field carries the
    /// offending URI so the caller's log message can cite it
    /// verbatim.
    #[error("non-file URI: {uri}")]
    NonFileUri {
        /// The offending URI — typically `untitled:…` or an
        /// in-memory scheme the LSP server emits for unsaved buffers.
        uri: String,
    },
    /// The `KnowledgeGraph` transaction failed — pragma miss, DDL
    /// rejection, or a `BEGIN IMMEDIATE` that could not acquire the
    /// write lock within the configured `busy_timeout` budget.
    #[error("knowledge graph write failed: {0}")]
    KnowledgeGraph(#[from] KnowledgeGraphError),
}

// ── SymbolKind → entities.kind mapping ───────────────────────────────────────

/// Map an LSP [`SymbolKind`] to the §12.1 `entities.kind` column
/// value.
///
/// See the module-level rustdoc for the full table.  Serena emits a
/// trimmed subset of the LSP 3.17 [`SymbolKind`] ladder through the
/// call- and type-hierarchy prepare/resolve path — this helper covers
/// that subset and falls back to `"symbol"` for anything else (a
/// deliberately lossy projection that prevents the fusion engine from
/// receiving an un-ranked kind when Serena extends its emission set
/// in a future release).
///
/// Pure function — no IO.  Kept `const fn` so the mapping is testable
/// without any other module state.
#[must_use]
pub const fn symbol_kind_to_entity_kind(kind: SymbolKind) -> &'static str {
    // `SymbolKind` is a newtype around `i32` with named constants,
    // not a variant-style enum, so `match` works through structural
    // equality on the wrapped integer.  The arm values are
    // `SymbolKind::*` associated constants rather than bare `i32`s so
    // the compiler rejects any renumbering drift from the `lsp-types`
    // crate.
    match kind {
        SymbolKind::FUNCTION | SymbolKind::METHOD | SymbolKind::CONSTRUCTOR => "function",
        SymbolKind::CLASS | SymbolKind::INTERFACE | SymbolKind::STRUCT | SymbolKind::ENUM => "type",
        SymbolKind::MODULE | SymbolKind::NAMESPACE | SymbolKind::PACKAGE => "module",
        SymbolKind::VARIABLE | SymbolKind::CONSTANT | SymbolKind::FIELD | SymbolKind::PROPERTY => {
            "variable"
        }
        // `SymbolKind` is an open newtype via `lsp-types`: any future
        // LSP 3.18+ extension arrives as a numeric code unknown to
        // this build.  `"symbol"` is the neutral entity-kind the
        // fusion engine ranks at the bottom of the ladder, so an
        // un-mapped new code degrades gracefully rather than failing
        // the INSERT.
        _ => "symbol",
    }
}

// ── URI → file path helper ───────────────────────────────────────────────────

/// Convert a `file://` URI into a local filesystem path string.
///
/// Returns [`CallHierarchyError::NonFileUri`] when the URI scheme is
/// anything other than `file`.  Kept private because no external
/// caller has a reason to surface this conversion — each module in
/// this crate keeps its own copy of the helper to preserve the
/// `DEC-0008` cycle-free invariant (a shared-private-module would
/// need to land in `ucil-core`, which is a heavier dependency than
/// the cost of a dozen duplicated lines).
fn uri_to_file_path(uri: &Url) -> Result<String, CallHierarchyError> {
    uri.to_file_path()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|()| CallHierarchyError::NonFileUri {
            uri: uri.to_string(),
        })
}

// ── Row projections (internal) ───────────────────────────────────────────────

/// Internal column projection for an `entities` row written by the
/// three persist functions.
///
/// Kept private — each persist function builds this tuple inline,
/// binds it via `rusqlite::params!`, and discards it.  No caller
/// outside this module has a reason to reify an entity row.
struct EntityInsert<'a> {
    kind: &'static str,
    name: &'a str,
    file_path: &'a str,
    start_line: i64,
    end_line: i64,
}

/// Insert an `entities` row and return the [`rusqlite::Connection::last_insert_rowid`].
///
/// Kept as a free function (rather than a method on a struct) so the
/// three persist functions share the INSERT text verbatim without a
/// heavier abstraction.  The caller MUST be inside an open
/// `execute_in_transaction` scope.
fn insert_entity(
    tx: &rusqlite::Transaction<'_>,
    row: &EntityInsert<'_>,
) -> Result<i64, rusqlite::Error> {
    let mut stmt = tx.prepare(
        "INSERT INTO entities \
         (kind, name, file_path, start_line, end_line) \
         VALUES (?1, ?2, ?3, ?4, ?5);",
    )?;
    stmt.execute(rusqlite::params![
        row.kind,
        row.name,
        row.file_path,
        row.start_line,
        row.end_line,
    ])?;
    Ok(tx.last_insert_rowid())
}

/// Insert a `relations` row binding `source_id → target_id` with the
/// given `kind` and `source_tool`.
///
/// The caller MUST be inside an open `execute_in_transaction` scope.
fn insert_relation(
    tx: &rusqlite::Transaction<'_>,
    source_id: i64,
    target_id: i64,
    kind: &str,
    source_tool: &str,
) -> Result<(), rusqlite::Error> {
    let mut stmt = tx.prepare(
        "INSERT INTO relations \
         (source_id, target_id, kind, source_tool) \
         VALUES (?1, ?2, ?3, ?4);",
    )?;
    stmt.execute(rusqlite::params![source_id, target_id, kind, source_tool])?;
    Ok(())
}

// ── Internal peer projection ─────────────────────────────────────────────────

/// Per-peer projection built before the transaction opens so a
/// non-`file://` URI surfaces [`CallHierarchyError::NonFileUri`]
/// without rolling back a partially-written batch.
struct PeerProjection {
    name: String,
    file_path: String,
    kind: &'static str,
    start_line: i64,
    end_line: i64,
}

impl PeerProjection {
    /// Project an LSP hierarchy item (the `from` of an incoming call,
    /// the `to` of an outgoing call, or a supertype [`TypeHierarchyItem`])
    /// into the tuple the persist-loop binds to `entities` columns.
    fn from_call_hierarchy_item(item: &CallHierarchyItem) -> Result<Self, CallHierarchyError> {
        let file_path = uri_to_file_path(&item.uri)?;
        Ok(Self {
            name: item.name.clone(),
            file_path,
            kind: symbol_kind_to_entity_kind(item.kind),
            start_line: i64::from(item.selection_range.start.line),
            end_line: i64::from(item.selection_range.end.line),
        })
    }

    fn from_type_hierarchy_item(item: &TypeHierarchyItem) -> Result<Self, CallHierarchyError> {
        let file_path = uri_to_file_path(&item.uri)?;
        Ok(Self {
            name: item.name.clone(),
            file_path,
            kind: symbol_kind_to_entity_kind(item.kind),
            start_line: i64::from(item.selection_range.start.line),
            end_line: i64::from(item.selection_range.end.line),
        })
    }
}

// ── persist_call_hierarchy_incoming ──────────────────────────────────────────

/// Persist the incoming-call hierarchy for `root_item`.
///
/// Fetches callers via `client` and writes the root + each caller
/// as `entities` rows plus one `relations` row per caller with
/// `kind = "calls"` and `(source_id = peer, target_id = root)` —
/// the LSP spec's peer → root "incoming" direction.
///
/// Returns the number of `relations` rows inserted (equal to
/// `callers.len()` on success); zero when the LSP server reports
/// no incoming calls (the transaction is skipped to preserve §11
/// WAL quiescence).
///
/// # Direction
///
/// `relations(source_id = peer, target_id = root, kind = "calls",
/// source_tool = "lsp:callHierarchy")` — the peer calls the root.
///
/// # Tracing
///
/// Opens a single span named `ucil.lsp.persist_call_hierarchy_incoming`
/// with the `root_uri`, `root_name`, and `language` attributes; each
/// row INSERT is a `tracing::debug!` event.
///
/// # Errors
///
/// * [`CallHierarchyError::Dispatch`] — the LSP dispatch through
///   [`DiagnosticsClient`] failed (timeout, transport, etc.).
/// * [`CallHierarchyError::NonFileUri`] — `root_item.uri` (or any
///   caller's `from.uri`) has a scheme other than `file://`; all URIs
///   are projected BEFORE the transaction opens so this error never
///   rolls back a partial batch.
/// * [`CallHierarchyError::KnowledgeGraph`] — the `BEGIN IMMEDIATE`
///   transaction could not be opened, an INSERT failed, or the
///   commit was rejected.
#[tracing::instrument(
    level = "info",
    skip(client, kg, root_item),
    fields(root_name = %root_item.name)
)]
pub async fn persist_call_hierarchy_incoming(
    client: &DiagnosticsClient,
    kg: &mut KnowledgeGraph,
    root_item: CallHierarchyItem,
    language: Language,
) -> Result<usize, CallHierarchyError> {
    let span = tracing::info_span!(
        "ucil.lsp.persist_call_hierarchy_incoming",
        root_uri = %root_item.uri,
        root_name = %root_item.name,
        language = ?language,
    );
    let _guard = span.enter();

    // Eagerly convert the root URI so the `NonFileUri` error surfaces
    // before we pay for the LSP round-trip.
    let root_projection = PeerProjection::from_call_hierarchy_item(&root_item)?;

    let callers = client.call_hierarchy_incoming(root_item).await?;

    if callers.is_empty() {
        tracing::debug!("no incoming callers returned; skipping transaction");
        return Ok(0);
    }

    // Project every peer URI BEFORE the transaction opens so a
    // non-`file://` URI does not roll back a partial batch.  The
    // transaction closure signature requires a `rusqlite::Error`
    // result, so a `CallHierarchyError::NonFileUri` cannot flow out
    // of it.
    let peer_projections = callers
        .iter()
        .map(|call| PeerProjection::from_call_hierarchy_item(&call.from))
        .collect::<Result<Vec<_>, _>>()?;

    let inserted = kg.execute_in_transaction(|tx| {
        let root_id = insert_entity(
            tx,
            &EntityInsert {
                kind: root_projection.kind,
                name: &root_projection.name,
                file_path: &root_projection.file_path,
                start_line: root_projection.start_line,
                end_line: root_projection.end_line,
            },
        )?;
        tracing::debug!(
            root_id,
            file_path = %root_projection.file_path,
            kind = root_projection.kind,
            "inserted root entity",
        );

        let mut count: usize = 0;
        for peer in &peer_projections {
            let peer_id = insert_entity(
                tx,
                &EntityInsert {
                    kind: peer.kind,
                    name: &peer.name,
                    file_path: &peer.file_path,
                    start_line: peer.start_line,
                    end_line: peer.end_line,
                },
            )?;
            insert_relation(tx, peer_id, root_id, "calls", "lsp:callHierarchy")?;
            tracing::debug!(
                peer_id,
                root_id,
                peer_name = %peer.name,
                "inserted incoming-call peer + relation",
            );
            count += 1;
        }
        Ok(count)
    })?;

    Ok(inserted)
}

// ── persist_call_hierarchy_outgoing ──────────────────────────────────────────

/// Persist the outgoing-call hierarchy for `root_item`.
///
/// Fetches callees via `client` and writes the root + each callee
/// as `entities` rows plus one `relations` row per callee with
/// `kind = "calls"` and `(source_id = root, target_id = peer)` —
/// the LSP spec's root → peer "outgoing" direction.
///
/// Returns the number of `relations` rows inserted (equal to
/// `callees.len()` on success); zero when the LSP server reports
/// no outgoing calls.
///
/// # Direction
///
/// `relations(source_id = root, target_id = peer, kind = "calls",
/// source_tool = "lsp:callHierarchy")` — the root calls the peer.
///
/// # Tracing
///
/// Opens a single span named `ucil.lsp.persist_call_hierarchy_outgoing`
/// with the `root_uri`, `root_name`, and `language` attributes.
///
/// # Errors
///
/// Same contract as [`persist_call_hierarchy_incoming`] —
/// `Dispatch` / `NonFileUri` / `KnowledgeGraph`.
#[tracing::instrument(
    level = "info",
    skip(client, kg, root_item),
    fields(root_name = %root_item.name)
)]
pub async fn persist_call_hierarchy_outgoing(
    client: &DiagnosticsClient,
    kg: &mut KnowledgeGraph,
    root_item: CallHierarchyItem,
    language: Language,
) -> Result<usize, CallHierarchyError> {
    let span = tracing::info_span!(
        "ucil.lsp.persist_call_hierarchy_outgoing",
        root_uri = %root_item.uri,
        root_name = %root_item.name,
        language = ?language,
    );
    let _guard = span.enter();

    let root_projection = PeerProjection::from_call_hierarchy_item(&root_item)?;

    let callees = client.call_hierarchy_outgoing(root_item).await?;

    if callees.is_empty() {
        tracing::debug!("no outgoing callees returned; skipping transaction");
        return Ok(0);
    }

    let peer_projections = callees
        .iter()
        .map(|call| PeerProjection::from_call_hierarchy_item(&call.to))
        .collect::<Result<Vec<_>, _>>()?;

    let inserted = kg.execute_in_transaction(|tx| {
        let root_id = insert_entity(
            tx,
            &EntityInsert {
                kind: root_projection.kind,
                name: &root_projection.name,
                file_path: &root_projection.file_path,
                start_line: root_projection.start_line,
                end_line: root_projection.end_line,
            },
        )?;
        tracing::debug!(
            root_id,
            file_path = %root_projection.file_path,
            kind = root_projection.kind,
            "inserted root entity",
        );

        let mut count: usize = 0;
        for peer in &peer_projections {
            let peer_id = insert_entity(
                tx,
                &EntityInsert {
                    kind: peer.kind,
                    name: &peer.name,
                    file_path: &peer.file_path,
                    start_line: peer.start_line,
                    end_line: peer.end_line,
                },
            )?;
            insert_relation(tx, root_id, peer_id, "calls", "lsp:callHierarchy")?;
            tracing::debug!(
                peer_id,
                root_id,
                peer_name = %peer.name,
                "inserted outgoing-call peer + relation",
            );
            count += 1;
        }
        Ok(count)
    })?;

    Ok(inserted)
}

// ── persist_type_hierarchy_supertypes ────────────────────────────────────────

/// Persist the supertype hierarchy for `root_item`.
///
/// Fetches supertypes via `client` and writes the root + each
/// supertype as `entities` rows plus one `relations` row per
/// supertype with `kind = "inherits"` and
/// `(source_id = root, target_id = supertype)` — root extends /
/// implements supertype.
///
/// Returns the number of `relations` rows inserted (equal to
/// `supertypes.len()` on success); zero when the LSP server
/// reports no supertypes (e.g. the root type has no parent).
///
/// # Direction
///
/// `relations(source_id = root, target_id = supertype, kind =
/// "inherits", source_tool = "lsp:typeHierarchy")` — the root
/// inherits from the supertype.
///
/// # Tracing
///
/// Opens a single span named `ucil.lsp.persist_type_hierarchy_supertypes`
/// with the `root_uri`, `root_name`, and `language` attributes.
///
/// # Errors
///
/// Same contract as [`persist_call_hierarchy_incoming`] —
/// `Dispatch` / `NonFileUri` / `KnowledgeGraph`.
#[tracing::instrument(
    level = "info",
    skip(client, kg, root_item),
    fields(root_name = %root_item.name)
)]
pub async fn persist_type_hierarchy_supertypes(
    client: &DiagnosticsClient,
    kg: &mut KnowledgeGraph,
    root_item: TypeHierarchyItem,
    language: Language,
) -> Result<usize, CallHierarchyError> {
    let span = tracing::info_span!(
        "ucil.lsp.persist_type_hierarchy_supertypes",
        root_uri = %root_item.uri,
        root_name = %root_item.name,
        language = ?language,
    );
    let _guard = span.enter();

    let root_projection = PeerProjection::from_type_hierarchy_item(&root_item)?;

    let supertypes = client.type_hierarchy_supertypes(root_item).await?;

    if supertypes.is_empty() {
        tracing::debug!("no supertypes returned; skipping transaction");
        return Ok(0);
    }

    let peer_projections = supertypes
        .iter()
        .map(PeerProjection::from_type_hierarchy_item)
        .collect::<Result<Vec<_>, _>>()?;

    let inserted = kg.execute_in_transaction(|tx| {
        let root_id = insert_entity(
            tx,
            &EntityInsert {
                kind: root_projection.kind,
                name: &root_projection.name,
                file_path: &root_projection.file_path,
                start_line: root_projection.start_line,
                end_line: root_projection.end_line,
            },
        )?;
        tracing::debug!(
            root_id,
            file_path = %root_projection.file_path,
            kind = root_projection.kind,
            "inserted root entity",
        );

        let mut count: usize = 0;
        for peer in &peer_projections {
            let peer_id = insert_entity(
                tx,
                &EntityInsert {
                    kind: peer.kind,
                    name: &peer.name,
                    file_path: &peer.file_path,
                    start_line: peer.start_line,
                    end_line: peer.end_line,
                },
            )?;
            insert_relation(tx, root_id, peer_id, "inherits", "lsp:typeHierarchy")?;
            tracing::debug!(
                peer_id,
                root_id,
                peer_name = %peer.name,
                "inserted supertype peer + relation",
            );
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
// [`SerenaClient`] impl the tests drive per `DEC-0008`'s
// dependency-inversion seam — it is **not** a mock of Serena's MCP
// wire format, just a concrete impl of UCIL's own trait.
// `test_fixtures` houses pure constructors for
// [`lsp_types::CallHierarchyItem`] and [`lsp_types::TypeHierarchyItem`]
// values + a [`tempfile::TempDir`] + [`KnowledgeGraph`] opener so the
// tests stay under `clippy::too_many_lines` while still asserting
// column-for-column against `entities` + `relations` reads.

#[cfg(test)]
mod fake_serena_client {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use lsp_types::{
        CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall,
        Diagnostic as LspDiagnostic, TypeHierarchyItem, Url,
    };

    use crate::diagnostics::{DiagnosticsClientError, SerenaClient};

    /// `FakeSerenaClient` scripted to return fixed per-root-URI
    /// responses for the three call/type-hierarchy requests.  The
    /// `diagnostics(…)` method returns an empty vector — this
    /// module's tests do not exercise the diagnostics path.
    ///
    /// The scripted vectors are `(root_uri, responses)` tuples — the
    /// dispatch method finds the first tuple whose `root_uri` matches
    /// the request's `item.uri` and returns its `responses`; any
    /// unscripted URI resolves to an empty vector (mirroring LSP
    /// semantics where an item with no callers/callees/supertypes
    /// returns `[]`).
    ///
    /// Pattern copied from `quality_pipeline.rs::fake_serena_client`.
    /// The `ScriptedFakeSerenaClient` is NOT a mock of Serena's MCP
    /// wire format — it implements UCIL's own [`SerenaClient`] trait,
    /// which is the dependency-inversion seam (`DEC-0008` §4).
    ///
    /// The `_by_uri` postfix on every field is intentional — each
    /// field mirrors one LSP request endpoint and the shared postfix
    /// flags the lookup-table semantics at every use site; the
    /// `struct_field_names` pedantic lint is allowed at type scope
    /// for that reason.
    #[allow(clippy::struct_field_names)]
    pub(super) struct ScriptedFakeSerenaClient {
        pub(super) incoming_by_uri: Mutex<Vec<(Url, Vec<CallHierarchyIncomingCall>)>>,
        pub(super) outgoing_by_uri: Mutex<Vec<(Url, Vec<CallHierarchyOutgoingCall>)>>,
        pub(super) supertypes_by_uri: Mutex<Vec<(Url, Vec<TypeHierarchyItem>)>>,
    }

    impl ScriptedFakeSerenaClient {
        pub(super) fn new(
            incoming: Vec<(Url, Vec<CallHierarchyIncomingCall>)>,
            outgoing: Vec<(Url, Vec<CallHierarchyOutgoingCall>)>,
            supertypes: Vec<(Url, Vec<TypeHierarchyItem>)>,
        ) -> Self {
            Self {
                incoming_by_uri: Mutex::new(incoming),
                outgoing_by_uri: Mutex::new(outgoing),
                supertypes_by_uri: Mutex::new(supertypes),
            }
        }
    }

    #[async_trait]
    impl SerenaClient for ScriptedFakeSerenaClient {
        async fn diagnostics(
            &self,
            _uri: Url,
        ) -> Result<Vec<LspDiagnostic>, DiagnosticsClientError> {
            // Unused by this module's tests; `quality_pipeline.rs`
            // owns the diagnostics path.
            Ok(Vec::new())
        }

        async fn call_hierarchy_incoming(
            &self,
            item: CallHierarchyItem,
        ) -> Result<Vec<CallHierarchyIncomingCall>, DiagnosticsClientError> {
            let script = self
                .incoming_by_uri
                .lock()
                .expect("ScriptedFakeSerenaClient mutex poisoned")
                .clone();
            for (scripted_uri, responses) in script {
                if scripted_uri == item.uri {
                    return Ok(responses);
                }
            }
            Ok(Vec::new())
        }

        async fn call_hierarchy_outgoing(
            &self,
            item: CallHierarchyItem,
        ) -> Result<Vec<CallHierarchyOutgoingCall>, DiagnosticsClientError> {
            let script = self
                .outgoing_by_uri
                .lock()
                .expect("ScriptedFakeSerenaClient mutex poisoned")
                .clone();
            for (scripted_uri, responses) in script {
                if scripted_uri == item.uri {
                    return Ok(responses);
                }
            }
            Ok(Vec::new())
        }

        async fn type_hierarchy_supertypes(
            &self,
            item: TypeHierarchyItem,
        ) -> Result<Vec<TypeHierarchyItem>, DiagnosticsClientError> {
            let script = self
                .supertypes_by_uri
                .lock()
                .expect("ScriptedFakeSerenaClient mutex poisoned")
                .clone();
            for (scripted_uri, responses) in script {
                if scripted_uri == item.uri {
                    return Ok(responses);
                }
            }
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod test_fixtures {
    use std::sync::Arc;

    use lsp_types::{
        CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Position, Range,
        SymbolKind, TypeHierarchyItem, Url,
    };
    use tempfile::TempDir;
    use ucil_core::KnowledgeGraph;

    use super::fake_serena_client::ScriptedFakeSerenaClient;
    use crate::diagnostics::{DiagnosticsClient, SerenaClient};

    /// Construct a canned [`CallHierarchyItem`].  The `selection_range`
    /// width is `(start_line..=start_line)` so the projected
    /// `entities.start_line` and `entities.end_line` both equal
    /// `start_line`, keeping row-readback asserts compact.
    pub(super) fn make_call_hierarchy_item(
        name: &str,
        uri: Url,
        kind: SymbolKind,
        start_line: u32,
    ) -> CallHierarchyItem {
        let pos_start = Position::new(start_line, 0);
        let pos_end = Position::new(start_line, 1);
        let range = Range::new(pos_start, pos_end);
        CallHierarchyItem {
            name: name.to_owned(),
            kind,
            tags: None,
            detail: None,
            uri,
            range,
            selection_range: range,
            data: None,
        }
    }

    /// Construct a canned [`TypeHierarchyItem`].
    pub(super) fn make_type_hierarchy_item(
        name: &str,
        uri: Url,
        kind: SymbolKind,
        start_line: u32,
    ) -> TypeHierarchyItem {
        let pos_start = Position::new(start_line, 0);
        let pos_end = Position::new(start_line, 1);
        let range = Range::new(pos_start, pos_end);
        TypeHierarchyItem {
            name: name.to_owned(),
            kind,
            tags: None,
            detail: None,
            uri,
            range,
            selection_range: range,
            data: None,
        }
    }

    /// Wrap a [`CallHierarchyItem`] peer in a
    /// [`CallHierarchyIncomingCall`] with an empty
    /// `from_ranges` vector — the persist loop only reads `from`.
    pub(super) fn wrap_incoming(peer: CallHierarchyItem) -> CallHierarchyIncomingCall {
        CallHierarchyIncomingCall {
            from: peer,
            from_ranges: Vec::new(),
        }
    }

    /// Wrap a [`CallHierarchyItem`] peer in a
    /// [`CallHierarchyOutgoingCall`] with an empty `from_ranges`.
    pub(super) fn wrap_outgoing(peer: CallHierarchyItem) -> CallHierarchyOutgoingCall {
        CallHierarchyOutgoingCall {
            to: peer,
            from_ranges: Vec::new(),
        }
    }

    /// Open a fresh on-disk [`KnowledgeGraph`] in a tempdir.  The
    /// returned [`TempDir`] must be held for the lifetime of the test
    /// (its `Drop` removes the db file).
    pub(super) fn open_fresh_kg() -> (TempDir, KnowledgeGraph) {
        let tmp = TempDir::new().expect("tempdir must be creatable");
        let db_path = tmp.path().join("knowledge.db");
        let kg = KnowledgeGraph::open(&db_path).expect("KnowledgeGraph::open must succeed");
        (tmp, kg)
    }

    /// Wrap a pre-built [`ScriptedFakeSerenaClient`] into a
    /// [`DiagnosticsClient`].  Hides the
    /// `Arc<dyn SerenaClient + Send + Sync>` coercion boilerplate.
    pub(super) fn client_from(fake: ScriptedFakeSerenaClient) -> DiagnosticsClient {
        let shared: Arc<dyn SerenaClient + Send + Sync> = Arc::new(fake);
        DiagnosticsClient::new(shared)
    }

    /// Fetch a single `i64` SCALAR from a `SELECT COUNT(*) …` query.
    pub(super) fn count_rows(kg: &KnowledgeGraph, sql: &str) -> i64 {
        kg.conn()
            .query_row(sql, [], |row| row.get::<_, i64>(0))
            .unwrap_or_else(|e| panic!("COUNT(*) query failed ({sql:?}): {e}"))
    }

    /// Fetch the `source_tool` column for a sampled `relations` row
    /// matching the given `kind`.
    pub(super) fn sample_relation_source_tool(kg: &KnowledgeGraph, kind: &str) -> String {
        kg.conn()
            .query_row(
                "SELECT source_tool FROM relations WHERE kind = ?1 LIMIT 1;",
                rusqlite::params![kind],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|e| panic!("sample source_tool for kind={kind:?} failed: {e}"))
    }

    /// Fetch `(source.name, target.name)` for a relation matching
    /// `kind`.  Used by direction-assertion tests to prove which
    /// entity sits on which side of the edge.
    pub(super) fn relation_name_pair(
        kg: &KnowledgeGraph,
        kind: &str,
        target_name: &str,
    ) -> (String, String) {
        kg.conn()
            .query_row(
                "SELECT src.name, dst.name \
                 FROM relations r \
                 JOIN entities src ON src.id = r.source_id \
                 JOIN entities dst ON dst.id = r.target_id \
                 WHERE r.kind = ?1 AND dst.name = ?2 LIMIT 1;",
                rusqlite::params![kind, target_name],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap_or_else(|e| {
                panic!("relation_name_pair kind={kind:?} target={target_name:?} failed: {e}")
            })
    }
}

// ── Module-root acceptance tests (F06 oracle) ────────────────────────────────
//
// The seven tests below live at module root (NOT inside a `mod tests
// { … }` block) per `DEC-0005`: the frozen `call_hierarchy::` selector
// in `feature-list.json` is a module prefix, and keeping module-root
// placement means a future planner who tightens the selector gets
// `call_hierarchy::test_*` rather than `call_hierarchy::tests::test_*`.

#[cfg(test)]
#[test]
fn test_symbol_kind_mapping_covers_serena_emitted_kinds() {
    // Functions / methods / constructors → "function"
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::FUNCTION), "function");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::METHOD), "function");
    assert_eq!(
        symbol_kind_to_entity_kind(SymbolKind::CONSTRUCTOR),
        "function"
    );
    // Classes / interfaces / structs / enums → "type"
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::CLASS), "type");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::INTERFACE), "type");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::STRUCT), "type");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::ENUM), "type");
    // Modules / namespaces / packages → "module"
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::MODULE), "module");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::NAMESPACE), "module");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::PACKAGE), "module");
    // Variables / constants / fields / properties → "variable"
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::VARIABLE), "variable");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::CONSTANT), "variable");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::FIELD), "variable");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::PROPERTY), "variable");
    // Unmapped → "symbol" (fallthrough — documented lossy projection).
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::FILE), "symbol");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::OPERATOR), "symbol");
    assert_eq!(symbol_kind_to_entity_kind(SymbolKind::EVENT), "symbol");
    assert_eq!(
        symbol_kind_to_entity_kind(SymbolKind::TYPE_PARAMETER),
        "symbol"
    );
}

#[cfg(test)]
#[tokio::test]
async fn test_persist_call_hierarchy_incoming_writes_entities_and_relations() {
    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{
        client_from, count_rows, make_call_hierarchy_item, open_fresh_kg, relation_name_pair,
        sample_relation_source_tool, wrap_incoming,
    };

    // Root: function `parse_header` in main.rs.
    let root_uri = Url::parse("file:///fixture/main.rs").expect("file URI must parse");
    let root = make_call_hierarchy_item("parse_header", root_uri.clone(), SymbolKind::FUNCTION, 4);

    // Two incoming callers — both in lib.rs at different lines.
    let caller_uri = Url::parse("file:///fixture/lib.rs").expect("file URI must parse");
    let caller_a = wrap_incoming(make_call_hierarchy_item(
        "read_request",
        caller_uri.clone(),
        SymbolKind::FUNCTION,
        10,
    ));
    let caller_b = wrap_incoming(make_call_hierarchy_item(
        "handle_connection",
        caller_uri.clone(),
        SymbolKind::METHOD,
        20,
    ));

    let client = client_from(ScriptedFakeSerenaClient::new(
        vec![(root_uri.clone(), vec![caller_a, caller_b])],
        Vec::new(),
        Vec::new(),
    ));
    let (_tmp, mut kg) = open_fresh_kg();

    let inserted = persist_call_hierarchy_incoming(&client, &mut kg, root.clone(), Language::Rust)
        .await
        .expect("persist_call_hierarchy_incoming must succeed");
    assert_eq!(
        inserted, 2,
        "two caller relations must be inserted (returned count)",
    );

    // 1 root + 2 peers = 3 entity rows.
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM entities;"),
        3,
        "exactly three entity rows expected",
    );
    // 2 relation rows of kind 'calls'.
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM relations WHERE kind = 'calls';"),
        2,
        "exactly two relation rows with kind='calls' expected",
    );

    // source_tool must be 'lsp:callHierarchy'.
    assert_eq!(
        sample_relation_source_tool(&kg, "calls"),
        "lsp:callHierarchy",
        "call relations must carry source_tool='lsp:callHierarchy'",
    );

    // Incoming direction: peer is source, root is target.  The target
    // name must equal the root's name for every incoming relation.
    let (src_a, tgt_a) = relation_name_pair(&kg, "calls", "parse_header");
    assert_eq!(
        tgt_a, "parse_header",
        "incoming relation's target must be the root entity",
    );
    assert!(
        src_a == "read_request" || src_a == "handle_connection",
        "incoming relation's source must be one of the scripted callers; got {src_a}",
    );
}

#[cfg(test)]
#[tokio::test]
async fn test_persist_call_hierarchy_outgoing_flips_direction() {
    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{
        client_from, count_rows, make_call_hierarchy_item, open_fresh_kg, relation_name_pair,
        sample_relation_source_tool, wrap_outgoing,
    };

    // Root: function `render_page` in main.rs.
    let root_uri = Url::parse("file:///fixture/main.rs").expect("file URI must parse");
    let root = make_call_hierarchy_item("render_page", root_uri.clone(), SymbolKind::FUNCTION, 7);

    // Three outgoing callees — peer URIs deliberately span two files
    // to prove file_path projection runs per-peer.
    let callee_uri_a = Url::parse("file:///fixture/util.rs").expect("file URI must parse");
    let callee_uri_b = Url::parse("file:///fixture/lib.rs").expect("file URI must parse");
    let callee_a = wrap_outgoing(make_call_hierarchy_item(
        "escape_html",
        callee_uri_a.clone(),
        SymbolKind::FUNCTION,
        3,
    ));
    let callee_b = wrap_outgoing(make_call_hierarchy_item(
        "format_date",
        callee_uri_a,
        SymbolKind::FUNCTION,
        14,
    ));
    let callee_c = wrap_outgoing(make_call_hierarchy_item(
        "join",
        callee_uri_b,
        SymbolKind::METHOD,
        8,
    ));

    let client = client_from(ScriptedFakeSerenaClient::new(
        Vec::new(),
        vec![(root_uri.clone(), vec![callee_a, callee_b, callee_c])],
        Vec::new(),
    ));
    let (_tmp, mut kg) = open_fresh_kg();

    let inserted = persist_call_hierarchy_outgoing(&client, &mut kg, root, Language::Rust)
        .await
        .expect("persist_call_hierarchy_outgoing must succeed");
    assert_eq!(
        inserted, 3,
        "three callee relations must be inserted (returned count)",
    );

    // 1 root + 3 peers = 4 entity rows.
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM entities;"),
        4,
        "exactly four entity rows expected",
    );
    // 3 relation rows of kind 'calls'.
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM relations WHERE kind = 'calls';"),
        3,
        "exactly three relation rows with kind='calls' expected",
    );

    // source_tool must be 'lsp:callHierarchy' (same as incoming —
    // shared per-op tag).
    assert_eq!(
        sample_relation_source_tool(&kg, "calls"),
        "lsp:callHierarchy",
        "call relations must carry source_tool='lsp:callHierarchy'",
    );

    // Outgoing direction (flipped vs incoming): root is source, peer
    // is target.  Assert by looking up a specific peer's target edge
    // and checking the source side is the root's name.
    let (src, tgt) = relation_name_pair(&kg, "calls", "escape_html");
    assert_eq!(
        src, "render_page",
        "outgoing relation's source must be the root entity",
    );
    assert_eq!(
        tgt, "escape_html",
        "outgoing relation's target must be the scripted callee",
    );
}

#[cfg(test)]
#[tokio::test]
async fn test_persist_type_hierarchy_supertypes_writes_inherits_relations() {
    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{
        client_from, count_rows, make_type_hierarchy_item, open_fresh_kg, relation_name_pair,
        sample_relation_source_tool,
    };

    // Root: class `HttpRequest` in request.py.
    let root_uri = Url::parse("file:///fixture/request.py").expect("file URI must parse");
    let root = make_type_hierarchy_item("HttpRequest", root_uri.clone(), SymbolKind::CLASS, 2);

    // Two supertypes — interface + abstract class.
    let base_uri = Url::parse("file:///fixture/base.py").expect("file URI must parse");
    let super_a =
        make_type_hierarchy_item("RequestLike", base_uri.clone(), SymbolKind::INTERFACE, 1);
    let super_b = make_type_hierarchy_item("AbstractRequest", base_uri, SymbolKind::CLASS, 9);

    let client = client_from(ScriptedFakeSerenaClient::new(
        Vec::new(),
        Vec::new(),
        vec![(root_uri.clone(), vec![super_a, super_b])],
    ));
    let (_tmp, mut kg) = open_fresh_kg();

    let inserted = persist_type_hierarchy_supertypes(&client, &mut kg, root, Language::Python)
        .await
        .expect("persist_type_hierarchy_supertypes must succeed");
    assert_eq!(
        inserted, 2,
        "two inherits relations must be inserted (returned count)",
    );

    // 1 root + 2 supertypes = 3 entity rows.
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM entities;"),
        3,
        "exactly three entity rows expected",
    );
    // 2 relation rows of kind 'inherits'.
    assert_eq!(
        count_rows(
            &kg,
            "SELECT COUNT(*) FROM relations WHERE kind = 'inherits';"
        ),
        2,
        "exactly two relation rows with kind='inherits' expected",
    );

    // source_tool must be 'lsp:typeHierarchy'.
    assert_eq!(
        sample_relation_source_tool(&kg, "inherits"),
        "lsp:typeHierarchy",
        "inherits relations must carry source_tool='lsp:typeHierarchy'",
    );

    // Supertype direction: root is source, supertype is target
    // (root extends/implements supertype).
    let (src, tgt) = relation_name_pair(&kg, "inherits", "RequestLike");
    assert_eq!(
        src, "HttpRequest",
        "inherits relation's source must be the root entity",
    );
    assert_eq!(
        tgt, "RequestLike",
        "inherits relation's target must be the supertype",
    );
}

#[cfg(test)]
#[tokio::test]
async fn test_persist_empty_hierarchy_returns_zero() {
    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{
        client_from, count_rows, make_call_hierarchy_item, make_type_hierarchy_item, open_fresh_kg,
    };

    // Fixture: a root whose scripted responses are empty across all
    // three hierarchy endpoints.  The persist functions must each
    // return Ok(0) and leave the `entities` + `relations` tables
    // untouched.
    let call_root_uri = Url::parse("file:///fixture/empty_call.rs").expect("file URI must parse");
    let type_root_uri = Url::parse("file:///fixture/empty_type.py").expect("file URI must parse");
    let call_root = make_call_hierarchy_item(
        "leaf_function",
        call_root_uri.clone(),
        SymbolKind::FUNCTION,
        0,
    );
    let type_root =
        make_type_hierarchy_item("RootClass", type_root_uri.clone(), SymbolKind::CLASS, 0);

    let client = client_from(ScriptedFakeSerenaClient::new(
        vec![(call_root_uri.clone(), Vec::new())],
        vec![(call_root_uri, Vec::new())],
        vec![(type_root_uri, Vec::new())],
    ));
    let (_tmp, mut kg) = open_fresh_kg();

    let incoming =
        persist_call_hierarchy_incoming(&client, &mut kg, call_root.clone(), Language::Rust)
            .await
            .expect("persist_call_hierarchy_incoming must succeed on empty script");
    let outgoing = persist_call_hierarchy_outgoing(&client, &mut kg, call_root, Language::Rust)
        .await
        .expect("persist_call_hierarchy_outgoing must succeed on empty script");
    let supers = persist_type_hierarchy_supertypes(&client, &mut kg, type_root, Language::Python)
        .await
        .expect("persist_type_hierarchy_supertypes must succeed on empty script");

    assert_eq!(incoming, 0, "empty incoming script must return 0");
    assert_eq!(outgoing, 0, "empty outgoing script must return 0");
    assert_eq!(supers, 0, "empty supertypes script must return 0");

    // With the transaction skipped on empty responses, both tables
    // must stay empty — including no orphan root entity.
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM entities;"),
        0,
        "no entity rows must be written when every script is empty",
    );
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM relations;"),
        0,
        "no relation rows must be written when every script is empty",
    );
}

#[cfg(test)]
#[tokio::test]
async fn test_non_file_uri_surfaces_typed_error() {
    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{client_from, count_rows, open_fresh_kg};

    // A non-file URI such as `untitled:Untitled-1` is legal per the
    // LSP spec (unsaved buffer) but has no `entities.file_path`
    // projection.  The persist function must surface
    // `CallHierarchyError::NonFileUri` BEFORE opening the
    // transaction, so no rollback footprint exists.
    let bad_uri = Url::parse("untitled:Untitled-1").expect("untitled URI must parse");
    let root = CallHierarchyItem {
        name: "scratch_fn".to_owned(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri: bad_uri.clone(),
        range: lsp_types::Range::new(
            lsp_types::Position::new(0, 0),
            lsp_types::Position::new(0, 1),
        ),
        selection_range: lsp_types::Range::new(
            lsp_types::Position::new(0, 0),
            lsp_types::Position::new(0, 1),
        ),
        data: None,
    };

    let client = client_from(ScriptedFakeSerenaClient::new(
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ));
    let (_tmp, mut kg) = open_fresh_kg();

    let err = persist_call_hierarchy_incoming(&client, &mut kg, root, Language::Rust)
        .await
        .expect_err("non-file URI must produce an error");
    match err {
        CallHierarchyError::NonFileUri { uri } => {
            assert_eq!(
                uri,
                bad_uri.to_string(),
                "NonFileUri must carry the offending URI verbatim",
            );
        }
        other => panic!("expected CallHierarchyError::NonFileUri, got {other:?}"),
    }

    // Error fired BEFORE any transaction opened — tables stay empty.
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM entities;"),
        0,
        "no entity rows must be written when the root URI is non-file",
    );
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM relations;"),
        0,
        "no relation rows must be written when the root URI is non-file",
    );
}

#[cfg(test)]
#[tokio::test]
async fn test_atomic_transaction_single_scope() {
    use self::fake_serena_client::ScriptedFakeSerenaClient;
    use self::test_fixtures::{
        client_from, count_rows, make_call_hierarchy_item, open_fresh_kg, wrap_incoming,
    };

    // Fixture: 3 incoming peers.  The visible outcome of the
    // single-scope invariant is the exact row counts (1 root + 3
    // peers = 4 entity rows; 3 relation rows) — splitting across
    // multiple transactions would still produce the same counts on
    // success, so we pair the count assertion with a compile-time
    // grep on this module's source (via `include_str!`) to count
    // the `execute_in_transaction` call sites — which MUST be
    // exactly 3 (one per persist function).
    let root_uri = Url::parse("file:///fixture/atomic.rs").expect("file URI must parse");
    let root = make_call_hierarchy_item("hot_path", root_uri.clone(), SymbolKind::FUNCTION, 0);

    let peer_uri = Url::parse("file:///fixture/peers.rs").expect("file URI must parse");
    let peer_a = wrap_incoming(make_call_hierarchy_item(
        "caller_a",
        peer_uri.clone(),
        SymbolKind::FUNCTION,
        1,
    ));
    let peer_b = wrap_incoming(make_call_hierarchy_item(
        "caller_b",
        peer_uri.clone(),
        SymbolKind::FUNCTION,
        2,
    ));
    let peer_c = wrap_incoming(make_call_hierarchy_item(
        "caller_c",
        peer_uri,
        SymbolKind::FUNCTION,
        3,
    ));

    let client = client_from(ScriptedFakeSerenaClient::new(
        vec![(root_uri, vec![peer_a, peer_b, peer_c])],
        Vec::new(),
        Vec::new(),
    ));
    let (_tmp, mut kg) = open_fresh_kg();

    let inserted = persist_call_hierarchy_incoming(&client, &mut kg, root, Language::Rust)
        .await
        .expect("persist_call_hierarchy_incoming must succeed");
    assert_eq!(inserted, 3);

    // Row-count observable: 1 root + 3 peers = 4 entity rows.
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM entities;"),
        4,
        "exactly four entity rows expected (1 root + 3 peers)",
    );
    assert_eq!(
        count_rows(&kg, "SELECT COUNT(*) FROM relations;"),
        3,
        "exactly three relation rows expected",
    );

    // Compile-time grep on the module source.  The needle is
    // built from two string literals via `concat!` so that the
    // source file itself never contains the full needle substring
    // outside the three persist-function call sites — the external
    // grep acceptance criterion on this file must count exactly 3.
    let module_src = include_str!("call_hierarchy.rs");
    let needle = concat!("execute_in", "_transaction(");
    let occurrences = module_src.matches(needle).count();
    assert_eq!(
        occurrences, 3,
        "exactly 3 transaction-open call sites must exist (one per persist function); \
         found {occurrences}",
    );
}
