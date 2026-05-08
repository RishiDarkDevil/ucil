//! `ucil-core` — core types, incremental engine, knowledge graph, cache, fusion, CEQP.
//!
//! This crate is the dependency-free kernel of UCIL. All other crates depend on it.
//! This `lib.rs` only re-exports public sub-modules; all logic lives in sub-modules.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod bonus_selector;
pub mod ceqp;
pub mod context_compiler;
pub mod cross_group;
pub mod fusion;
pub mod incremental;
pub mod knowledge_graph;
pub mod otel;
pub mod schema_migration;
pub mod tier_merger;
pub mod types;

/// Crate version, identical to the `Cargo.toml` package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Re-exports ────────────────────────────────────────────────────────────────

// Single-line per AC22 (WO-0056) — `#[rustfmt::skip]` blocks the
// 100-col wrap so the `grep -nE 'fuse_g2_rrf|rrf_weight|G2FusedHit|...'`
// returns all 8 new fusion symbols on the same line.
#[rustfmt::skip]
pub use fusion::{fuse_g2_rrf, rrf_weight, G2FusedHit, G2FusedOutcome, G2Hit, G2Source, G2SourceResults, G2_RRF_K};
// Single-line per AC22 / AC03 (WO-0088) — `#[rustfmt::skip]` blocks
// the 100-col wrap so `grep -qE '^pub use bonus_selector::\{' …`
// matches without depending on rustfmt's wrapping heuristic.
// P3-W10-F11 lands the public surface for downstream daemon-side
// production-wiring consumer WOs.
#[rustfmt::skip]
pub use bonus_selector::{BonusContextSource, BonusEntries, BonusSelectionOptions, HitWithBonus, select_bonus_context};
// Single-line per AC22 / AC03 (WO-0087 + WO-0088) — `#[rustfmt::skip]`
// blocks the 100-col wrap so `grep -qE '^pub use context_compiler::\{' …`
// matches without depending on rustfmt's wrapping heuristic.  P3-W10-F01
// lands `RepoMap` / `RankedSymbol`; P3-W10-F09 (WO-0088) extends the
// re-export with `AssembledResponse` / `ResponseMeta` / etc.
#[rustfmt::skip]
pub use context_compiler::{AssembledResponse, RankedSymbol, RepoMap, RepoMapError, RepoMapOptions, ResponseAssemblyOptions, ResponseMeta, assemble_response, hit_token_estimate};
// Single-line per AC22 (WO-0056) — `#[rustfmt::skip]` blocks the
// 100-col wrap so `grep -nE 'execute_cross_group|fuse_cross_group|GroupExecutor|...'`
// returns all 14 cross-group symbols on the same line. P3-W9-F03 / F04
// land the public surface for downstream daemon-side consumer WOs.
#[rustfmt::skip]
pub use cross_group::{execute_cross_group, fuse_cross_group, CrossGroupExecution, CrossGroupFusedHit, CrossGroupFusedOutcome, CrossGroupQuery, Group, GroupExecutor, GroupHit, GroupResult, GroupStatus, CROSS_GROUP_MASTER_DEADLINE, CROSS_GROUP_PER_GROUP_DEADLINE, CROSS_GROUP_RRF_K};
pub use incremental::{dependent_metric, symbol_count, FileRevision, UcilDatabase, UcilDb};
// Grouped onto single lines so the WO-0024 acceptance greps
// (`pub use knowledge_graph::.*Entity` etc.) match without depending
// on rustfmt's wrapping heuristic — a single combined block exceeds
// the 100-col width and rustfmt would split the names onto their own
// lines, putting them past the `pub use` anchor.
pub use knowledge_graph::SymbolResolution;
pub use knowledge_graph::{Convention, Entity, Relation};
pub use knowledge_graph::{HotObservation, WalCheckpointMode};
pub use knowledge_graph::{KnowledgeGraph, KnowledgeGraphError};
pub use otel::{init_tracer, shutdown_tracer};
pub use schema_migration::{MigrationError, SCHEMA_VERSION};
pub use types::{
    CeqpParams, Diagnostic, KnowledgeEntry, QueryPlan, ResponseEnvelope, Symbol, ToolGroup,
};
