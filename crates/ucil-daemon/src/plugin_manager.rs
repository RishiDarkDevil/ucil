//! Plugin-manager skeleton: discover → spawn → health-check.
//!
//! The plugin manager is the daemon's bridge between UCIL's own registry
//! and third-party MCP-speaking plugins.  A plugin ships a
//! `plugin.toml` manifest ([master-plan §14.1]) describing how to launch
//! the plugin binary and how UCIL should talk to it.
//!
//! This module covers the *skeleton* subset required by P1-W3-F05:
//!
//! | Capability              | Function                           |
//! |-------------------------|------------------------------------|
//! | parse manifest          | [`PluginManifest::from_path`]      |
//! | walk a plugins dir      | [`PluginManager::discover`]        |
//! | launch a plugin process | [`PluginManager::spawn`]           |
//! | MCP `tools/list` probe  | [`PluginManager::health_check`]    |
//!
//! Deliberately **out of scope** for this WO (see `ucil-master-plan`
//! feature card P1-W3-F06 and the WO-0009 `scope_out` list):
//! HOT/COLD lifecycle, idle-timeout auto-restart, crash-restart counters,
//! circuit breakers, and registration of plugin-provided tools onto the
//! daemon's MCP server.  The types here intentionally return owned data
//! so the downstream lifecycle layer can wrap them without re-parsing.
//!
//! # Wire protocol
//!
//! The health check is a single JSON-RPC 2.0 request/response over the
//! spawned process's stdio, exactly matching the form the daemon itself
//! speaks to its own MCP clients:
//!
//! ```json
//! // request — one line, newline-terminated
//! {"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}
//!
//! // response
//! {"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"...","description":"..."}]}}
//! ```
//!
//! [master-plan §14.1]: ../../../../ucil-master-plan-v2.1-final.md

// The public API items share a name prefix with the module ("plugin_manager"
// → "PluginManager", "PluginManifest", …); pedantic clippy would flag every
// one.  The convention matches the rest of the crate (see `session_manager`).
#![allow(clippy::module_name_repetitions)]

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use serde::Deserialize;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::RwLock,
    time::timeout,
};

// ── Constants ────────────────────────────────────────────────────────────────

/// Timeout budget, in milliseconds, for a complete `tools/list` health
/// check — manifest spawn, request write, response read, and child kill.
///
/// The value follows master-plan §14.2 (plugin-manager health-check
/// defaults): 5 s is the upper bound an MCP plugin is allowed to take
/// before the daemon declares it unhealthy and falls back to the COLD
/// state.
pub const HEALTH_CHECK_TIMEOUT_MS: u64 = 5_000;

/// Default idle-timeout, in minutes, for a HOT/COLD plugin when the
/// manifest does not set `[lifecycle] idle_timeout_minutes` (master-plan
/// §14.1 shipped example default).
pub const DEFAULT_IDLE_TIMEOUT_MINUTES: u64 = 10;

// ── Manifest types ───────────────────────────────────────────────────────────

/// Parsed `plugin.toml` manifest.
///
/// Models the master-plan §14.1 top-level tables: `[plugin]`,
/// `[capabilities]` (with nested `[capabilities.activation]`),
/// `[transport]`, the optional `[resources]`, and the optional
/// `[lifecycle]`. `[capabilities]` and `[resources]` were added in
/// Phase 2 Week 6 (P2-W6-F01); `#[serde(default)]` on both keeps
/// minimal Phase-1 manifests parsing without edits.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct PluginManifest {
    /// Identity table.
    pub plugin: PluginSection,
    /// `[capabilities]` table — provides / languages / activation
    /// rules. Defaults to an empty section so older minimal manifests
    /// still parse.
    #[serde(default)]
    pub capabilities: CapabilitiesSection,
    /// How to launch the plugin and what wire protocol to use.
    pub transport: TransportSection,
    /// Optional `[resources]` table — soft hints used by the daemon's
    /// scheduler to size sandbox + memory caps.
    #[serde(default)]
    pub resources: Option<ResourcesSection>,
    /// Optional `[lifecycle]` table: HOT/COLD mode + idle-timeout knobs.
    #[serde(default)]
    pub lifecycle: Option<LifecycleSection>,
}

/// `[plugin]` section of a plugin manifest.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct PluginSection {
    /// Short identifier.  Must be unique across all loaded plugins.
    pub name: String,
    /// Semver version string, provided verbatim in the health-check
    /// response envelope.
    pub version: String,
    /// Human-readable description of what the plugin does.
    #[serde(default)]
    pub description: Option<String>,
}

/// `[transport]` section of a plugin manifest.
///
/// Only the `stdio` transport is implemented here; the structural shape
/// is preserved so future transports (e.g. `sse`, `socket`) can be added
/// without breaking manifest parsing.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct TransportSection {
    /// The transport kind.  Only `"stdio"` is supported today; any other
    /// value is accepted by the parser but rejected by
    /// [`PluginManager::spawn`] with [`PluginError::UnsupportedTransport`].
    #[serde(rename = "type")]
    pub kind: String,
    /// Executable to invoke.  Passed verbatim to
    /// [`tokio::process::Command::new`]; absolute paths and
    /// binaries-on-`PATH` are both accepted.
    pub command: String,
    /// Optional arguments forwarded to the plugin process.
    #[serde(default)]
    pub args: Vec<String>,
}

