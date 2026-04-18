//! `ucil-daemon` library root — re-exports for integration tests.
//!
//! All daemon logic lives in sub-modules (`lifecycle`, `plugin_manager`,
//! `server`, `session_manager`, …).  This file only declares modules
//! and re-exports.
//!
//! The `lifecycle` module (introduced in WO-0021 for P1-W3-F01) owns
//! the daemon's PID-file guard and `SIGTERM` / `SIGHUP` driven shutdown
//! — see [`lifecycle`] for details.
//!
//! The `session_manager` + `session_ttl` pair (WO-0021 for P1-W4-F07)
//! own in-memory session state: `SessionManager` indexes sessions by
//! [`SessionId`] and tracks `call_history`, `files_in_context`,
//! `inferred_domain`, and `expires_at`; `session_ttl` houses the
//! saturating arithmetic (`compute_expires_at`, `is_expired`,
//! [`DEFAULT_TTL_SECS`]) shared between creation and purge paths.
//!
//! The `storage` module (introduced in WO-0022 for P1-W2-F06) owns the
//! two-tier `.ucil/` directory tree; see [`storage::StorageLayout`] for
//! the layout spec (master-plan §11.2 lines 1060-1088). The matching
//! crash-recovery [`Checkpoint`] (WO-0022 for P1-W3-F09) lives in the
//! `lifecycle` module and persists the daemon's last-indexed commit to
//! `.ucil/checkpoint.json` so a restart skips already-indexed prefixes.
//!
//! The `watcher` module (introduced in WO-0026 for P1-W3-F02) owns the
//! two-path file-change detector described in master-plan §18 Phase 1
//! Week 3 line 1741 and §14 lines 1024-1025: editor/filesystem events
//! arrive via `notify-debouncer-full` with a 100 ms debounce window,
//! while `PostToolUse` hook invocations bypass the debouncer and emit
//! a [`watcher::FileEvent`] immediately — see [`watcher::FileWatcher`].
//!
//! The `priority_queue` module (introduced in WO-0028 for `P1-W3-F08`)
//! owns the recency-ordered `(Instant, PathBuf)` queue that backs the
//! "recently-queried files first" invariant in master-plan §21.2 lines
//! 2196-2204 — see [`priority_queue::PriorityIndexingQueue`].
//!
//! The `startup` module (introduced in WO-0028 for `P1-W3-F08`) owns the
//! progressive-startup orchestrator described in master-plan §18 Phase 1
//! Week 3 line 1745: it spawns the MCP server and exposes a
//! [`startup::ReadyHandle`] that resolves once the server has emitted
//! its first successful response, so callers can assert the 2 s
//! startup budget end-to-end.
//!
//! The `executor` module (introduced in WO-0032 for `P1-W4-F04`) owns
//! the tree-sitter → knowledge-graph ingestion pipeline described in
//! master-plan §18 Phase 1 Week 4 line 1759 ("Wire tree-sitter extraction
//! → knowledge graph population"): [`executor::IngestPipeline::ingest_file`]
//! parses a file with `ucil_treesitter`, extracts symbols, and upserts
//! the whole batch inside one `BEGIN IMMEDIATE` transaction per file.
//!
//! WO-0037 for `P1-W5-F02` (master-plan §18 Phase 1 Week 5 lines 1762-1770,
//! "Serena integration → G1 structural fusion") extends [`executor`] with
//! the [`executor::SerenaHoverClient`] dependency-inversion seam (per
//! `DEC-0008`) and the [`executor::enrich_find_definition`] fusion
//! function that enriches a
//! [`ucil_core::knowledge_graph::SymbolResolution`] with optional
//! [`executor::HoverDoc`] context.  Hover-fetch errors are logged at
//! `warn!` and suppressed from the fused result so a Serena outage
//! never breaks the G1 response — wiring into
//! `server::McpServer::handle_find_definition` is deferred to a
//! follow-up WO + ADR because the frozen `P1-W4-F05` acceptance
//! selector asserts on the current `_meta` JSON shape.
//!
//! The `server` module (introduced in WO-0010 for `P1-W3-F07`) owns
//! the MCP JSON-RPC 2.0 skeleton.  WO-0033 for `P1-W4-F05` (master-plan
//! §3.2 row 2 + §18 Phase 1 Week 4 line 1751) promoted the
//! `find_definition` tool from the 22-tool stub catalog to a real
//! handler: [`server::McpServer::with_knowledge_graph`] builds a
//! server that, on `tools/call` for `find_definition`, resolves the
//! symbol through [`ucil_core::KnowledgeGraph::resolve_symbol`] and
//! projects inbound `calls`-kind relations onto a `_meta.callers`
//! list.  The 21 remaining tools keep the phase-1 `_meta.not_yet_
//! implemented: true` stub so phase-log invariant #9 stays satisfied.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod executor;
pub mod lifecycle;
pub mod plugin_manager;
pub mod priority_queue;
pub mod server;
pub mod session_manager;
pub mod session_ttl;
pub mod startup;
pub mod storage;
pub(crate) mod text_search;
pub mod understand_code;
pub mod watcher;

// Crate-private test utilities (process-wide PATH guard). Compiled
// only under `#[cfg(test)]` so the release binary stays clean. See
// DEC-0011 for the rationale — `watcher::tests` mutate `PATH` via
// `std::env::set_var` while `session_manager::tests` spawn `git` via
// `tokio::process::Command`, and both classes serialise through the
// `test_support::ENV_GUARD` mutex to avoid the coverage-gate race.
#[rustfmt::skip]
#[cfg(test)] mod test_support;

#[rustfmt::skip]
pub use executor::{enrich_find_definition, Caller, EnrichedFindDefinition, ExecutorError, HoverDoc, HoverFetchError, HoverSource, IngestPipeline, SerenaHoverClient, SOURCE_TOOL, TREE_SITTER_VALID_FROM};
#[rustfmt::skip]
pub use lifecycle::{Checkpoint, CheckpointError, Lifecycle, PidFile, PidFileError, ShutdownReason};
pub use plugin_manager::{
    HealthStatus, LifecycleSection, PluginError, PluginHealth, PluginManager, PluginManifest,
    PluginRuntime, PluginSection, PluginState, TransportSection, DEFAULT_IDLE_TIMEOUT_MINUTES,
    HEALTH_CHECK_TIMEOUT_MS,
};
// `health_check_with_timeout` is a method on `PluginManager`; it is reached via the
// re-exported `PluginManager` above — no additional item-level re-export is needed.
pub use priority_queue::{PriorityIndexingQueue, QueueEntry};
pub use server::{
    ceqp_input_schema, ucil_tools, McpError, McpServer, ToolDescriptor, JSONRPC_VERSION,
    MCP_PROTOCOL_VERSION, READ_TIMEOUT_MS, TOOL_COUNT, WRITE_TIMEOUT_MS,
};
pub use session_manager::{
    CallRecord, SessionId, SessionInfo, SessionManager, WorktreeInfo, DEFAULT_TTL_SECS,
};
pub use session_ttl::{compute_expires_at, is_expired};
pub use startup::{ProgressiveStartup, ReadyHandle, StartupError, STARTUP_DEADLINE};
pub use storage::{StorageError, StorageLayout};
pub use watcher::{
    EventSource, FileEvent, FileEventKind, FileWatcher, WatcherError, DEBOUNCE_WINDOW,
};
