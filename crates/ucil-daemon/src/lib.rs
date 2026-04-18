//! `ucil-daemon` library root — re-exports for integration tests.
//!
//! All daemon logic lives in sub-modules (`lifecycle`, `plugin_manager`,
//! `server`, `session_manager`, …).  This file only declares modules
//! and re-exports.
//!
//! The `lifecycle` module (introduced in WO-0021 for P1-W3-F01) owns
//! the daemon's PID-file guard and `SIGTERM` / `SIGHUP` driven shutdown
//! — see [`lifecycle`] for details.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod lifecycle;
pub mod plugin_manager;
pub mod server;
pub mod session_manager;
pub mod session_ttl;

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
pub use session_manager::{
    CallRecord, SessionId, SessionInfo, SessionManager, WorktreeInfo, DEFAULT_TTL_SECS,
};
pub use session_ttl::{compute_expires_at, is_expired};