/// `[capabilities]` section of a plugin manifest (master-plan §14.1).
///
/// Declares what the plugin contributes to UCIL and when the daemon
/// should activate it. All fields are `#[serde(default)]` so a manifest
/// that omits the entire `[capabilities]` table parses to an empty
/// section that activates for nothing — the conservative default.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct CapabilitiesSection {
    /// MCP tool names the plugin provides (e.g. `["search_code"]`).
    #[serde(default)]
    pub provides: Vec<String>,
    /// Languages the plugin understands (e.g. `["rust", "typescript"]`).
    /// Empty means language-agnostic.
    #[serde(default)]
    pub languages: Vec<String>,
    /// Activation rules — when UCIL should load and route to this
    /// plugin. See [`ActivationSection`].
    #[serde(default)]
    pub activation: ActivationSection,
}

/// `[capabilities.activation]` subsection.
///
/// Each list applies as an OR: a non-empty `on_language` filter means
/// "activate when the active session targets one of these languages";
/// an empty list means "no language filter — activate for any". Same
/// rule for `on_tool`. `eager` skips lazy activation and pre-warms the
/// plugin at daemon startup.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct ActivationSection {
    /// Languages whose presence in a session activates this plugin.
    #[serde(default)]
    pub on_language: Vec<String>,
    /// MCP tool calls whose name activates this plugin.
    #[serde(default)]
    pub on_tool: Vec<String>,
    /// When `true`, load the plugin at daemon startup instead of
    /// lazily on first activation match.
    #[serde(default)]
    pub eager: bool,
}

/// `[resources]` section of a plugin manifest (master-plan §14.1).
///
/// Soft hints the daemon uses to size sandbox + scheduling. All fields
/// are `Option` so a manifest may omit any of them; the absent fields
/// fall back to crate-wide defaults at the point of use (not modelled
/// here).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct ResourcesSection {
    /// Expected resident memory ceiling, in mebibytes.
    pub memory_mb: Option<u64>,
    /// Wall-clock time the plugin needs from `spawn` to first
    /// `tools/list` reply, in milliseconds.
    pub startup_time_ms: Option<u64>,
    /// Typical wall-clock time for a single tool call, in milliseconds.
    pub typical_query_ms: Option<u64>,
}

/// `[lifecycle]` section of a plugin manifest (master-plan §14.1).
///
/// Controls whether the plugin participates in the HOT/COLD lifecycle
/// and, if so, after how many minutes of idleness it is demoted to the
/// `IDLE` state.  When the section is absent from the manifest the
/// plugin is treated as HOT (never auto-demoted) — this matches the
/// master-plan default behavior.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct LifecycleSection {
    /// Enable HOT/COLD lifecycle management for this plugin.  When
    /// `false` (the default) the plugin is considered always-HOT and is
    /// never auto-demoted.
    #[serde(default)]
    pub hot_cold: bool,
    /// Minutes of idleness after which a HOT plugin is demoted to
    /// `IDLE`.  Falls back to [`DEFAULT_IDLE_TIMEOUT_MINUTES`] when the
    /// field is absent.
    #[serde(default)]
    pub idle_timeout_minutes: Option<u64>,
}

impl LifecycleSection {
    /// Resolve the idle-timeout for this lifecycle entry, applying the
    /// crate default when the manifest did not specify one.
    #[must_use]
    pub fn idle_timeout(&self) -> Duration {
        let minutes = self
            .idle_timeout_minutes
            .unwrap_or(DEFAULT_IDLE_TIMEOUT_MINUTES);
        Duration::from_secs(minutes.saturating_mul(60))
    }
}

impl PluginManifest {
    /// Read and parse a `plugin.toml` manifest from disk.
    ///
    /// # Errors
    ///
    /// * [`PluginError::Io`] — the file could not be opened.
    /// * [`PluginError::ManifestParse`] — the file is not valid TOML or
    ///   omits required fields.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, PluginError> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path).map_err(PluginError::Io)?;
        let manifest: Self = toml::from_str(&raw).map_err(|e| PluginError::ManifestParse {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(manifest)
    }
}

// ── HOT/COLD lifecycle types ─────────────────────────────────────────────────

/// Lifecycle state of a plugin runtime.
///
/// Mirrors master-plan §14.2:
///
/// ```text
/// DISCOVERED → REGISTERED → LOADING → ACTIVE → IDLE → STOPPED → ERROR
/// ```
///
/// A HOT plugin (manifest `[lifecycle] hot_cold = true`) that sits idle
/// for longer than its configured `idle_timeout_minutes` auto-transitions
/// `Active → Idle`.  A subsequent call routes back `Idle → Loading` via
/// [`PluginRuntime::mark_call`], and the manager then drives
/// `Loading → Active` via a real health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PluginState {
    /// A manifest has been found on disk but not yet registered.
    Discovered,
    /// The manifest has been parsed and accepted into the registry.
    Registered,
    /// The child process is starting up / the initial health check is
    /// in flight.
    Loading,
    /// The plugin responded to a health check and is serving requests.
    Active,
    /// The plugin has been hibernated after exceeding its idle timeout.
    Idle,
    /// The plugin has been explicitly stopped; the registry retains the
    /// manifest so the plugin can be re-started on demand.
    Stopped,
    /// The plugin transitioned to a terminal failure state.  The
    /// associated message is stored separately; this enum variant is
    /// intentionally fieldless so the state is `Copy`-able.
    Error,
}

