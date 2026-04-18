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
