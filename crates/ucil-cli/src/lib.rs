//! `ucil-cli` library root — re-exports for integration tests.
//!
//! All CLI logic lives in sub-modules (commands/init, commands/daemon, etc.).
//! This file only declares modules and re-exports.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]