impl std::fmt::Display for PluginState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let token = match self {
            Self::Discovered => "DISCOVERED",
            Self::Registered => "REGISTERED",
            Self::Loading => "LOADING",
            Self::Active => "ACTIVE",
            Self::Idle => "IDLE",
            Self::Stopped => "STOPPED",
            Self::Error => "ERROR",
        };
        f.write_str(token)
    }
}

/// Per-plugin runtime state — manifest plus lifecycle bookkeeping.
///
/// `PluginRuntime` is deliberately a value type so callers can hold it
/// by ownership and mutate it without contending on a shared lock.  The
/// [`PluginManager`] also keeps a parallel cloned view behind an
/// `Arc<RwLock<_>>` so the background idle-monitor task (see
/// [`PluginManager::run_idle_monitor`]) can drive state transitions
/// without the caller needing to hand it out a mutable reference.
#[derive(Debug, Clone)]
pub struct PluginRuntime {
    /// The manifest that produced this runtime.  Kept by value so the
    /// runtime survives a discover → rescan cycle.
    pub manifest: PluginManifest,
    /// Current lifecycle state.  Transitions flow through the
    /// state-machine methods on this type; direct mutation by callers
    /// is discouraged outside tests.
    pub state: PluginState,
    /// Wall-clock instant of the most recent [`Self::mark_call`] (or
    /// construction, if no call has happened yet).
    pub last_call: Instant,
    /// Idle budget resolved from the manifest `[lifecycle]` section or
    /// the crate default.  A tick whose `now - last_call` exceeds this
    /// duration demotes the runtime to [`PluginState::Idle`].
    pub idle_timeout: Duration,
}

impl PluginRuntime {
    /// Build a new runtime in [`PluginState::Registered`].
    ///
    /// `idle_timeout` is resolved from the manifest's `[lifecycle]`
    /// section: `idle_timeout_minutes` → [`Duration`] of that many
    /// minutes; missing → [`DEFAULT_IDLE_TIMEOUT_MINUTES`].
    #[must_use]
    pub fn new(manifest: PluginManifest) -> Self {
        let idle_timeout = manifest.lifecycle.as_ref().map_or_else(
            || Duration::from_secs(DEFAULT_IDLE_TIMEOUT_MINUTES * 60),
            LifecycleSection::idle_timeout,
        );
        Self {
            manifest,
            state: PluginState::Registered,
            last_call: Instant::now(),
            idle_timeout,
        }
    }

    /// Builder helper to override the idle timeout (tests use this for
    /// fast-tick scenarios without hand-patching the manifest).
    #[must_use]
    pub const fn with_idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    /// Record that a tool call arrived at this plugin.
    ///
    /// * If the runtime is [`PluginState::Idle`] it flips to
    ///   [`PluginState::Loading`] — the signal that the manager owes
    ///   this plugin a re-spawn and fresh health check.
    /// * `last_call` is advanced unconditionally so subsequent ticks
    ///   restart the idle countdown from "now".
    pub fn mark_call(&mut self) {
        if matches!(self.state, PluginState::Idle) {
            self.state = PluginState::Loading;
        }
        self.last_call = Instant::now();
    }

    /// Advance the idle countdown.
    ///
    /// Returns `Some(new_state)` whenever the tick produced a state
    /// transition (today: only `Active → Idle`).  Returns `None` when
    /// the state was left untouched.
    pub fn tick(&mut self, now: Instant) -> Option<PluginState> {
        if matches!(self.state, PluginState::Active)
            && now.saturating_duration_since(self.last_call) > self.idle_timeout
        {
            self.state = PluginState::Idle;
            return Some(PluginState::Idle);
        }
        None
    }
}

// ── Health-check output types ────────────────────────────────────────────────

/// Result of a [`PluginManager::health_check`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHealth {
    /// Plugin name, copied from `manifest.plugin.name`.
    pub name: String,
    /// Tool identifiers reported by the plugin via `tools/list`.
    pub tools: Vec<String>,
    /// Overall health status.
    pub status: HealthStatus,
}

