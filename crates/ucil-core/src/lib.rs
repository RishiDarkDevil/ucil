//! `ucil-core` — core types, incremental engine, knowledge graph, cache, fusion, CEQP.
//!
//! This crate is the dependency-free kernel of UCIL. All other crates depend on it.
//! This `lib.rs` only re-exports public sub-modules; all logic lives in sub-modules.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod otel;
pub mod schema_migration;
pub mod types;

/// Crate version, identical to the `Cargo.toml` package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Re-exports ────────────────────────────────────────────────────────────────

pub use otel::{init_tracer, shutdown_tracer};
pub use schema_migration::{MigrationError, SCHEMA_VERSION};
pub use types::{
    CeqpParams, Diagnostic, KnowledgeEntry, QueryPlan, ResponseEnvelope, Symbol, ToolGroup,
};
