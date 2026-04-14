//! `ucil-daemon` library root — re-exports for integration tests.
//!
//! All daemon logic lives in sub-modules (`server`, `watcher`, `plugin_manager`, etc.).
//! This file only declares modules and re-exports.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]