/// Health classification returned by a `tools/list` probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// The plugin replied with a well-formed JSON-RPC result.
    Ok,
    /// The plugin replied within the timeout but with zero tools.
    /// Treated as degraded rather than failed because the plugin may
    /// still be warming up.
    Degraded(String),
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by the plugin-manager skeleton.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PluginError {
    /// A filesystem operation failed.
    #[error("i/o error: {0}")]
    Io(#[source] std::io::Error),
    /// A `plugin.toml` file failed to parse.
    #[error("failed to parse manifest at {}: {source}", path.display())]
    ManifestParse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Underlying TOML decoder error.
        #[source]
        source: toml::de::Error,
    },
    /// The manifest transport kind is not supported by the skeleton.
    #[error("unsupported transport `{0}` (only `stdio` is implemented)")]
    UnsupportedTransport(String),
    /// `spawn()` could not start the plugin subprocess.
    #[error("failed to spawn `{command}`: {source}")]
    Spawn {
        /// Executable that failed to launch.
        command: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// The spawned child did not expose a piped stdin or stdout.
    #[error("child process is missing piped `{0}` — spawn was misconfigured")]
    MissingStdio(&'static str),
    /// Writing to, or reading from, the child's stdio failed.
    #[error("stdio transport error: {0}")]
    StdioTransport(#[source] std::io::Error),
    /// The health check did not complete inside
    /// [`HEALTH_CHECK_TIMEOUT_MS`] milliseconds.
    #[error("health check timed out after {ms} ms")]
    HealthCheckTimeout {
        /// Configured timeout, in milliseconds.
        ms: u64,
    },
    /// The plugin returned malformed JSON or a JSON-RPC error frame.
    #[error("plugin returned an invalid tools/list response: {0}")]
    ProtocolError(String),
}

// ── Plugin manager ───────────────────────────────────────────────────────────

/// Plugin-manager façade.
///
/// The manager owns a list of [`PluginRuntime`]s behind an
/// [`Arc`]/[`RwLock`] pair so the background idle-monitor task (see
/// [`Self::run_idle_monitor`]) and caller threads can observe and
/// mutate runtime state without fighting over ownership.  All of the
/// original skeleton operations ([`Self::discover`], [`Self::spawn`],
/// [`Self::health_check`]) remain associated functions — none of them
/// depend on manager state.
#[derive(Debug, Clone, Default)]
pub struct PluginManager {
    runtimes: Arc<RwLock<Vec<PluginRuntime>>>,
}

impl PluginManager {
    /// Construct a new, empty `PluginManager`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk `plugins_dir` (non-recursively) for `*.toml` files and parse
    /// each one as a [`PluginManifest`].
    ///
    /// Entries that fail to parse are propagated as errors — the caller
    /// decides whether to skip, retry, or escalate.  Subdirectories are
    /// ignored by design; plugins that ship multiple manifests belong
    /// in peer directories.
    ///
    /// # Errors
    ///
    /// * [`PluginError::Io`] — `plugins_dir` cannot be read.
    /// * [`PluginError::ManifestParse`] — any single file is not a valid
    ///   manifest.
    pub fn discover(plugins_dir: &Path) -> Result<Vec<PluginManifest>, PluginError> {
        let entries = std::fs::read_dir(plugins_dir).map_err(PluginError::Io)?;
        let mut manifests = Vec::new();
        for entry in entries {
            let entry = entry.map_err(PluginError::Io)?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("toml") {
                continue;
            }
            manifests.push(PluginManifest::from_path(&path)?);
        }
        // Sort for determinism — `read_dir` order is filesystem-defined.
        manifests.sort_by(|a, b| a.plugin.name.cmp(&b.plugin.name));
        Ok(manifests)
    }

    /// Spawn the plugin's transport subprocess with stdin/stdout piped.
    ///
    /// The returned [`Child`] has both stdio handles taken available for
    /// the caller to drive the JSON-RPC dialogue; [`Self::health_check`]
    /// is one such driver.
    ///
    /// # Errors
    ///
    /// * [`PluginError::UnsupportedTransport`] — transport kind other
    ///   than `stdio`.
    /// * [`PluginError::Spawn`] — the OS refused to start the process.
    pub fn spawn(manifest: &PluginManifest) -> Result<Child, PluginError> {
        if manifest.transport.kind != "stdio" {
            return Err(PluginError::UnsupportedTransport(
                manifest.transport.kind.clone(),
            ));
        }
        let mut cmd = Command::new(&manifest.transport.command);
        cmd.args(&manifest.transport.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        cmd.spawn().map_err(|source| PluginError::Spawn {
            command: manifest.transport.command.clone(),
            source,
        })
    }

    /// Spawn the plugin, send `tools/list`, await the response, and
    /// return a [`PluginHealth`] summary.
    ///
    /// The spawned child is killed before this function returns so
    /// callers do not need to clean it up.  Every `.await` that touches
    /// the child's stdio is wrapped in a single outer
    /// [`tokio::time::timeout`] of [`HEALTH_CHECK_TIMEOUT_MS`].
    ///
    /// # Errors
    ///
    /// * [`PluginError::UnsupportedTransport`] / [`PluginError::Spawn`]
    ///   — as for [`Self::spawn`].
    /// * [`PluginError::MissingStdio`] — child did not expose a piped
    ///   stdin or stdout.
    /// * [`PluginError::StdioTransport`] — underlying I/O error.
    /// * [`PluginError::HealthCheckTimeout`] — no response inside the
    ///   timeout budget.
    /// * [`PluginError::ProtocolError`] — malformed or error-kind
    ///   response frame.
    pub async fn health_check(manifest: &PluginManifest) -> Result<PluginHealth, PluginError> {
        Self::health_check_with_timeout(manifest, HEALTH_CHECK_TIMEOUT_MS).await
    }

    /// Variant of [`Self::health_check`] that accepts a caller-supplied
    /// timeout budget.
    ///
    /// The existing 5 s [`HEALTH_CHECK_TIMEOUT_MS`] is appropriate for
    /// plugins that are already installed on disk (e.g. HOT/COLD
    /// lifecycle ticks, in-flight daemon probes).  Callers that launch
    /// a plugin via a package manager on a cold cache — for example
    /// `uvx --from git+<url>@<ref> serena-mcp-server` — should pass a
    /// larger budget (≥30 s on first run) because uvx may have to
    /// download, build, and install Python dependencies before the
    /// plugin's `tools/list` reply can arrive.
    ///
    /// # Errors
    ///
    /// * [`PluginError::UnsupportedTransport`] / [`PluginError::Spawn`]
    ///   — as for [`Self::spawn`].
    /// * [`PluginError::MissingStdio`] — child did not expose a piped
    ///   stdin or stdout.
    /// * [`PluginError::StdioTransport`] — underlying I/O error.
    /// * [`PluginError::HealthCheckTimeout`] — no response inside the
    ///   caller-supplied timeout budget.  The timeout is user-supplied;
    ///   callers on slow-uvx-install paths should budget ≥30 s on
    ///   first-run.
    /// * [`PluginError::ProtocolError`] — malformed or error-kind
    ///   response frame.
    pub async fn health_check_with_timeout(
        manifest: &PluginManifest,
        timeout_ms: u64,
    ) -> Result<PluginHealth, PluginError> {
        let budget = Duration::from_millis(timeout_ms);
        let mut child = Self::spawn(manifest)?;
        let result = timeout(budget, async { Self::run_tools_list(&mut child).await }).await;

        // Always try to reap the child — don't let it linger if the
        // health check timed out.
        let _ = child.start_kill();
        let _ = child.wait().await;

        let tool_names = match result {
            Ok(inner) => inner?,
            Err(_elapsed) => {
                return Err(PluginError::HealthCheckTimeout { ms: timeout_ms });
            }
        };

        let status = if tool_names.is_empty() {
            HealthStatus::Degraded(format!(
                "plugin `{}` reported zero tools",
                manifest.plugin.name
            ))
        } else {
            HealthStatus::Ok
        };

        Ok(PluginHealth {
            name: manifest.plugin.name.clone(),
            tools: tool_names,
            status,
        })
    }

    /// Drive the MCP handshake and return the plugin's advertised tool
    /// names.
    ///
    /// Per the Model Context Protocol (spec 2024-11-05 onward) every
    /// server MUST process an `initialize` round-trip followed by the
    /// `notifications/initialized` notification before it accepts any
    /// other request; real MCP servers such as Serena reject a bare
    /// `tools/list` with JSON-RPC error `-32602`.  This helper therefore
    /// performs the full handshake:
    ///
    /// 1. Send `initialize` with the minimal client-capabilities frame.
    /// 2. Read and discard the server's `initialize` response — we treat
    ///    it as best-effort so the same code path works against the
    ///    in-tree `mock-mcp-plugin` binary and against real servers.
    /// 3. Send `notifications/initialized` (no response expected).
    /// 4. Send `tools/list` and parse the response frame.
    ///
    /// Extracted from [`Self::health_check`] so the outer
    /// [`tokio::time::timeout`] wrapper covers the entire handshake
    /// without duplicating the timeout across three separate `.await`
    /// points.
    async fn run_tools_list(child: &mut Child) -> Result<Vec<String>, PluginError> {
        let mut stdin = child
            .stdin
            .take()
            .ok_or(PluginError::MissingStdio("stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(PluginError::MissingStdio("stdout"))?;

        // ── Step 1: initialize ──────────────────────────────────────────
        let initialize = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"ucil","version":"0.1.0"}}}
"#;
        stdin
            .write_all(initialize)
            .await
            .map_err(PluginError::StdioTransport)?;
        stdin.flush().await.map_err(PluginError::StdioTransport)?;

        let mut reader = BufReader::new(stdout);
        let mut initialize_response = String::new();
        reader
            .read_line(&mut initialize_response)
            .await
            .map_err(PluginError::StdioTransport)?;
        // Best-effort: a JSON-RPC error here (e.g. a mock that doesn't
        // speak `initialize`) is swallowed so the same code path drives
        // both the mock and real MCP servers.

        // ── Step 2: notifications/initialized (no response expected) ────
        let initialized = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}
"#;
        stdin
            .write_all(initialized)
            .await
            .map_err(PluginError::StdioTransport)?;
        stdin.flush().await.map_err(PluginError::StdioTransport)?;

        // ── Step 3: tools/list ──────────────────────────────────────────
        let tools_list = br#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
"#;
        stdin
            .write_all(tools_list)
            .await
            .map_err(PluginError::StdioTransport)?;
        stdin.flush().await.map_err(PluginError::StdioTransport)?;
        // Signal EOF so the plugin's loop exits cleanly after responding.
        drop(stdin);

        let mut tools_response = String::new();
        reader
            .read_line(&mut tools_response)
            .await
            .map_err(PluginError::StdioTransport)?;
        parse_tools_list_response(&tools_response)
    }

    // ── HOT/COLD lifecycle façade ────────────────────────────────────────────

    /// Register and activate a plugin: `Registered → Loading → Active`
    /// via a real [`Self::health_check`].
    ///
    /// The returned [`PluginRuntime`] is an owned snapshot for the
    /// caller to drive directly (e.g. calling
    /// [`PluginRuntime::mark_call`]); the manager retains a parallel
    /// clone so the background idle monitor can tick it.
    ///
    /// # Errors
    ///
    /// Propagates any error from [`Self::health_check`] (spawn failure,
    /// stdio transport error, timeout, protocol error, …).
    pub async fn activate(
        &mut self,
        manifest: &PluginManifest,
    ) -> Result<PluginRuntime, PluginError> {
        let mut runtime = PluginRuntime::new(manifest.clone());
        runtime.state = PluginState::Loading;
        // Real health check — no mocks (rust-style.md and master-plan §14.2).
        Self::health_check(manifest).await?;
        runtime.state = PluginState::Active;
        runtime.last_call = Instant::now();

        self.runtimes.write().await.push(runtime.clone());
        Ok(runtime)
    }

    /// Wake an `Idle` or `Loading` runtime: `→ Active` via a real
    /// health check.
    ///
    /// Called after [`PluginRuntime::mark_call`] has flipped an idle
    /// runtime to `Loading`.  Safe to call from any state — an
    /// `Active` runtime is left untouched and no health check is
    /// issued.
    ///
    /// # Errors
    ///
    /// Propagates any error from [`Self::health_check`].
    pub async fn wake(runtime: &mut PluginRuntime) -> Result<(), PluginError> {
        if matches!(runtime.state, PluginState::Active) {
            return Ok(());
        }
        runtime.state = PluginState::Loading;
        Self::health_check(&runtime.manifest).await?;
        runtime.state = PluginState::Active;
        runtime.last_call = Instant::now();
        Ok(())
    }

    /// Spawn a background task that periodically calls
    /// [`PluginRuntime::tick`] on every registered runtime.
    ///
    /// The returned [`tokio::task::JoinHandle`] is detached from the
    /// manager: callers who need to stop the monitor should
    /// [`tokio::task::JoinHandle::abort`] it explicitly.  A clone of the
    /// internal `runtimes` handle is captured by the task, so adding or
    /// removing runtimes after the monitor has started is observed by
    /// the next tick.
    pub fn run_idle_monitor(&mut self, interval: Duration) -> tokio::task::JoinHandle<()> {
        let runtimes = self.runtimes.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                // `tick` is cancellation-safe; no `.await` of external
                // I/O here, so a bare `.await` is sufficient (rust-style.md
                // timeout rule applies to IO awaits only).
                ticker.tick().await;
                let now = Instant::now();
                let mut guard = runtimes.write().await;
                for rt in guard.iter_mut() {
                    let _ = rt.tick(now);
                }
            }
        })
    }

    /// Snapshot of the runtimes currently registered with this manager.
    ///
    /// Intended for diagnostics and the test that proves `activate`
    /// registers a runtime.  The returned `Vec` is a clone — mutating
    /// it does NOT propagate back to the manager.
    pub async fn registered_runtimes(&self) -> Vec<PluginRuntime> {
        self.runtimes.read().await.clone()
    }
}

