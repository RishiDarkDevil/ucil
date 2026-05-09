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
//! WO-0047 for `P2-W7-F01` extends [`executor`] with the
//! [`executor::G1Source`] dependency-inversion seam (per `DEC-0008`)
//! and [`executor::execute_g1`], the parallel orchestrator that fans
//! out a structural query to tree-sitter / Serena / ast-grep /
//! diagnostics-bridge per master-plan §5.1 and returns a
//! [`executor::G1Outcome`] with per-source [`executor::G1ToolStatus`]
//! (`Available` / `Unavailable` / `TimedOut` / `Errored`).  Production
//! wiring of real subprocess clients lands in P2-W7-F02 (fusion) and
//! P2-W7-F05 (`find_references`); F01 ships the orchestrator + the
//! trait only.
//!
//! WO-0048 for `P2-W7-F02` extends [`executor`] with
//! [`executor::fuse_g1`], the G1 fusion layer per master-plan §5.1
//! lines 430-442: it groups [`executor::G1Outcome`] per-source results
//! by source location, unions unique fields, and resolves conflicting
//! field values by source authority Serena > tree-sitter > ast-grep >
//! diagnostics.  Production wiring of real subprocess clients into the
//! fusion path is deferred to P2-W7-F05 (`find_references`); F02 ships
//! the fusion algorithm only.
//!
//! WO-0053 for `P2-W7-F09` lands the
//! [`branch_manager`] module owning the per-branch `LanceDB`
//! vector-store lifecycle described by master-plan §6.4 line 144
//! ("Branch index manager: Creates, updates, prunes, and archives
//! per-branch code indexes. Delta indexing from parent branches for
//! fast creation"), §11.2 line 1074 (per-branch `vectors/` directory)
//! and §12.2 lines 1321-1346 (the 12-field `code_chunks` table
//! schema).  [`branch_manager::BranchManager`] exposes
//! [`branch_manager::BranchManager::create_branch_table`] (with
//! optional `parent` for filesystem-level delta-clone of an
//! already-indexed branch),
//! [`branch_manager::BranchManager::archive_branch_table`] (atomic
//! rename to `<base>/branches/.archive/<sanitised>-<unix_ts_micros>/`),
//! and [`branch_manager::BranchManager::branch_vectors_dir`] +
//! [`branch_manager::BranchManager::archive_root`] /
//! [`branch_manager::BranchManager::branches_root`] for path
//! arithmetic.  Production wiring of `BranchManager` into the daemon's
//! startup / branch-detection / session paths is deferred to
//! `P2-W8-F04` (`LanceDB` background chunk indexing per master-plan §18
//! Phase 2 Week 8 line 1788); F09 ships the standalone API + the unit
//! test verifying its lifecycle semantics.
//!
//! WO-0064 for `P2-W8-F04` lands the
//! [`lancedb_indexer`] module owning the per-branch background
//! chunk-indexing pipeline that consumes
//! [`ucil_embeddings::EmbeddingChunker`] +
//! [`ucil_embeddings::CodeRankEmbed`] (the latter behind the
//! `UCIL`-internal [`lancedb_indexer::EmbeddingSource`] trait per
//! `DEC-0008` §4) and writes 12-column
//! [`branch_manager::code_chunks_schema`]-conforming
//! [`arrow_array::RecordBatch`] rows into the per-branch
//! `code_chunks` `LanceDB` table opened by
//! [`branch_manager::BranchManager::create_branch_table`]
//! (master-plan §11.2 + §12.2).  Incremental skip is driven by a
//! `<branches_root>/<sanitised>/indexer-state.json` mtime sidecar
//! persisted via [`lancedb_indexer::IndexerState::save_atomic`].
//! The companion [`lancedb_indexer::IndexerHandle`] subscribes to a
//! `tokio::sync::mpsc::Receiver<watcher::FileEvent>` and dispatches
//! each create/modify event to the indexer — consumer wiring of the
//! handle into [`watcher::FileWatcher`] is deferred to a follow-up
//! `WO`.  Frozen acceptance test
//! [`executor::test_lancedb_incremental_indexing`] (`DEC-0007`
//! module-root) exercises 6 sub-assertions via a deterministic
//! `TestEmbeddingSource` impl per `DEC-0008`.
//!
//! WO-0063 for `P2-W7-F06` lights up
//! [`server::McpServer::with_g2_sources`] and the G2-fused half of the
//! `search_code` MCP tool (master-plan §3.2 row 4 / §5.2 G2 fan-out):
//! the handler fans out via [`g2_search::G2SourceFactory::build`], runs
//! the three providers in parallel ([`g2_search::ProbeProvider`] /
//! [`g2_search::RipgrepProvider`] / [`g2_search::LancedbProvider`]),
//! and fuses via `ucil_core::fuse_g2_rrf` (WO-0056).  The legacy
//! P1-W5-F09 KG+ripgrep merge stays byte-identical per `DEC-0015` D1;
//! the fused output appears on a new additive `_meta.g2_fused` field.
//! [`g2_search::LancedbProvider`] returns empty `hits` until P2-W8-F04
//! populates the per-branch `code_chunks.lance` table per `DEC-0015`
//! D3.
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

