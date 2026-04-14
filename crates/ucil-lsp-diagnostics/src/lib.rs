//! `ucil-lsp-diagnostics` — LSP diagnostics bridge: complements Serena with
//! call hierarchy, type hierarchy, and diagnostics integration.
//!
//! This `lib.rs` only re-exports public sub-modules; all logic lives in sub-modules.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]