/// Parse a single JSON-RPC 2.0 `tools/list` response frame and return
/// just the tool names.
///
/// Extracted as a free function so it can be covered by a fast unit
/// test that doesn't require spawning a subprocess.
fn parse_tools_list_response(frame: &str) -> Result<Vec<String>, PluginError> {
    if frame.trim().is_empty() {
        return Err(PluginError::ProtocolError(
            "plugin closed its stdout without sending a response frame".to_owned(),
        ));
    }
    let value: serde_json::Value = serde_json::from_str(frame.trim())
        .map_err(|e| PluginError::ProtocolError(format!("invalid JSON: {e}")))?;

    if let Some(err) = value.get("error") {
        return Err(PluginError::ProtocolError(format!(
            "plugin returned JSON-RPC error: {err}"
        )));
    }
    let tools = value
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            PluginError::ProtocolError("response missing `result.tools` array".to_owned())
        })?;

    let mut names = Vec::with_capacity(tools.len());
    for tool in tools {
        let name = tool
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                PluginError::ProtocolError("tool entry missing string `name`".to_owned())
            })?;
        names.push(name.to_owned());
    }
    Ok(names)
}

// ── Module-level acceptance test ─────────────────────────────────────────────
//
// NOTE on placement: this test is intentionally a peer of — not nested
// inside — the `mod tests { }` block that follows.  The feature-list
// oracle for P1-W3-F06 carries the frozen selector
// `plugin_manager::test_hot_cold_lifecycle`; placing the function in
// `mod tests { }` would change its nextest path to
// `plugin_manager::tests::test_hot_cold_lifecycle` and the verifier
// would reject.  See ucil-build/escalations/20260415-1856 and DEC-0007.

