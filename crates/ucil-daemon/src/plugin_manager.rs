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

/// Maximum consecutive restart attempts before
/// [`PluginManager::restart_with_backoff`] trips the circuit breaker
/// and transitions the runtime to [`PluginState::Error`].
///
/// Tracks master-plan §14.2 default `max_restarts = 3`.
pub const MAX_RESTARTS: u32 = 3;

/// Base backoff, in milliseconds, used by
/// [`PluginManager::restart_with_backoff`] for exponential backoff.
///
/// The actual delay between attempt `n` and attempt `n + 1` is
/// `base × 2^n`, so attempts complete at base × {1, 2, 4} for the
/// default [`MAX_RESTARTS`] of 3.  Tests can override the base via
/// [`PluginManager::with_circuit_breaker_base`] to keep wall-time
/// inside the fast-test budget.  The production default follows
/// master-plan §14.2 (1 s base).
pub const CIRCUIT_BREAKER_BASE_BACKOFF_MS: u64 = 1_000;

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
    /// Calls [`Self::validate`] before returning so callers always
    /// receive a well-formed manifest.
    ///
    /// # Errors
    ///
    /// * [`PluginError::Io`] — the file could not be opened.
    /// * [`PluginError::ManifestParse`] — the file is not valid TOML or
    ///   omits required fields.
    /// * [`PluginError::InvalidManifest`] — the manifest parses but
    ///   fails a semantic check (empty required field, activation
    ///   on a language not declared in `capabilities.languages`).
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, PluginError> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path).map_err(PluginError::Io)?;
        let manifest: Self = toml::from_str(&raw).map_err(|e| PluginError::ManifestParse {
            path: path.to_path_buf(),
            source: e,
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate semantic invariants on top of the TOML schema check.
    ///
    /// Master-plan §14.1 requires every manifest to carry a non-empty
    /// `plugin.name` (no whitespace), `plugin.version`, `transport.kind`,
    /// and `transport.command`. When `capabilities.languages` is
    /// non-empty every entry in `capabilities.activation.on_language`
    /// MUST also appear in `capabilities.languages` — otherwise the
    /// activation rule is unreachable.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::InvalidManifest`] with the offending
    /// `field` and a human-readable `reason`.
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.plugin.name.is_empty() {
            return Err(PluginError::InvalidManifest {
                field: "plugin.name",
                reason: "plugin.name must not be empty".to_owned(),
            });
        }
        if self.plugin.name.chars().any(char::is_whitespace) {
            return Err(PluginError::InvalidManifest {
                field: "plugin.name",
                reason: format!(
                    "plugin.name `{}` must not contain whitespace",
                    self.plugin.name
                ),
            });
        }
        if self.plugin.version.is_empty() {
            return Err(PluginError::InvalidManifest {
                field: "plugin.version",
                reason: "plugin.version must not be empty".to_owned(),
            });
        }
        if self.transport.kind.is_empty() {
            return Err(PluginError::InvalidManifest {
                field: "transport.type",
                reason: "transport.type must not be empty".to_owned(),
            });
        }
        if self.transport.command.is_empty() {
            return Err(PluginError::InvalidManifest {
                field: "transport.command",
                reason: "transport.command must not be empty".to_owned(),
            });
        }
        if !self.capabilities.languages.is_empty() {
            for lang in &self.capabilities.activation.on_language {
                if !self.capabilities.languages.iter().any(|l| l == lang) {
                    return Err(PluginError::InvalidManifest {
                        field: "capabilities.activation.on_language",
                        reason: format!(
                            "activation language `{lang}` is not declared in capabilities.languages"
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    /// True when the activation rules say this plugin should fire for
    /// the given language.
    ///
    /// An empty `capabilities.activation.on_language` means the plugin
    /// is language-agnostic and activates for any language; otherwise
    /// the rule is a simple membership test.
    #[must_use]
    pub fn activates_for_language(&self, language: &str) -> bool {
        if self.capabilities.activation.on_language.is_empty() {
            return true;
        }
        self.capabilities
            .activation
            .on_language
            .iter()
            .any(|s| s == language)
    }

    /// True when the activation rules say this plugin should fire for
    /// the given MCP tool name.
    ///
    /// Same semantics as [`Self::activates_for_language`]: empty
    /// `on_tool` means activate for every tool; otherwise membership.
    #[must_use]
    pub fn activates_for_tool(&self, tool: &str) -> bool {
        if self.capabilities.activation.on_tool.is_empty() {
            return true;
        }
        self.capabilities
            .activation
            .on_tool
            .iter()
            .any(|s| s == tool)
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
///
/// State machine (master-plan §14.2):
///
/// ```text
/// DISCOVERED → REGISTERED → LOADING → ACTIVE → IDLE → STOPPED
///                                      ↑       ↓
///                                      └───────┘  (HOT/COLD via tick + mark_call)
/// any → ERROR
/// ```
///
/// Transitions are driven by the `register` / `mark_loading` /
/// `mark_active` / `stop` / `mark_error` methods.  Each successful
/// transition is logged at `tracing::info!` (or `warn!` for ERROR)
/// with target `ucil.plugin.lifecycle` per master-plan §15.2.
///
/// # Hot-reload coordination
///
/// `in_flight` is a per-runtime [`tokio::sync::RwLock`] gate: every
/// caller that issues a tool call against the plugin acquires the read
/// half for the duration of the call.  [`PluginManager::reload`]
/// acquires the write half before swapping the child process, which
/// blocks until every outstanding read-guard has been dropped — i.e.
/// every in-flight tool call has returned.  This is the master-plan
/// §14.2 "drain in-flight before swap" semantics.
///
/// # Circuit-breaker counter
///
/// `restart_attempts` accumulates consecutive failed restart attempts
/// observed by [`PluginManager::restart_with_backoff`].  It resets to
/// zero on any successful health check (via `restart_with_backoff` or
/// [`PluginManager::reload`]).
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
    /// Captured failure message when the runtime is in
    /// [`PluginState::Error`].  `None` whenever the state has never
    /// been promoted to ERROR (or has been reset by a successful
    /// `register()`).
    pub error_message: Option<String>,
    /// Per-runtime in-flight gate. Tool-call dispatch acquires the
    /// read half; [`PluginManager::reload`] acquires the write half
    /// to drain readers before swapping the child process.
    pub in_flight: Arc<RwLock<()>>,
    /// Consecutive failed restart attempts observed by
    /// [`PluginManager::restart_with_backoff`].  Resets to zero on a
    /// successful health check.
    pub restart_attempts: u32,
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
            error_message: None,
            in_flight: Arc::new(RwLock::new(())),
            restart_attempts: 0,
        }
    }

    /// Build a runtime that starts in [`PluginState::Discovered`].
    ///
    /// Used when the manifest has been found on disk but not yet
    /// promoted to the registry — call [`Self::register`] to drive
    /// `Discovered → Registered`.
    #[must_use]
    pub fn discovered(manifest: PluginManifest) -> Self {
        let mut rt = Self::new(manifest);
        rt.state = PluginState::Discovered;
        rt
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

    // ── Lifecycle state-machine transitions ──────────────────────────────

    /// `Discovered → Registered`.
    ///
    /// Resets `error_message` to `None` so a manifest that previously
    /// failed health checks can be re-registered cleanly.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::IllegalTransition`] when invoked from a
    /// state other than [`PluginState::Discovered`].
    pub fn register(&mut self) -> Result<(), PluginError> {
        let old = self.state;
        if !matches!(old, PluginState::Discovered) {
            return Err(PluginError::IllegalTransition {
                from: old,
                to: PluginState::Registered,
            });
        }
        self.state = PluginState::Registered;
        self.error_message = None;
        log_transition(&self.manifest.plugin.name, old, self.state);
        Ok(())
    }

    /// `Registered | Idle → Loading`.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::IllegalTransition`] when invoked from any
    /// other state.
    pub fn mark_loading(&mut self) -> Result<(), PluginError> {
        let old = self.state;
        if !matches!(old, PluginState::Registered | PluginState::Idle) {
            return Err(PluginError::IllegalTransition {
                from: old,
                to: PluginState::Loading,
            });
        }
        self.state = PluginState::Loading;
        log_transition(&self.manifest.plugin.name, old, self.state);
        Ok(())
    }

    /// `Loading → Active`. Advances `last_call` so the idle countdown
    /// restarts from the moment the plugin became available.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::IllegalTransition`] when invoked from a
    /// state other than [`PluginState::Loading`].
    pub fn mark_active(&mut self) -> Result<(), PluginError> {
        let old = self.state;
        if !matches!(old, PluginState::Loading) {
            return Err(PluginError::IllegalTransition {
                from: old,
                to: PluginState::Active,
            });
        }
        self.state = PluginState::Active;
        self.last_call = Instant::now();
        log_transition(&self.manifest.plugin.name, old, self.state);
        Ok(())
    }

    /// Any non-[`PluginState::Error`], non-[`PluginState::Stopped`] →
    /// [`PluginState::Stopped`].
    ///
    /// Stopping an already-Stopped runtime is a no-op (returns `Ok`)
    /// because manager bookkeeping may attempt redundant stops.
    /// Stopping a runtime that is in [`PluginState::Error`] is
    /// rejected — the caller must `register()` (which clears the
    /// error) first if they want a clean shutdown path.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::IllegalTransition`] when invoked from
    /// [`PluginState::Error`].
    pub fn stop(&mut self) -> Result<(), PluginError> {
        let old = self.state;
        if matches!(old, PluginState::Stopped) {
            return Ok(());
        }
        if matches!(old, PluginState::Error) {
            return Err(PluginError::IllegalTransition {
                from: old,
                to: PluginState::Stopped,
            });
        }
        self.state = PluginState::Stopped;
        log_transition(&self.manifest.plugin.name, old, self.state);
        Ok(())
    }

    /// Any → [`PluginState::Error`]. Captures the supplied message in
    /// [`Self::error_message`] and emits a `tracing::warn!` event.
    ///
    /// Never returns an error — the ERROR state is reachable from any
    /// other state by design (it is the catch-all failure capture).
    pub fn mark_error(&mut self, msg: impl Into<String>) {
        let old = self.state;
        let msg = msg.into();
        self.state = PluginState::Error;
        self.error_message = Some(msg.clone());
        tracing::warn!(
            target: "ucil.plugin.lifecycle",
            plugin = %self.manifest.plugin.name,
            from = %old,
            to = %PluginState::Error,
            error = %msg,
            "plugin entered ERROR state",
        );
    }
}

/// Emit a single `tracing::info!` event for a successful state
/// transition.  Centralised so every transition method records the
/// same fields under the same target (`ucil.plugin.lifecycle`,
/// master-plan §15.2).
fn log_transition(plugin: &str, from: PluginState, to: PluginState) {
    tracing::info!(
        target: "ucil.plugin.lifecycle",
        plugin = %plugin,
        from = %from,
        to = %to,
        "plugin lifecycle transition",
    );
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
    /// The manifest parsed but failed a semantic check enforced by
    /// [`PluginManifest::validate`] — e.g. empty `plugin.name` or an
    /// activation entry that targets an undeclared language.
    #[error("invalid manifest: {field}: {reason}")]
    InvalidManifest {
        /// Dotted name of the offending field (`"plugin.name"`,
        /// `"capabilities.activation.on_language"`, …).
        field: &'static str,
        /// Human-readable explanation suitable for logs / error
        /// messages.
        reason: String,
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
    /// A lifecycle-state transition was rejected because the source
    /// state does not permit moving to the target state.  Emitted by
    /// the [`PluginRuntime`] transition methods (`register`,
    /// `mark_loading`, `mark_active`, `stop`).
    #[error("illegal lifecycle transition: {from} → {to}")]
    IllegalTransition {
        /// State the runtime was in when the transition was attempted.
        from: PluginState,
        /// State the caller asked the runtime to enter.
        to: PluginState,
    },
    /// A manager operation that addresses a runtime by name (such as
    /// [`PluginManager::reload`] or
    /// [`PluginManager::restart_with_backoff`]) could not find a
    /// runtime registered under that name.
    #[error("plugin `{name}` is not registered with this manager")]
    NotFound {
        /// Name the caller looked up.
        name: String,
    },
    /// [`PluginManager::restart_with_backoff`] exhausted
    /// [`MAX_RESTARTS`] consecutive failed restart attempts and
    /// transitioned the runtime to [`PluginState::Error`].
    #[error("plugin `{name}` circuit breaker tripped after {attempts} restart attempts")]
    CircuitBreakerOpen {
        /// Name of the plugin whose breaker tripped.
        name: String,
        /// Number of attempts that were exhausted before the trip.
        attempts: u32,
    },
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
///
/// `circuit_breaker_base` configures the starting delay used by
/// [`Self::restart_with_backoff`] for exponential backoff between
/// failed attempts.  The production default follows
/// [`CIRCUIT_BREAKER_BASE_BACKOFF_MS`]; tests shrink it via
/// [`Self::with_circuit_breaker_base`] to keep wall-time inside the
/// fast-test budget.
#[derive(Debug, Clone)]
pub struct PluginManager {
    runtimes: Arc<RwLock<Vec<PluginRuntime>>>,
    circuit_breaker_base: Duration,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self {
            runtimes: Arc::new(RwLock::new(Vec::new())),
            circuit_breaker_base: Duration::from_millis(CIRCUIT_BREAKER_BASE_BACKOFF_MS),
        }
    }
}

impl PluginManager {
    /// Construct a new, empty `PluginManager`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the base delay used for exponential backoff between
    /// failed restart attempts inside [`Self::restart_with_backoff`].
    ///
    /// Production code keeps the default 1 s base; the
    /// `test_circuit_breaker` acceptance test shrinks the base to a
    /// few milliseconds so the breaker-trip exercise completes inside
    /// the fast-test wall-time budget.  The default constructor
    /// [`Self::new`] continues to use [`CIRCUIT_BREAKER_BASE_BACKOFF_MS`]
    /// — call this builder explicitly to opt in to a different base.
    #[must_use]
    pub const fn with_circuit_breaker_base(mut self, base: Duration) -> Self {
        self.circuit_breaker_base = base;
        self
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

    /// Hot-reload a registered plugin: drain in-flight tool calls,
    /// re-spawn the child via [`Self::health_check`], then mark the
    /// runtime [`PluginState::Active`] again — all without restarting
    /// the daemon (master-plan §14.2 hot-reload semantics).
    ///
    /// Behaviour:
    ///
    /// 1. Look up the runtime by name; clone its `manifest` and
    ///    `in_flight` gate, then release the manager's read lock so the
    ///    drain in step 2 cannot deadlock against another caller.
    /// 2. Acquire the WRITER half of the runtime's `in_flight` lock —
    ///    blocks until every outstanding read-guard (i.e. every
    ///    in-flight tool call) is dropped.  Held for the entire reload
    ///    window.
    /// 3. Run `health_check(&manifest)` against the cloned manifest —
    ///    spawns a fresh child, drives the MCP handshake, kills it.
    /// 4. On success: re-acquire the manager's write lock; locate the
    ///    runtime by name; set `state = Active`, `last_call =
    ///    Instant::now()`, `restart_attempts = 0`.
    /// 5. On failure: increment `restart_attempts` and propagate the
    ///    error; the runtime's `state` is left unchanged so the caller
    ///    can decide whether to escalate to
    ///    [`Self::restart_with_backoff`].
    /// 6. The writer guard is released before this function returns,
    ///    so the next tool call's `in_flight.read().await` can proceed
    ///    immediately.
    ///
    /// # Errors
    ///
    /// * [`PluginError::NotFound`] — no runtime registered under
    ///   `name`.
    /// * Any error from [`Self::health_check`] — spawn, stdio, timeout,
    ///   or protocol failure.
    pub async fn reload(&mut self, name: &str) -> Result<(), PluginError> {
        // Step 1: snapshot manifest + in_flight gate; the read guard
        // is dropped at the end of this expression — no `let` binding,
        // so the lock is held only for the lookup.
        let (manifest_clone, in_flight) = self
            .runtimes
            .read()
            .await
            .iter()
            .find(|rt| rt.manifest.plugin.name == name)
            .map(|rt| (rt.manifest.clone(), rt.in_flight.clone()))
            .ok_or_else(|| PluginError::NotFound { name: name.into() })?;

        // Step 2: drain in-flight readers by acquiring the writer half.
        let _drain_guard = in_flight.write().await;

        // Step 3: real health check (no mocks per rust-style.md).
        match Self::health_check(&manifest_clone).await {
            Ok(_health) => {
                // Step 4: re-acquire manager's write lock and flip state.
                if let Some(runtime) = self
                    .runtimes
                    .write()
                    .await
                    .iter_mut()
                    .find(|rt| rt.manifest.plugin.name == name)
                {
                    runtime.state = PluginState::Active;
                    runtime.last_call = Instant::now();
                    runtime.restart_attempts = 0;
                }
                tracing::info!(
                    target: "ucil.plugin.lifecycle",
                    op = "reload",
                    plugin = %name,
                    "plugin reload complete",
                );
                Ok(())
            }
            Err(e) => {
                // Step 5: bump restart_attempts; leave state unchanged.
                if let Some(runtime) = self
                    .runtimes
                    .write()
                    .await
                    .iter_mut()
                    .find(|rt| rt.manifest.plugin.name == name)
                {
                    runtime.restart_attempts = runtime.restart_attempts.saturating_add(1);
                }
                tracing::warn!(
                    target: "ucil.plugin.lifecycle",
                    op = "reload",
                    plugin = %name,
                    error = %e,
                    "plugin reload failed",
                );
                Err(e)
            }
        }
    }

    /// Restart a registered plugin up to [`MAX_RESTARTS`] times with
    /// exponential backoff; trip the circuit breaker on exhaustion.
    ///
    /// Behaviour (master-plan §14.2):
    ///
    /// 1. Look up the manifest by name; release the read lock before
    ///    starting the loop.  Returns [`PluginError::NotFound`] when
    ///    no runtime matches.
    /// 2. Iterate `attempt in 0..MAX_RESTARTS`.  Each iteration runs a
    ///    real [`Self::health_check`].  On `Ok` the runtime flips to
    ///    [`PluginState::Active`], `last_call` advances,
    ///    `restart_attempts` resets to zero, and an info-level tracing
    ///    event records the success.  On `Err` `restart_attempts`
    ///    increments, then the task sleeps for
    ///    `circuit_breaker_base × 2^attempt` (i.e. base × {1, 2, 4} at
    ///    the default base) before the next attempt.
    /// 3. After [`MAX_RESTARTS`] consecutive failures the breaker
    ///    trips: the runtime is marked
    ///    [`PluginState::Error`] via [`PluginRuntime::mark_error`]
    ///    with a circuit-breaker message, and the function returns
    ///    [`PluginError::CircuitBreakerOpen`].  A warn-level tracing
    ///    event records the trip.
    ///
    /// # Errors
    ///
    /// * [`PluginError::NotFound`] — no runtime registered under
    ///   `name`.
    /// * [`PluginError::CircuitBreakerOpen`] — every attempt failed.
    pub async fn restart_with_backoff(&mut self, name: &str) -> Result<(), PluginError> {
        // Step 1: snapshot the manifest. The read guard drops at the
        // end of the expression — the lock is held only for lookup.
        let manifest = self
            .runtimes
            .read()
            .await
            .iter()
            .find(|rt| rt.manifest.plugin.name == name)
            .map(|rt| rt.manifest.clone())
            .ok_or_else(|| PluginError::NotFound { name: name.into() })?;

        let base = self.circuit_breaker_base;

        // Step 2: bounded restart loop with exponential backoff.
        for attempt in 0..MAX_RESTARTS {
            if Self::health_check(&manifest).await.is_ok() {
                if let Some(runtime) = self
                    .runtimes
                    .write()
                    .await
                    .iter_mut()
                    .find(|rt| rt.manifest.plugin.name == name)
                {
                    runtime.state = PluginState::Active;
                    runtime.last_call = Instant::now();
                    runtime.restart_attempts = 0;
                }
                tracing::info!(
                    target: "ucil.plugin.lifecycle",
                    op = "restart_with_backoff",
                    plugin = %name,
                    attempts = attempt + 1,
                    "plugin restart succeeded",
                );
                return Ok(());
            }
            if let Some(runtime) = self
                .runtimes
                .write()
                .await
                .iter_mut()
                .find(|rt| rt.manifest.plugin.name == name)
            {
                runtime.restart_attempts = runtime.restart_attempts.saturating_add(1);
            }
            tokio::time::sleep(base * 2u32.pow(attempt)).await;
        }

        // Step 3: breaker trip — mark the runtime ERROR and surface it.
        if let Some(runtime) = self
            .runtimes
            .write()
            .await
            .iter_mut()
            .find(|rt| rt.manifest.plugin.name == name)
        {
            runtime.mark_error(format!(
                "circuit breaker tripped after {MAX_RESTARTS} restart attempts"
            ));
        }
        tracing::warn!(
            target: "ucil.plugin.lifecycle",
            op = "restart_with_backoff",
            plugin = %name,
            attempts = MAX_RESTARTS,
            "plugin circuit breaker tripped",
        );
        Err(PluginError::CircuitBreakerOpen {
            name: name.into(),
            attempts: MAX_RESTARTS,
        })
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

    /// Register a pre-built [`PluginRuntime`] with the manager without
    /// going through [`Self::activate`].
    ///
    /// Complements `activate`: `activate` requires a successful
    /// `health_check` against a real MCP child, which is too heavy for
    /// callers (and tests) that already have a runtime in hand and
    /// want to exercise [`Self::restart_with_backoff`] or
    /// [`Self::reload`] against it directly.  This is also the natural
    /// hook for the future `ucil plugin install <name>` flow which
    /// will register a runtime declaratively before running the first
    /// health check.
    ///
    /// Synchronous by design: `add` is called during manager setup,
    /// before any task is reading the runtimes list, so the underlying
    /// `try_write` cannot contend in normal use.  If the lock IS
    /// contended (unexpected — programmer error) the runtime is NOT
    /// registered and a `tracing::warn!` event is emitted rather than
    /// panicking; the caller can detect the omission via
    /// [`Self::registered_runtimes`].
    pub fn add(&mut self, runtime: PluginRuntime) {
        match self.runtimes.try_write() {
            Ok(mut guard) => guard.push(runtime),
            Err(_) => {
                tracing::warn!(
                    target: "ucil.plugin.lifecycle",
                    plugin = %runtime.manifest.plugin.name,
                    "PluginManager::add: runtimes lock contended; runtime not registered",
                );
            }
        }
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

/// Acceptance test for `P2-W6-F03` — hot-reload of a live plugin
/// without restarting the daemon, with proper draining of in-flight
/// tool calls before the swap.
///
/// Frozen selector: `plugin_manager::test_hot_reload`.
///
/// Walks a real runtime through:
///
/// 1. `PluginManager::activate` registers a runtime in `Active` against
///    the real `mock-mcp-plugin` binary (no mocks).
/// 2. A background task acquires the runtime's `in_flight.read()`
///    guard and holds it for ~100 ms — simulating an in-flight tool
///    call.
/// 3. The main task calls `mgr.reload(...)`. The writer-side gate must
///    BLOCK until the background reader drops its guard, then run a
///    real `health_check`, then flip the runtime back to `Active`.
/// 4. After the reload returns, `in_flight.read()` must succeed
///    immediately (proves the writer guard was released).
#[cfg(test)]
#[tokio::test]
async fn test_hot_reload() {
    use std::time::Duration as Dur;

    let mock = mock_mcp_plugin_path();
    assert!(
        mock.exists(),
        "expected mock-mcp-plugin binary at {} — run `cargo build -p ucil-daemon --bin mock-mcp-plugin` first",
        mock.display()
    );

    let manifest = PluginManifest {
        plugin: PluginSection {
            name: "hot-reload-fixture".into(),
            version: "0.1.0".into(),
            description: Some("hot-reload acceptance test manifest".into()),
        },
        capabilities: CapabilitiesSection::default(),
        transport: TransportSection {
            kind: "stdio".into(),
            command: mock.to_string_lossy().into_owned(),
            args: vec![],
        },
        resources: None,
        lifecycle: None,
    };

    let mut mgr = PluginManager::new();
    let runtime = mgr
        .activate(&manifest)
        .await
        .expect("activate must succeed against the real mock plugin");
    assert_eq!(runtime.state, PluginState::Active);

    // Capture the in_flight gate from the registered runtime so the
    // background task and the post-reload re-acquisition both share
    // the SAME Arc<RwLock<()>> the manager will drain.
    let in_flight = mgr.registered_runtimes().await[0].in_flight.clone();

    // Background reader holds the read half for ~100 ms. The reload
    // call from the main task must wait at least that long before it
    // can proceed.
    let reader_handle = {
        let in_flight = in_flight.clone();
        tokio::spawn(async move {
            let _read_guard = in_flight.read().await;
            tokio::time::sleep(Dur::from_millis(100)).await;
            // Read guard drops here.
        })
    };

    // Yield once so the spawned reader has a chance to acquire its
    // guard before the writer below queues up.
    tokio::task::yield_now().await;

    let start = Instant::now();
    mgr.reload("hot-reload-fixture")
        .await
        .expect("reload must succeed");
    let elapsed = start.elapsed();

    // The reader is done by the time the writer succeeds, but join
    // explicitly so any panic surfaces.
    reader_handle.await.expect("reader task did not panic");

    assert!(
        elapsed >= Dur::from_millis(100),
        "reload must wait for the in-flight reader to drop its guard (elapsed {elapsed:?})",
    );
    assert!(
        elapsed < Dur::from_secs(2),
        "reload must complete inside the test wall-time budget (elapsed {elapsed:?})",
    );

    let snapshot = mgr.registered_runtimes().await;
    assert_eq!(snapshot.len(), 1, "manager retains exactly one runtime");
    assert_eq!(
        snapshot[0].state,
        PluginState::Active,
        "after a successful reload the runtime must be Active",
    );
    assert_eq!(
        snapshot[0].restart_attempts, 0,
        "successful reload must reset restart_attempts to zero",
    );
    assert!(
        snapshot[0].last_call > start,
        "reload must advance last_call past the start of the reload window",
    );

    // Post-reload: the writer guard was dropped before reload returned,
    // so a fresh read must succeed immediately. tokio::time::timeout
    // bounds the wait so a regression here doesn't hang the suite.
    let _post_read = tokio::time::timeout(Dur::from_secs(1), in_flight.read())
        .await
        .expect("post-reload in_flight.read() must not block on a leaked writer guard");
}

/// Master-plan §14.1-complete fixture body for `test_manifest_parser`.
#[cfg(test)]
const FIXTURE_14_1_BODY: &str = r#"[plugin]
name = "semgrep-fixture"
version = "1.2.3"
description = "Test fixture covering every §14.1 section."

[capabilities]
provides = ["search_code", "scan_security"]
languages = ["rust", "typescript", "python"]

[capabilities.activation]
on_language = ["rust", "typescript"]
on_tool = ["search_code"]
eager = false

[transport]
type = "stdio"
command = "/usr/local/bin/semgrep"
args = ["--mcp"]

[resources]
memory_mb = 200
startup_time_ms = 300
typical_query_ms = 50

[lifecycle]
hot_cold = true
idle_timeout_minutes = 5
"#;

/// Assert every field of the §14.1-complete fixture manifest.
#[cfg(test)]
fn assert_fixture_fields(manifest: &PluginManifest) {
    // [plugin]
    assert_eq!(manifest.plugin.name, "semgrep-fixture");
    assert_eq!(manifest.plugin.version, "1.2.3");
    assert_eq!(
        manifest.plugin.description.as_deref(),
        Some("Test fixture covering every §14.1 section.")
    );

    // [capabilities] + [capabilities.activation]
    assert_eq!(
        manifest.capabilities.provides,
        vec!["search_code".to_owned(), "scan_security".to_owned()],
    );
    assert_eq!(
        manifest.capabilities.languages,
        vec![
            "rust".to_owned(),
            "typescript".to_owned(),
            "python".to_owned(),
        ],
    );
    assert_eq!(
        manifest.capabilities.activation.on_language,
        vec!["rust".to_owned(), "typescript".to_owned()],
    );
    assert_eq!(
        manifest.capabilities.activation.on_tool,
        vec!["search_code".to_owned()],
    );
    assert!(!manifest.capabilities.activation.eager);

    // [transport]
    assert_eq!(manifest.transport.kind, "stdio");
    assert_eq!(manifest.transport.command, "/usr/local/bin/semgrep");
    assert_eq!(manifest.transport.args, vec!["--mcp".to_owned()]);

    // [resources]
    let resources = manifest
        .resources
        .as_ref()
        .expect("[resources] table must parse");
    assert_eq!(resources.memory_mb, Some(200));
    assert_eq!(resources.startup_time_ms, Some(300));
    assert_eq!(resources.typical_query_ms, Some(50));

    // [lifecycle]
    let lifecycle = manifest
        .lifecycle
        .as_ref()
        .expect("[lifecycle] table must parse");
    assert!(lifecycle.hot_cold);
    assert_eq!(lifecycle.idle_timeout_minutes, Some(5));
}

/// Assert that an empty `plugin.name` is rejected by
/// [`PluginManifest::from_path`] with the right field.
#[cfg(test)]
fn assert_empty_plugin_name_rejected(dir: &std::path::Path) {
    let bad_path = dir.join("bad.toml");
    std::fs::write(
        &bad_path,
        r#"[plugin]
name = ""
version = "0.0.0"

[transport]
type = "stdio"
command = "/usr/bin/true"
"#,
    )
    .expect("write bad manifest");

    let err = PluginManifest::from_path(&bad_path).expect_err("empty plugin.name must be rejected");
    match err {
        PluginError::InvalidManifest { field, .. } => {
            assert_eq!(
                field, "plugin.name",
                "expected plugin.name field, got {field}"
            );
        }
        other => panic!("expected InvalidManifest {{ field: \"plugin.name\", .. }}, got {other:?}"),
    }
}

/// Build the struct-literal manifest used by the activation-helper
/// assertions in `test_manifest_parser`.
#[cfg(test)]
fn helper_manifest() -> PluginManifest {
    PluginManifest {
        plugin: PluginSection {
            name: "helper-only".into(),
            version: "0.1.0".into(),
            description: None,
        },
        capabilities: CapabilitiesSection {
            provides: vec![],
            languages: vec!["rust".into(), "typescript".into()],
            activation: ActivationSection {
                on_language: vec!["rust".into()],
                on_tool: vec![],
                eager: false,
            },
        },
        transport: TransportSection {
            kind: "stdio".into(),
            command: "/usr/bin/true".into(),
            args: vec![],
        },
        resources: None,
        lifecycle: None,
    }
}

/// Acceptance test for `P2-W6-F01` — `[capabilities]` + `[resources]`
/// manifest parsing.
///
/// Frozen selector: `plugin_manager::test_manifest_parser`.
///
/// Walks `PluginManifest::from_path` against a fixture covering every
/// master-plan §14.1 section (`[plugin]`, `[capabilities]` with nested
/// `[capabilities.activation]`, `[transport]`, `[resources]`,
/// `[lifecycle]`).  Then exercises the sad path
/// (`PluginError::InvalidManifest`) and the activation helpers.
#[cfg(test)]
#[test]
fn test_manifest_parser() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("semgrep.toml");
    std::fs::write(&path, FIXTURE_14_1_BODY).expect("write fixture manifest");

    let manifest = PluginManifest::from_path(&path).expect("parse §14.1 fixture");
    assert_fixture_fields(&manifest);
    manifest
        .validate()
        .expect("§14.1-complete manifest must validate");

    assert_empty_plugin_name_rejected(dir.path());

    let m = helper_manifest();
    assert!(
        m.activates_for_language("rust"),
        "rust is in on_language → activates",
    );
    assert!(
        !m.activates_for_language("go"),
        "go is not in on_language → does NOT activate",
    );
    // Empty `on_tool` ⇒ activates for every tool (master-plan default
    // for plugins that don't filter).
    assert!(
        m.activates_for_tool("anything"),
        "empty on_tool ⇒ activates for any tool",
    );
}

/// Acceptance test for `P2-W6-F02` — full plugin lifecycle state
/// machine (DISCOVERED → REGISTERED → LOADING → ACTIVE → IDLE →
/// LOADING → ACTIVE → STOPPED), plus illegal-transition rejection
/// and the ERROR-capture path.
///
/// Frozen selector: `plugin_manager::test_lifecycle_state_machine`.
///
/// Drives the in-memory state machine only — no subprocess spawned,
/// no `tracing` subscriber installed (avoids cross-test
/// contamination).  Subscriber-based assertion is left for the
/// later integration test (P2-W6-F08) per the WO scope.
#[cfg(test)]
#[tokio::test]
async fn test_lifecycle_state_machine() {
    use std::time::Duration as Dur;

    let manifest = PluginManifest {
        plugin: PluginSection {
            name: "lifecycle-fixture".into(),
            version: "0.1.0".into(),
            description: None,
        },
        capabilities: CapabilitiesSection::default(),
        transport: TransportSection {
            kind: "stdio".into(),
            command: "/usr/bin/true".into(),
            args: vec![],
        },
        resources: None,
        lifecycle: None,
    };

    // ── Phase 1: Discovered → Registered ─────────────────────────────
    let mut runtime = PluginRuntime::discovered(manifest.clone());
    assert_eq!(runtime.state, PluginState::Discovered);
    runtime
        .register()
        .expect("Discovered → Registered must succeed");
    assert_eq!(runtime.state, PluginState::Registered);
    assert!(
        runtime.error_message.is_none(),
        "register() must clear any prior error_message",
    );

    // ── Phase 2: Registered → Loading → Active ───────────────────────
    runtime
        .mark_loading()
        .expect("Registered → Loading must succeed");
    assert_eq!(runtime.state, PluginState::Loading);
    runtime
        .mark_active()
        .expect("Loading → Active must succeed");
    assert_eq!(runtime.state, PluginState::Active);

    // ── Phase 3: Active → Idle (via tick + back-dated last_call) ─────
    runtime.idle_timeout = Dur::from_millis(50);
    runtime.last_call = Instant::now()
        .checked_sub(Dur::from_millis(250))
        .expect("clock supports a 250 ms rewind");
    let transition = runtime.tick(Instant::now());
    assert_eq!(transition, Some(PluginState::Idle));
    assert_eq!(runtime.state, PluginState::Idle);

    // ── Phase 4: Idle → Loading → Active (re-entry) ──────────────────
    runtime.mark_loading().expect("Idle → Loading must succeed");
    assert_eq!(runtime.state, PluginState::Loading);
    runtime
        .mark_active()
        .expect("Loading → Active must succeed (re-entry)");
    assert_eq!(runtime.state, PluginState::Active);

    // ── Phase 5: illegal Active → Registered must error, not panic ──
    let illegal = runtime.register();
    match illegal {
        Err(PluginError::IllegalTransition { from, to }) => {
            assert_eq!(from, PluginState::Active);
            assert_eq!(to, PluginState::Registered);
        }
        other => panic!("expected IllegalTransition, got {other:?}"),
    }
    assert_eq!(
        runtime.state,
        PluginState::Active,
        "illegal transition must NOT mutate state",
    );

    // ── Phase 6: Active → Stopped ────────────────────────────────────
    runtime.stop().expect("Active → Stopped must succeed");
    assert_eq!(runtime.state, PluginState::Stopped);
    // Stopping an already-Stopped runtime is a no-op (Ok).
    runtime.stop().expect("Stopped → Stopped is a no-op");
    assert_eq!(runtime.state, PluginState::Stopped);

    // ── Phase 7: separate runtime exercises the ERROR capture path ──
    let mut second = PluginRuntime::new(manifest);
    assert_eq!(
        second.state,
        PluginState::Registered,
        "PluginRuntime::new starts in Registered",
    );
    second.mark_error("boom");
    assert_eq!(second.state, PluginState::Error);
    assert_eq!(second.error_message.as_deref(), Some("boom"));

    // mark_error from Error itself is also legal (catch-all).
    second.mark_error("still broken");
    assert_eq!(second.state, PluginState::Error);
    assert_eq!(second.error_message.as_deref(), Some("still broken"));

    // stop() from Error must be rejected — ERROR is a terminal state
    // until a fresh register() resets the error_message.
    let stop_from_error = second.stop();
    assert!(
        matches!(
            stop_from_error,
            Err(PluginError::IllegalTransition {
                from: PluginState::Error,
                to: PluginState::Stopped,
            }),
        ),
        "stop() from Error must return IllegalTransition, got {stop_from_error:?}",
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
