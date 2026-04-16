//! `ucil-daemon` library root — re-exports for integration tests.
//!
//! All daemon logic lives in sub-modules (`server`, `watcher`, `plugin_manager`, etc.).
//! This file only declares modules and re-exports.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod plugin_manager;
pub mod session_manager;

pub use plugin_manager::{
    HealthStatus, PluginError, PluginHealth, PluginManager, PluginManifest, PluginSection,
    TransportSection, HEALTH_CHECK_TIMEOUT_MS,
};
pub use session_manager::{SessionId, SessionInfo, SessionManager, WorktreeInfo};
