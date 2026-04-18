//! `ucil-core` — core types, incremental engine, knowledge graph, cache, fusion, CEQP.
//!
//! This crate is the dependency-free kernel of UCIL. All other crates depend on it.
//! This `lib.rs` only re-exports public sub-modules; all logic lives in sub-modules.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod incremental;
pub mod knowledge_graph;
pub mod otel;
pub mod schema_migration;
pub mod types;

/// Crate version, identical to the `Cargo.toml` package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Re-exports ────────────────────────────────────────────────────────────────

pub use incremental::{dependent_metric, symbol_count, FileRevision, UcilDatabase, UcilDb};
// Grouped onto single lines so the WO-0024 acceptance greps
// (`pub use knowledge_graph::.*Entity` etc.) match without depending
// on rustfmt's wrapping heuristic — a single combined block exceeds
// the 100-col width and rustfmt would split the names onto their own
// lines, putting them past the `pub use` anchor.
pub use knowledge_graph::{Entity, Relation};
pub use knowledge_graph::{HotObservation, WalCheckpointMode};
pub use knowledge_graph::{KnowledgeGraph, KnowledgeGraphError};
pub use otel::{init_tracer, shutdown_tracer};
pub use schema_migration::{MigrationError, SCHEMA_VERSION};
pub use types::{
    CeqpParams, Diagnostic, KnowledgeEntry, QueryPlan, ResponseEnvelope, Symbol, ToolGroup,
};