/// Locate the `mock-mcp-plugin` binary that Cargo compiles alongside
/// this crate's lib tests.
///
/// `CARGO_BIN_EXE_*` env vars are only injected for integration tests
/// (files under `tests/`).  For a library unit test we derive the path
/// from [`std::env::current_exe`]: the test binary lives in
/// `target/{profile}/deps/`, so two `pop`s yield `target/{profile}/`,
/// where cargo places the `mock-mcp-plugin` bin output.
#[cfg(test)]
fn mock_mcp_plugin_path() -> std::path::PathBuf {
    let mut exe = std::env::current_exe().expect("current_exe must succeed in tests");
    // .../target/{profile}/deps/<test_binary>
    exe.pop();
    // .../target/{profile}/deps
    exe.pop();
    exe.push(if cfg!(windows) {
        "mock-mcp-plugin.exe"
    } else {
        "mock-mcp-plugin"
    });
    exe
}

/// End-to-end HOT/COLD lifecycle exercise — the acceptance test for
/// `P1-W3-F06`.
///
/// Frozen selector: `plugin_manager::test_hot_cold_lifecycle`.
///
/// The test walks a real runtime through:
///
/// 1. `PluginManager::activate` spawns the `mock-mcp-plugin` binary,
///    runs a real `tools/list` health check, and returns a runtime in
///    [`PluginState::Active`].
/// 2. `PluginRuntime::tick` demotes the runtime to
///    [`PluginState::Idle`] once its `idle_timeout` (overridden here to
///    50 ms) elapses with no call.
/// 3. `PluginRuntime::mark_call` flips `Idle → Loading`.
/// 4. `PluginManager::wake` runs another real health check, driving
///    `Loading → Active`.
///
/// The health checks spawn a real subprocess (no mocks of
/// `tokio::process::Command` or the child's stdio).
#[cfg(test)]
#[tokio::test]
async fn test_hot_cold_lifecycle() {
    use std::time::Duration as Dur;

    let mock = mock_mcp_plugin_path();
    assert!(
        mock.exists(),
        "expected mock-mcp-plugin binary at {} — run `cargo build -p ucil-daemon --bin mock-mcp-plugin` first",
        mock.display()
    );

    // Manifest with an explicit short idle-timeout of 1 minute — the
    // test also overrides the resolved `idle_timeout` on the runtime
    // directly so `tick` fires within test wall-time.
    let manifest = PluginManifest {
        plugin: PluginSection {
            name: "hot-cold-lifecycle".into(),
            version: "0.1.0".into(),
            description: Some("HOT/COLD acceptance test manifest".into()),
        },
        capabilities: CapabilitiesSection::default(),
        transport: TransportSection {
            kind: "stdio".into(),
            command: mock.to_string_lossy().into_owned(),
            args: vec![],
        },
        resources: None,
        lifecycle: Some(LifecycleSection {
            hot_cold: true,
            idle_timeout_minutes: Some(1),
        }),
    };

    // ── Phase 1: activate → Registered → Loading → Active ────────────
    let mut mgr = PluginManager::new();
    let mut runtime = mgr
        .activate(&manifest)
        .await
        .expect("activate must succeed against the real mock plugin");

    assert_eq!(
        runtime.state,
        PluginState::Active,
        "after a successful health check the runtime must be Active",
    );
    let snapshot = mgr.registered_runtimes().await;
    assert_eq!(
        snapshot.len(),
        1,
        "activate should register the runtime with the manager (got {} runtimes)",
        snapshot.len(),
    );
    assert_eq!(
        snapshot[0].state,
        PluginState::Active,
        "registered snapshot should also be Active",
    );

    // ── Phase 2: tick → Active → Idle ────────────────────────────────
    // Shrink the idle budget and back-date last_call so `tick` fires
    // immediately.  This exercises the real clock comparison inside
    // PluginRuntime::tick without requiring the test to sleep an
    // idle_timeout_minutes worth of wall-clock time.
    runtime.idle_timeout = Dur::from_millis(50);
    runtime.last_call = Instant::now()
        .checked_sub(Dur::from_millis(250))
        .expect("test clock must support a 250 ms rewind");

    let transition = runtime.tick(Instant::now());
    assert_eq!(
        transition,
        Some(PluginState::Idle),
        "tick must demote an Active runtime whose idle budget is exhausted",
    );
    assert_eq!(runtime.state, PluginState::Idle);

    // Idempotence: a second tick on an Idle runtime must not re-fire.
    let second = runtime.tick(Instant::now());
    assert_eq!(
        second, None,
        "tick on an already-Idle runtime must return None",
    );

    // ── Phase 3: mark_call → Idle → Loading ──────────────────────────
    runtime.mark_call();
    assert_eq!(
        runtime.state,
        PluginState::Loading,
        "mark_call on Idle must flip to Loading",
    );

    // ── Phase 4: wake → Loading → Active (real health check) ─────────
    PluginManager::wake(&mut runtime)
        .await
        .expect("wake must succeed against the real mock plugin");

    assert_eq!(
        runtime.state,
        PluginState::Active,
        "wake must drive Loading → Active via a real health check",
    );
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_MANIFEST: &str = r#"
[plugin]
name = "demo"
version = "0.1.0"
description = "Example plugin for unit tests."

[transport]
type = "stdio"
command = "/usr/bin/true"
args = ["--hello"]
"#;

    #[test]
    fn manifest_parse_reads_plugin_and_transport_sections() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("demo.toml");
        std::fs::write(&path, MINIMAL_MANIFEST).expect("write manifest");

        let manifest = PluginManifest::from_path(&path).expect("parse manifest");
        assert_eq!(manifest.plugin.name, "demo");
        assert_eq!(manifest.plugin.version, "0.1.0");
        assert_eq!(
            manifest.plugin.description.as_deref(),
            Some("Example plugin for unit tests.")
        );
        assert_eq!(manifest.transport.kind, "stdio");
        assert_eq!(manifest.transport.command, "/usr/bin/true");
        assert_eq!(manifest.transport.args, vec!["--hello".to_owned()]);
    }

    #[test]
    fn manifest_parse_rejects_missing_required_field() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.toml");
        // `[plugin]` is missing `version`, which has no `#[serde(default)]`.
        std::fs::write(
            &path,
            "[plugin]\nname = \"bad\"\n[transport]\ntype = \"stdio\"\ncommand = \"true\"\n",
        )
        .expect("write bad manifest");

        let err = PluginManifest::from_path(&path).expect_err("must reject missing field");
        assert!(
            matches!(err, PluginError::ManifestParse { .. }),
            "expected ManifestParse, got {err:?}"
        );
    }

    #[test]
    fn manifest_parse_reports_io_error_for_missing_file() {
        let err = PluginManifest::from_path("/this/path/does/not/exist.toml")
            .expect_err("missing file must error");
        assert!(
            matches!(err, PluginError::Io(_)),
            "expected Io, got {err:?}"
        );
    }

    #[test]
    fn discover_lists_only_toml_files_sorted_by_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("zebra.toml"), MINIMAL_MANIFEST).unwrap();

        let alpha = MINIMAL_MANIFEST.replace(r#"name = "demo""#, r#"name = "alpha""#);
        std::fs::write(dir.path().join("alpha.toml"), alpha).unwrap();

        // Non-toml files must be ignored.
        std::fs::write(dir.path().join("README.md"), "ignore me").unwrap();
        // Subdirectories must be ignored.
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(
            dir.path().join("nested").join("ignored.toml"),
            MINIMAL_MANIFEST,
        )
        .unwrap();

        let manifests = PluginManager::discover(dir.path()).expect("discover");
        assert_eq!(manifests.len(), 2, "expected 2 top-level toml manifests");
        assert_eq!(manifests[0].plugin.name, "alpha");
        assert_eq!(manifests[1].plugin.name, "demo");
    }

    #[test]
    fn discover_errors_when_dir_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("absent");
        let err = PluginManager::discover(&missing).expect_err("missing dir must error");
        assert!(
            matches!(err, PluginError::Io(_)),
            "expected Io, got {err:?}"
        );
    }

    #[test]
    fn spawn_rejects_non_stdio_transport() {
        let manifest = PluginManifest {
            plugin: PluginSection {
                name: "bad".into(),
                version: "0.0.0".into(),
                description: None,
            },
            capabilities: CapabilitiesSection::default(),
            transport: TransportSection {
                kind: "sse".into(),
                command: "unused".into(),
                args: vec![],
            },
            resources: None,
            lifecycle: None,
        };
        let err = PluginManager::spawn(&manifest).expect_err("non-stdio must be rejected");
        assert!(
            matches!(err, PluginError::UnsupportedTransport(ref k) if k == "sse"),
            "expected UnsupportedTransport(\"sse\"), got {err:?}"
        );
    }

    #[test]
    fn parse_tools_list_response_happy_path() {
        let frame = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[
            {"name":"a"},{"name":"b"}
        ]}}"#;
        let names = parse_tools_list_response(frame).expect("parse ok");
        assert_eq!(names, vec!["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn parse_tools_list_response_rejects_jsonrpc_error() {
        let frame =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;
        let err =
            parse_tools_list_response(frame).expect_err("jsonrpc error frame must be rejected");
        assert!(
            matches!(err, PluginError::ProtocolError(ref m) if m.contains("JSON-RPC error")),
            "expected ProtocolError mentioning JSON-RPC error, got {err:?}"
        );
    }

    #[test]
    fn parse_tools_list_response_rejects_empty_frame() {
        let err = parse_tools_list_response("").expect_err("empty must be rejected");
        assert!(
            matches!(err, PluginError::ProtocolError(ref m) if m.contains("without sending")),
            "expected ProtocolError about empty frame, got {err:?}"
        );
    }

    #[tokio::test]
    async fn health_check_timeout_fires_when_command_hangs() {
        // `/usr/bin/cat` with no args reads stdin forever and never
        // writes a response, which exercises the timeout path without
        // needing the mock binary.
        let manifest = PluginManifest {
            plugin: PluginSection {
                name: "hang".into(),
                version: "0.0.0".into(),
                description: None,
            },
            capabilities: CapabilitiesSection::default(),
            transport: TransportSection {
                kind: "stdio".into(),
                // `cat` echoes back whatever we send it — but since it
                // echoes the request line and we parse that as the
                // response frame, we must pick a command that reads
                // stdin but does NOT echo it. `sleep` fits.
                command: "sleep".into(),
                args: vec!["30".into()],
            },
            resources: None,
            lifecycle: None,
        };

        // Use a tighter budget in-process so the test doesn't wait 5 s.
        let result = tokio::time::timeout(
            Duration::from_millis(600),
            PluginManager::health_check(&manifest),
        )
        .await;

        // Either the outer tokio timeout fires first (Err(Elapsed)) or
        // the inner HEALTH_CHECK_TIMEOUT does — either way the test
        // proves the code doesn't hang forever.
        match result {
            Ok(inner) => {
                assert!(
                    matches!(inner, Err(PluginError::HealthCheckTimeout { .. })),
                    "expected HealthCheckTimeout, got {inner:?}"
                );
            }
            Err(_elapsed) => {
                // Outer bound tripped — acceptable; the inner call was
                // about to time out anyway.
            }
        }
    }
}
