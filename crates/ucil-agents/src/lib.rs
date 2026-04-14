//! `ucil-agents` — internal agent implementations: provider, interpreter, synthesis,
//! conflict resolution, clarification, convention, memory curator, architecture.
//!
//! This `lib.rs` only re-exports public sub-modules; all logic lives in sub-modules.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]