pub mod branch_manager;
pub mod executor;
pub mod g2_search;
pub mod g3;
pub mod g4;
pub mod g5;
pub mod g7;
pub mod g8;
pub mod lancedb_indexer;
pub mod lifecycle;
pub mod plugin_manager;
pub mod priority_queue;
pub mod scip;
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
pub use branch_manager::{BranchManager, BranchManagerError, BranchTableInfo, code_chunks_schema, sanitise_branch_name, ARCHIVE_DIR_NAME};
#[rustfmt::skip]
pub use executor::{enrich_find_definition, execute_g1, fuse_g1, Caller, EnrichedFindDefinition, ExecutorError, G1Conflict, G1FusedEntry, G1FusedLocation, G1FusedOutcome, G1FusionEntry, G1Outcome, G1Query, G1Source, G1ToolKind, G1ToolOutput, G1ToolStatus, HoverDoc, HoverFetchError, HoverSource, IngestPipeline, SerenaHoverClient, G1_MASTER_DEADLINE, G1_PER_SOURCE_DEADLINE, SOURCE_TOOL, TREE_SITTER_VALID_FROM};
#[rustfmt::skip]
pub use g2_search::{G2SearchError, G2SourceFactory, G2SourceProvider, LancedbProvider, ProbeProvider, RipgrepProvider};
#[rustfmt::skip]
pub use g3::{execute_g3, merge_g3_by_entity, G3FactObservation, G3MergeOutcome, G3MergedFact, G3Outcome, G3Query, G3Source, G3SourceOutput, G3SourceStatus, G3_MASTER_DEADLINE, G3_PER_SOURCE_DEADLINE};
#[rustfmt::skip]
pub use g4::{execute_g4, merge_g4_dependency_union, G4BlastRadiusEntry, G4DependencyEdge, G4EdgeKind, G4EdgeOrigin, G4Outcome, G4Query, G4Source, G4SourceOutput, G4SourceStatus, G4UnifiedEdge, G4UnionOutcome, G4_MASTER_DEADLINE, G4_PER_SOURCE_DEADLINE};
#[rustfmt::skip]
pub use g5::{assemble_g5_context, execute_g5, G5AssembledContext, G5ContextChunk, G5Outcome, G5Query, G5Source, G5SourceKind, G5SourceOutput, G5SourceStatus, G5_MASTER_DEADLINE, G5_PER_SOURCE_DEADLINE};
#[rustfmt::skip]
pub use lancedb_indexer::{ChunkIndexerError, CodeRankEmbeddingSource, EmbeddingSource, EmbeddingSourceError, IndexerHandle, IndexerState, IndexerStats, LancedbChunkIndexer};
#[rustfmt::skip]
pub use lifecycle::{Checkpoint, CheckpointError, Lifecycle, PidFile, PidFileError, ShutdownReason};
pub use plugin_manager::{
    ActivationSection, CapabilitiesSection, HealthStatus, LifecycleSection, PluginError,
    PluginHealth, PluginManager, PluginManifest, PluginRuntime, PluginSection, PluginState,
    ResourcesSection, TransportSection, CIRCUIT_BREAKER_BASE_BACKOFF_MS,
    DEFAULT_IDLE_TIMEOUT_MINUTES, HEALTH_CHECK_TIMEOUT_MS, MAX_RESTARTS,
};
// `health_check_with_timeout` is a method on `PluginManager`; it is reached via the
// re-exported `PluginManager` above — no additional item-level re-export is needed.
pub use priority_queue::{PriorityIndexingQueue, QueueEntry};
pub use scip::{
    index_repo, load_index_to_sqlite, query_symbol, ScipError, ScipG1Source, ScipReference,
    SCIP_INDEX_DEADLINE_SECS, SCIP_SCHEMA,
};
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
    auto_select_backend, count_files_capped, detect_watchman, EventSource, FileEvent,
    FileEventKind, FileWatcher, WatcherBackend, WatcherError, WatchmanCapability, DEBOUNCE_WINDOW,
    POLL_WATCHER_INTERVAL, WATCHMAN_AUTO_SELECT_THRESHOLD,
};
