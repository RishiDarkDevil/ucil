//! `ucil-lsp-diagnostics` — LSP diagnostics bridge: complements Serena with
//! call hierarchy, type hierarchy, and diagnostics integration.
//!
//! Per master-plan §13, this crate owns the thin bridge that the daemon
//! uses to surface LSP data into the fusion engine.  When Serena is
//! registered as an ACTIVE plugin UCIL delegates LSP responsibilities
//! to it (no duplicate subprocesses); otherwise the bridge spawns and
//! supervises its own language servers.  Per `DEC-0008`, the
//! Serena-delegation path runs through Serena's existing MCP channel
//! rather than a literal shared socket.
//!
//! This `lib.rs` only re-exports public sub-modules; all logic lives in
//! sub-modules.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod bridge;
pub mod call_hierarchy;
pub mod diagnostics;
pub mod quality_pipeline;
pub mod types;

// ── Re-exports ────────────────────────────────────────────────────────────────

pub use bridge::{BridgeError, LspDiagnosticsBridge};
pub use call_hierarchy::{
    persist_call_hierarchy_incoming, persist_call_hierarchy_outgoing,
    persist_type_hierarchy_supertypes, symbol_kind_to_entity_kind, CallHierarchyError,
};
pub use diagnostics::{
    DiagnosticsClient, DiagnosticsClientError, SerenaClient, LSP_REQUEST_TIMEOUT_MS,
};
pub use quality_pipeline::{
    category_from_severity, language_default_server, persist_diagnostics, severity_to_quality,
    QualityPipelineError,
};
pub use types::{Diagnostic, DiagnosticSeverity, Language, LspEndpoint, LspTransport};
