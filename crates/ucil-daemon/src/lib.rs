//! `ucil-daemon` library root — re-exports for integration tests.
//!
//! All daemon logic lives in sub-modules (`server`, `watcher`, `plugin_manager`, etc.).
//! This file only declares modules and re-exports.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod lifecycle;
pub mod plugin_manager;
pub mod server;
pub mod session_manager;

pub use lifecycle::{Lifecycle, PidFile, PidFileError, ShutdownReason};
pub use plugin_manager::{
    HealthStatus, LifecycleSection, PluginError, PluginHealth, PluginManager, PluginManifest,
    PluginRuntime, PluginSection, PluginState, TransportSection, DEFAULT_IDLE_TIMEOUT_MINUTES,
    HEALTH_CHECK_TIMEOUT_MS,
};
// `health_check_with_timeout` is a method on `PluginManager`; it is reached via the
// re-exported `PluginManager` above — no additional item-level re-export is needed.
pub use server::{
    ceqp_input_schema, ucil_tools, McpError, McpServer, ToolDescriptor, JSONRPC_VERSION,
    MCP_PROTOCOL_VERSION, READ_TIMEOUT_MS, TOOL_COUNT, WRITE_TIMEOUT_MS,
};
pub use session_manager::{SessionId, SessionInfo, SessionManager, WorktreeInfo};
