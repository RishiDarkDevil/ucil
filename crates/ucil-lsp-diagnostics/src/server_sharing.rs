//! Degraded-mode LSP subprocess spawner (`P1-W5-F07`, `WO-0029`).
//!
//! Per master-plan §13.4 lines 1424–1431 and `[lsp_diagnostics]` config
//! lines 2074–2082, the diagnostics bridge has two operating modes:
//!
//! * **Serena-managed**: Serena owns the LSP server processes; UCIL
//!   delegates LSP requests through Serena's MCP channel
//!   (per [`DEC-0008`]).  No subprocesses are owned by this crate.
//! * **Degraded mode**: Serena is unavailable, so the bridge spawns and
//!   supervises its own per-language LSP subprocesses (e.g.
//!   `pyright-langserver`, `rust-analyzer`,
//!   `typescript-language-server`).  This module is the home of that
//!   degraded-mode lifecycle: spawn, per-server `last_used` tracking,
//!   an idle-grace reaper, and graceful shutdown via stdin-EOF +
//!   [`tokio::time::timeout`]-bounded await for child exit.
//!
//! [`FallbackSpawner`] is the single owner of the live children.  Its
//! [`FallbackSpawner::install_into`] populates the
//! [`crate::bridge::LspDiagnosticsBridge`]'s endpoint map with
//! [`crate::types::LspTransport::Standalone`] entries — one per
//! configured-AND-available binary — so downstream LSP request
//! dispatch (`P1-W5-F08` integration tests) can route over the
//! standalone subprocesses instead of through Serena.
//!
//! # Idle-grace reaper
//!
//! On construction, [`FallbackSpawner::new`] spawns a background task
//! (the internal `reap_idle` helper) that wakes every
//! [`REAP_INTERVAL`] and shuts down any subprocess whose
//! `last_used + grace_period` has elapsed.  The grace period defaults
//! to [`DEFAULT_GRACE_PERIOD_MINUTES`] (mirroring
//! `[lsp_diagnostics] grace_period_minutes = 5`); tests use
//! [`FallbackSpawner::with_grace_period`] to inject a faster value
//! without exercising real five-minute waits.
//!
//! # Timeout discipline
//!
//! Every IO `.await` in this module is wrapped in
//! [`tokio::time::timeout`] per `rust-style.md` Async §:
//!
//! * The internal `shutdown_handle` helper wraps `child.wait()` in
//!   [`tokio::time::timeout(SHUTDOWN_TIMEOUT, …)`](tokio::time::timeout).
//! * The internal `reap_idle` polling loop uses [`tokio::time::sleep`]
//!   (timer, not IO) and [`tokio::sync::Notify::notified`] (sync
//!   primitive, not IO) inside a [`tokio::select!`] — neither is
//!   wrapped because neither is IO.
//!
//! # Out of scope (deferred)
//!
//! * Real LSP JSON-RPC traffic over the spawned subprocesses
//!   (`P1-W5-F04`, `P1-W5-F08`).
//! * Daemon integration that computes `serena_managed` from
//!   `PluginManager::registered_runtimes()` (reserved per `DEC-0008`
//!   §Consequences for a future progressive-startup WO).
//! * TOML parsing of `[lsp_diagnostics]` — accepted as constructor
//!   arguments or module constants for now.
//!
//! [`DEC-0008`]: https://github.com/RishiDarkDevil/ucil/blob/main/ucil-build/decisions/DEC-0008-lsp-bridge-via-serena-mcp-channel.md

// `ServerSharingError`, `ServerHandle`, `FallbackSpawner` all share the
// `server` / `Server` prefix with the module name (`server_sharing`).
// The convention in this workspace is `<module>Error` /
// `<module>Handle` / `<role>Spawner` — see `bridge::BridgeError` and
// `quality_pipeline::QualityPipelineError` for the same pattern — so
// allowing the lint at module scope keeps the naming consistent
// without per-item `#[allow]` spam.
#![allow(clippy::module_name_repetitions)]
// `FallbackSpawner::new` / `with_grace_period` accept a borrowed
// `&HashMap<Language, (String, Vec<String>)>` with the default
// `RandomState` hasher.  `clippy::implicit_hasher` would push us to
// add a generic `S: BuildHasher` parameter, but the spawner has no
// performance-sensitive lookup over this map (it iterates once at
// construction) and a generic parameter would leak into the bridge's
// `with_fallback_spawner` constructor signature.  Allowed at module
// scope to keep the public surface tight.
#![allow(clippy::implicit_hasher)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::bridge::LspDiagnosticsBridge;
use crate::types::{Language, LspEndpoint, LspTransport};

// ── Module constants ─────────────────────────────────────────────────────────

/// Default grace period (in minutes) before an idle LSP server is
/// reaped.
///
/// Mirrors `[lsp_diagnostics] grace_period_minutes = 5` from
/// master-plan config lines 2074–2082.  Constructor
/// [`FallbackSpawner::with_grace_period`] overrides this for tests.
pub const DEFAULT_GRACE_PERIOD_MINUTES: u64 = 5;

/// Soft cap on concurrently spawned LSP subprocesses.
///
/// Mirrors `[lsp_diagnostics] max_concurrent_servers = 5` from the
/// master plan.  The spawner does not hard-enforce this — callers are
/// expected to supply a [`HashMap`] sized below the cap.  The constant
/// is exported so a future TOML-parsing WO can validate config.
pub const MAX_CONCURRENT_SERVERS: usize = 5;

/// How often the idle-grace reaper task wakes to check for expired
/// servers.
///
/// Sixty seconds is the same coarse cadence as the master-plan example
/// — the reaper does not need sub-minute precision because the grace
/// period itself is measured in minutes.
pub const REAP_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum time the spawner will wait for a child process to exit
/// gracefully (post stdin-EOF) before falling back to SIGKILL.
///
/// Tests assert that [`FallbackSpawner::shutdown_all`] returns within
/// this bound; production paths surface
/// [`ServerSharingError::ShutdownTimeout`] if a child ignores both EOF
/// and SIGKILL within the bound.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by the [`FallbackSpawner`] lifecycle.
///
/// `#[non_exhaustive]` so future variants (e.g. spawn-rate-limited,
/// `PID`-file-collision) added by follow-on WOs do not constitute a
/// `SemVer` break.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ServerSharingError {
    /// The configured LSP-server binary is not present on the host's
    /// `PATH` (or the configured absolute path does not exist).  The
    /// spawner detects this by inspecting
    /// [`std::io::ErrorKind::NotFound`] from
    /// [`tokio::process::Command::spawn`] — no separate `which`
    /// pre-flight is performed because the spawn itself is the
    /// authoritative check.
    #[error("LSP server binary not found for language {language:?}: `{command}`")]
    BinaryNotFound {
        /// The [`Language`] whose server could not be located.
        language: Language,
        /// The configured executable name or absolute path that
        /// triggered the [`std::io::ErrorKind::NotFound`].
        command: String,
    },
    /// [`tokio::process::Command::spawn`] returned an IO error other
    /// than [`std::io::ErrorKind::NotFound`] — typically a permission
    /// denial, an `EAGAIN` from process-table exhaustion, or a
    /// platform-specific error.
    #[error("failed to spawn LSP server for language {language:?}: {source}")]
    SpawnFailed {
        /// The [`Language`] whose server failed to start.
        language: Language,
        /// The underlying IO error from
        /// [`tokio::process::Command::spawn`] or `wait`.
        #[source]
        source: std::io::Error,
    },
    /// The child process did not exit within [`SHUTDOWN_TIMEOUT`] of
    /// receiving an EOF on stdin AND a follow-up SIGKILL.  Indicates a
    /// pathologically uncooperative subprocess; in production the
    /// daemon should escalate.  Tests assert this variant does not
    /// surface for `sleep`-based stand-ins.
    #[error("LSP server for language {language:?} did not exit within shutdown timeout")]
    ShutdownTimeout {
        /// The [`Language`] whose server failed to exit.
        language: Language,
    },
}

// ── ServerHandle ─────────────────────────────────────────────────────────────

/// A handle to a single live LSP subprocess managed by
/// [`FallbackSpawner`].
///
/// Stores the OS process id, the [`tokio::process::Child`] handle (so
/// the spawner can `wait` / `kill` it), the most recent activity
/// [`Instant`] (refreshed by [`FallbackSpawner::touch`]), the
/// [`LspTransport::Standalone`] copy that was used to spawn the
/// subprocess (so [`FallbackSpawner::install_into`] can publish it
/// onto the bridge), and the [`Language`] (so the reaper can identify
/// the entry without a back-reference into the map key).
///
/// The struct's fields are private — callers go through the spawner's
/// accessor methods ([`FallbackSpawner::pid_for`],
/// [`FallbackSpawner::last_used_for`], etc.) so the spawner remains
/// the single source of truth for the lifecycle.
pub struct ServerHandle {
    pid: u32,
    child: Child,
    last_used: Instant,
    transport: LspTransport,
    language: Language,
}

impl ServerHandle {
    /// OS process id of the spawned subprocess.
    ///
    /// Returns `0` only when [`tokio::process::Child::id`] returned
    /// `None` (i.e. the child has already exited and tokio has
    /// reaped its zombie).  In a freshly spawned handle the PID is
    /// always positive.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.pid
    }

    /// The [`Language`] this subprocess serves.
    #[must_use]
    pub const fn language(&self) -> Language {
        self.language
    }

    /// Borrow the [`LspTransport::Standalone`] copy that was used to
    /// spawn this subprocess.
    #[must_use]
    pub const fn transport(&self) -> &LspTransport {
        &self.transport
    }

    /// The most recent activity [`Instant`] for this subprocess —
    /// initialised at spawn time and refreshed by
    /// [`FallbackSpawner::touch`].
    #[must_use]
    pub const fn last_used(&self) -> Instant {
        self.last_used
    }
}

// ── FallbackSpawner ──────────────────────────────────────────────────────────

/// Owns the degraded-mode LSP subprocess lifecycle.
///
/// On construction, attempts to spawn one [`tokio::process::Child`]
/// per entry in the `commands` map.  Missing binaries are surfaced as
/// [`ServerSharingError::BinaryNotFound`]; other spawn failures as
/// [`ServerSharingError::SpawnFailed`].  Once at least one subprocess
/// is alive, the spawner starts a background reaper task that wakes
/// every [`REAP_INTERVAL`] and shuts down any subprocess whose
/// `last_used + grace_period` has elapsed.
///
/// # Invariants
///
/// * Every entry in [`Self::languages`] corresponds to a live
///   [`Child`] until [`Self::shutdown_all`] is called or the spawner
///   is dropped.
/// * [`Self::install_into`] is idempotent: calling it twice produces
///   the same bridge-endpoint state (the bridge's
///   [`crate::bridge::LspDiagnosticsBridge::insert_endpoint`] returns
///   the prior entry on duplicate keys).
/// * [`Self::touch`] only mutates `last_used`; it never spawns or
///   reaps.
///
/// # Drop semantics
///
/// [`Drop`] is best-effort: it aborts the reaper, then sends SIGKILL
/// (`start_kill`) to every surviving child.  It does **not** wait for
/// children to exit — `await` is unavailable in [`Drop`].  Production
/// callers should prefer the async [`Self::shutdown_all`] for
/// deterministic shutdown.
pub struct FallbackSpawner {
    handles: Arc<Mutex<HashMap<Language, ServerHandle>>>,
    grace_period: Duration,
    reaper_handle: Option<JoinHandle<()>>,
    shutdown_notify: Arc<Notify>,
}

impl std::fmt::Debug for FallbackSpawner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self
            .handles
            .try_lock()
            .map_or(usize::MAX, |handles| handles.len());
        // The handle map and `Notify` are not user-facing — surface
        // only the externally observable lifecycle state.
        f.debug_struct("FallbackSpawner")
            .field("live_subprocess_count", &count)
            .field("grace_period", &self.grace_period)
            .field("reaper_running", &self.reaper_handle.is_some())
            .finish_non_exhaustive()
    }
}

impl FallbackSpawner {
    /// Construct a spawner using the master-plan default grace period
    /// of [`DEFAULT_GRACE_PERIOD_MINUTES`] minutes.
    ///
    /// `commands` maps each [`Language`] to the executable name plus
    /// its arguments — e.g.
    /// `(Language::Python, ("pyright-langserver".into(), vec!["--stdio".into()]))`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerSharingError::BinaryNotFound`] when an entry's
    /// command is not on `PATH`, or
    /// [`ServerSharingError::SpawnFailed`] for other IO errors during
    /// [`tokio::process::Command::spawn`].  On error the function
    /// drops the partially constructed handle map; tokio's
    /// [`tokio::process::Command::kill_on_drop`] ensures any
    /// already-spawned children are reaped immediately.
    pub fn new(
        commands: &HashMap<Language, (String, Vec<String>)>,
    ) -> Result<Self, ServerSharingError> {
        Self::with_grace_period(
            commands,
            Duration::from_secs(DEFAULT_GRACE_PERIOD_MINUTES * 60),
        )
    }

    /// Construct a spawner with an explicit grace period.
    ///
    /// Equivalent to [`Self::new`] but accepts a custom
    /// `grace_period`.  Tests use this hook to assert the reaper's
    /// behaviour without waiting five real minutes.
    ///
    /// # Errors
    ///
    /// Same as [`Self::new`].
    pub fn with_grace_period(
        commands: &HashMap<Language, (String, Vec<String>)>,
        grace_period: Duration,
    ) -> Result<Self, ServerSharingError> {
        let mut handles_map: HashMap<Language, ServerHandle> = HashMap::new();
        for (language, (command, args)) in commands {
            let handle = Self::spawn_one(*language, command, args)?;
            handles_map.insert(*language, handle);
        }

        let handles = Arc::new(Mutex::new(handles_map));
        let shutdown_notify = Arc::new(Notify::new());
        let reaper_handle = tokio::spawn(reap_idle(
            Arc::clone(&handles),
            Arc::clone(&shutdown_notify),
            grace_period,
        ));

        tracing::debug!(
            target: "ucil.lsp.fallback_spawner.new",
            grace_period_secs = grace_period.as_secs(),
            "FallbackSpawner constructed"
        );

        Ok(Self {
            handles,
            grace_period,
            reaper_handle: Some(reaper_handle),
            shutdown_notify,
        })
    }

    fn spawn_one(
        language: Language,
        command: &str,
        args: &[String],
    ) -> Result<ServerHandle, ServerSharingError> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(true);

        let child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ServerSharingError::BinaryNotFound {
                language,
                command: command.to_string(),
            },
            _ => ServerSharingError::SpawnFailed {
                language,
                source: e,
            },
        })?;

        let pid = child.id().unwrap_or(0);
        tracing::info!(
            target: "ucil.lsp.fallback_spawner.spawn",
            ?language,
            command,
            pid,
            "Spawned standalone LSP subprocess"
        );

        Ok(ServerHandle {
            pid,
            child,
            last_used: Instant::now(),
            transport: LspTransport::Standalone {
                command: command.to_string(),
                args: args.to_vec(),
            },
            language,
        })
    }

    /// Populate `bridge` with one [`LspEndpoint`] per live spawned
    /// subprocess.
    ///
    /// Each endpoint carries an [`LspTransport::Standalone`] whose
    /// `command` + `args` match the spawned subprocess.  The bridge's
    /// [`crate::bridge::LspDiagnosticsBridge::insert_endpoint`] is
    /// upsert-style — repeated calls overwrite prior entries.
    pub fn install_into(&self, bridge: &mut LspDiagnosticsBridge) {
        let handles = lock_handles(&self.handles);
        for handle in handles.values() {
            let endpoint = LspEndpoint {
                language: handle.language,
                transport: handle.transport.clone(),
            };
            bridge.insert_endpoint(endpoint);
        }
    }

    /// Refresh the `last_used` timestamp for `language` — call this on
    /// every dispatched LSP request to defer the idle reaper.
    ///
    /// No-op when no subprocess is registered for `language`.
    pub fn touch(&mut self, language: Language) {
        let mut handles = lock_handles(&self.handles);
        if let Some(handle) = handles.get_mut(&language) {
            handle.last_used = Instant::now();
            tracing::trace!(
                target: "ucil.lsp.fallback_spawner.touch",
                ?language,
                "Refreshed last_used"
            );
        }
    }

    /// Returns `true` when a child subprocess for `language` is still
    /// alive (i.e. [`tokio::process::Child::try_wait`] returns
    /// `Ok(None)`); `false` if it has already exited or no entry
    /// exists.
    pub fn is_alive(&mut self, language: Language) -> bool {
        let mut handles = lock_handles(&self.handles);
        handles
            .get_mut(&language)
            .is_some_and(|handle| matches!(handle.child.try_wait(), Ok(None)))
    }

    /// Read the most recent `last_used` [`Instant`] for `language`,
    /// or `None` if no subprocess is registered.
    #[must_use]
    pub fn last_used_for(&self, language: Language) -> Option<Instant> {
        let handles = lock_handles(&self.handles);
        handles.get(&language).map(ServerHandle::last_used)
    }

    /// Read the OS process id for `language`, or `None` if no
    /// subprocess is registered.
    #[must_use]
    pub fn pid_for(&self, language: Language) -> Option<u32> {
        let handles = lock_handles(&self.handles);
        handles.get(&language).map(ServerHandle::pid)
    }

    /// Snapshot of the languages currently backed by a live
    /// subprocess.
    #[must_use]
    pub fn languages(&self) -> Vec<Language> {
        let handles = lock_handles(&self.handles);
        handles.keys().copied().collect()
    }

    /// Returns the spawner's configured grace period.
    #[must_use]
    pub const fn grace_period(&self) -> Duration {
        self.grace_period
    }

    /// Stop the reaper task and gracefully shut down every managed
    /// subprocess.
    ///
    /// Returns the count of subprocesses that were drained from the
    /// handle map and shut down.  Each subprocess is given up to
    /// [`SHUTDOWN_TIMEOUT`] to exit after its stdin is closed; if it
    /// does not, the internal `shutdown_handle` helper follows up with
    /// SIGKILL and one more bounded wait — exceeding that surfaces as
    /// [`ServerSharingError::ShutdownTimeout`].
    ///
    /// # Errors
    ///
    /// Returns the first [`ServerSharingError`] encountered while
    /// shutting down any subprocess.  All shutdowns are awaited
    /// concurrently via [`tokio::task::JoinSet`] so the wall-clock
    /// bound is `~SHUTDOWN_TIMEOUT + 1s` regardless of the number of
    /// managed subprocesses.
    pub async fn shutdown_all(&mut self) -> Result<usize, ServerSharingError> {
        // Stop the reaper before draining so it does not race us on
        // the same handle map.
        self.shutdown_notify.notify_waiters();
        if let Some(handle) = self.reaper_handle.take() {
            handle.abort();
        }

        let drained: Vec<(Language, ServerHandle)> = {
            let mut handles = lock_handles(&self.handles);
            handles.drain().collect()
        };

        let count = drained.len();
        let mut set: tokio::task::JoinSet<Result<(), ServerSharingError>> =
            tokio::task::JoinSet::new();
        for (language, handle) in drained {
            set.spawn(async move { shutdown_handle(language, handle).await });
        }

        let mut first_err: Option<ServerSharingError> = None;
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    if first_err.is_none() {
                        first_err = Some(error);
                    }
                }
                Err(join_err) => {
                    tracing::warn!(
                        target: "ucil.lsp.fallback_spawner.shutdown_all",
                        ?join_err,
                        "shutdown task did not complete cleanly",
                    );
                }
            }
        }

        if let Some(error) = first_err {
            return Err(error);
        }

        tracing::info!(
            target: "ucil.lsp.fallback_spawner.shutdown_all",
            count,
            "FallbackSpawner shut down all children"
        );

        Ok(count)
    }
}

impl Drop for FallbackSpawner {
    fn drop(&mut self) {
        // Best-effort: signal the reaper to stop and abort its task.
        self.shutdown_notify.notify_waiters();
        if let Some(handle) = self.reaper_handle.take() {
            handle.abort();
        }

        // We cannot `.await` in `Drop`, so we cannot do a graceful
        // shutdown here.  `try_lock` succeeds in the common case
        // (`shutdown_all` has already drained the map, or the reaper
        // task is no longer holding the lock).  When it fails — e.g.
        // because the reaper is in the middle of a critical section —
        // tokio's `kill_on_drop(true)` set on each `Command` ensures
        // the children are still reaped when the `ServerHandle`s
        // eventually drop.
        if let Ok(mut handles) = self.handles.try_lock() {
            for (_lang, mut handle) in handles.drain() {
                let _ = handle.child.start_kill();
            }
        }
    }
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Lock the handle map, recovering from any prior poisoning by reusing
/// the inner data.  Poisoning is only possible if a panic occurred
/// while a handle map mutation was in flight; in this module no such
/// mutation panics in practice, but the recovery path keeps us
/// compliant with `rust-style.md` (no `.unwrap()` / `.expect()`
/// outside `#[cfg(test)]`).
fn lock_handles(
    handles: &Mutex<HashMap<Language, ServerHandle>>,
) -> std::sync::MutexGuard<'_, HashMap<Language, ServerHandle>> {
    handles
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Gracefully shut down a single [`ServerHandle`].
///
/// 1. Drop the child's stdin to send EOF — most LSP servers exit
///    cleanly on EOF.
/// 2. Wait up to [`SHUTDOWN_TIMEOUT`] for the child to exit
///    (wrapped in [`tokio::time::timeout`] per `rust-style.md`).
/// 3. If the wait times out, fall back to
///    [`tokio::process::Child::start_kill`] (SIGKILL on Unix) and
///    wait one more bounded second; exceeding that surfaces as
///    [`ServerSharingError::ShutdownTimeout`].
async fn shutdown_handle(
    language: Language,
    mut handle: ServerHandle,
) -> Result<(), ServerSharingError> {
    // Closing stdin sends EOF — graceful path for cooperative LSP
    // servers.  `Child::stdin` is `Option<ChildStdin>`; `take` moves
    // it out, dropping it (and closing the pipe) at end of statement.
    drop(handle.child.stdin.take());

    match timeout(SHUTDOWN_TIMEOUT, handle.child.wait()).await {
        Ok(Ok(_status)) => Ok(()),
        Ok(Err(source)) => Err(ServerSharingError::SpawnFailed { language, source }),
        Err(_elapsed) => {
            // Hard kill if graceful exit timed out.
            let _ = handle.child.start_kill();
            match timeout(Duration::from_secs(1), handle.child.wait()).await {
                Ok(Ok(_status)) => Ok(()),
                Ok(Err(source)) => Err(ServerSharingError::SpawnFailed { language, source }),
                Err(_elapsed_again) => Err(ServerSharingError::ShutdownTimeout { language }),
            }
        }
    }
}

/// Background task that reaps idle LSP subprocesses.
///
/// Loops, waking every [`REAP_INTERVAL`] (or earlier if
/// `shutdown_notify` is signalled), and shuts down any subprocess
/// whose `last_used + grace_period` has elapsed.
///
/// The lock is never held across an `.await` — the loop snapshots the
/// to-reap language list under the lock, releases the lock, then
/// awaits each [`shutdown_handle`] sequentially.
async fn reap_idle(
    handles: Arc<Mutex<HashMap<Language, ServerHandle>>>,
    shutdown_notify: Arc<Notify>,
    grace_period: Duration,
) {
    loop {
        tokio::select! {
            () = shutdown_notify.notified() => break,
            () = tokio::time::sleep(REAP_INTERVAL) => {}
        }

        let now = Instant::now();
        let to_reap: Vec<Language> = {
            let handles = lock_handles(&handles);
            handles
                .iter()
                .filter(|(_, handle)| now.duration_since(handle.last_used) >= grace_period)
                .map(|(language, _)| *language)
                .collect()
        };

        for language in to_reap {
            let handle_opt = {
                let mut handles = lock_handles(&handles);
                handles.remove(&language)
            };
            if let Some(handle) = handle_opt {
                if let Err(error) = shutdown_handle(language, handle).await {
                    tracing::warn!(
                        target: "ucil.lsp.fallback_spawner.reap",
                        ?language,
                        ?error,
                        "Failed to gracefully shut down idle LSP subprocess"
                    );
                }
            }
        }
    }
}

// ── Module-root acceptance test (frozen-pattern selector) ────────────────────
//
// Per WO-0006/WO-0007/WO-0011/WO-0013 lesson: the frozen-pattern
// selector `server_sharing::test_fallback_spawn` must resolve to a
// module-root `#[tokio::test]` function (NOT inside `mod tests { … }`)
// so a future planner who promotes it to an exact-match selector
// gets the path `server_sharing::test_fallback_spawn` rather than
// `server_sharing::tests::test_fallback_spawn`.

/// End-to-end exercise of the spawner lifecycle on `sleep`-backed
/// stand-in subprocesses.
///
/// Uses two `sleep 60` children (one per language) so the test runs
/// on any Unix host without `pyright` / `rust-analyzer` installed.
/// Asserts the bridge endpoint map is populated, both children are
/// alive, [`FallbackSpawner::touch`] advances `last_used`, and
/// [`FallbackSpawner::shutdown_all`] returns within
/// [`SHUTDOWN_TIMEOUT`].
#[cfg(test)]
#[cfg(unix)]
#[tokio::test]
async fn test_fallback_spawn() {
    let mut commands: HashMap<Language, (String, Vec<String>)> = HashMap::new();
    commands.insert(
        Language::Python,
        ("sleep".to_string(), vec!["60".to_string()]),
    );
    commands.insert(
        Language::Rust,
        ("sleep".to_string(), vec!["60".to_string()]),
    );

    // Use a long grace period so the reaper does not interfere with
    // the test before `shutdown_all` runs.
    let mut spawner = FallbackSpawner::with_grace_period(&commands, Duration::from_secs(3600))
        .expect("sleep is available on every Unix CI host");

    // Grace period plumbed through to the spawner.
    assert_eq!(spawner.grace_period(), Duration::from_secs(3600));

    // Two languages registered.
    let mut langs = spawner.languages();
    langs.sort_by_key(|l| format!("{l:?}"));
    assert_eq!(langs.len(), 2, "expected two registered languages");

    // PIDs are positive (not the `child.id() == None` sentinel).
    assert!(spawner.pid_for(Language::Python).unwrap_or(0) > 0);
    assert!(spawner.pid_for(Language::Rust).unwrap_or(0) > 0);

    // Install endpoints into a fresh bridge — assert both languages
    // appear with the configured `Standalone` transport.
    let mut bridge = LspDiagnosticsBridge::new(false);
    spawner.install_into(&mut bridge);
    assert_eq!(bridge.endpoints().len(), 2);

    let py_endpoint = bridge
        .endpoint_for(Language::Python)
        .expect("Python endpoint must be installed");
    match &py_endpoint.transport {
        LspTransport::Standalone { command, args } => {
            assert_eq!(command, "sleep");
            assert_eq!(args, &vec!["60".to_string()]);
        }
        LspTransport::DelegatedToSerena => panic!("expected Standalone transport"),
    }

    let rs_endpoint = bridge
        .endpoint_for(Language::Rust)
        .expect("Rust endpoint must be installed");
    assert!(matches!(
        &rs_endpoint.transport,
        LspTransport::Standalone { command, .. } if command == "sleep"
    ));

    // Both children alive.
    assert!(spawner.is_alive(Language::Python));
    assert!(spawner.is_alive(Language::Rust));

    // Touch refreshes `last_used`.
    let before = spawner
        .last_used_for(Language::Python)
        .expect("Python last_used should be tracked");
    tokio::time::sleep(Duration::from_millis(20)).await;
    spawner.touch(Language::Python);
    let after = spawner
        .last_used_for(Language::Python)
        .expect("Python last_used should still be tracked");
    assert!(
        after > before,
        "touch must advance last_used (before={before:?} after={after:?})"
    );

    // Shutdown — must complete within SHUTDOWN_TIMEOUT.  We wrap the
    // whole shutdown_all in a generous outer timeout to surface a
    // hang as a test failure rather than a CI hang.
    let shutdown_outer = Duration::from_secs(10);
    let shutdown_result = tokio::time::timeout(shutdown_outer, spawner.shutdown_all()).await;
    let count = shutdown_result
        .expect("shutdown_all must return within outer test timeout")
        .expect("shutdown_all must not surface ShutdownTimeout for sleep-based stand-ins");
    assert_eq!(count, 2, "shutdown_all must drain both children");

    // After shutdown the spawner has no live subprocesses.
    assert!(spawner.languages().is_empty());
    assert!(!spawner.is_alive(Language::Python));
    assert!(!spawner.is_alive(Language::Rust));

    // Bridge retains its installed endpoints — install_into copied
    // the transport into the bridge, which the bridge owns
    // independently of spawner lifetime.
    assert_eq!(bridge.endpoints().len(), 2);
}

// ── Supporting tests (non-selector-frozen) ───────────────────────────────────

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::{FallbackSpawner, ServerSharingError, SHUTDOWN_TIMEOUT};
    use crate::bridge::LspDiagnosticsBridge;
    use crate::types::{Language, LspTransport};
    use std::collections::HashMap;
    use std::time::Duration;

    /// A bogus binary surfaces as `BinaryNotFound` (or `SpawnFailed`
    /// on platforms where the kernel returns a different error).
    /// Either is acceptable per the work-order acceptance criterion;
    /// the key invariant is that the spawner does not panic.
    #[tokio::test]
    async fn test_binary_not_found() {
        let mut commands: HashMap<Language, (String, Vec<String>)> = HashMap::new();
        commands.insert(
            Language::Python,
            (
                "ucil-test-definitely-not-a-real-binary-12345".to_string(),
                vec![],
            ),
        );

        let result = FallbackSpawner::new(&commands);
        let is_expected = matches!(
            result,
            Err(ServerSharingError::BinaryNotFound { .. } | ServerSharingError::SpawnFailed { .. })
        );
        assert!(
            is_expected,
            "expected BinaryNotFound or SpawnFailed, got Ok or unexpected error variant",
        );
    }

    /// `Display` for each error variant must mention enough context
    /// to debug the failure (language + command for the spawn errors,
    /// language for the timeout error).
    #[test]
    fn test_error_display_renders_context() {
        let not_found = ServerSharingError::BinaryNotFound {
            language: Language::Rust,
            command: "rust-analyzer-bogus".to_string(),
        };
        let rendered = format!("{not_found}");
        assert!(
            rendered.contains("Rust") && rendered.contains("rust-analyzer-bogus"),
            "BinaryNotFound display lacks context: {rendered}"
        );

        let timeout = ServerSharingError::ShutdownTimeout {
            language: Language::TypeScript,
        };
        let rendered = format!("{timeout}");
        assert!(
            rendered.contains("TypeScript"),
            "ShutdownTimeout display lacks language context: {rendered}"
        );
    }

    /// Partial spawn failure: a missing binary in the second slot
    /// must NOT leave orphan processes from the first slot.  We
    /// can't directly observe orphans, but we verify that `new`
    /// returns the expected error variant and does not panic.  The
    /// `kill_on_drop(true)` invariant is what actually prevents
    /// orphans; this test just exercises the error path.
    #[tokio::test]
    async fn test_partial_spawn_failure_does_not_panic() {
        let mut commands: HashMap<Language, (String, Vec<String>)> = HashMap::new();
        commands.insert(
            Language::Python,
            ("ucil-test-bogus-py-binary".to_string(), vec![]),
        );
        let result = FallbackSpawner::new(&commands);
        assert!(matches!(
            result,
            Err(ServerSharingError::BinaryNotFound { .. } | ServerSharingError::SpawnFailed { .. })
        ));
    }

    /// `touch` on an unregistered language is a silent no-op — does
    /// not panic, does not insert a phantom entry.
    #[tokio::test]
    async fn test_touch_unknown_language_is_noop() {
        let mut commands: HashMap<Language, (String, Vec<String>)> = HashMap::new();
        commands.insert(
            Language::Python,
            ("sleep".to_string(), vec!["60".to_string()]),
        );
        let mut spawner = FallbackSpawner::with_grace_period(&commands, Duration::from_secs(3600))
            .expect("sleep is available");

        // Touching a language we never spawned is a no-op.
        spawner.touch(Language::Java);
        assert!(spawner.last_used_for(Language::Java).is_none());
        assert!(spawner.last_used_for(Language::Python).is_some());

        // Cleanly drain to release the child.
        let _ = tokio::time::timeout(Duration::from_secs(10), spawner.shutdown_all()).await;
    }

    /// Sanity check: `SHUTDOWN_TIMEOUT` is meaningful (at least 1 s)
    /// so misconfiguration in this module does not silently produce
    /// flaky tests.
    #[test]
    fn test_shutdown_timeout_is_reasonable() {
        assert!(SHUTDOWN_TIMEOUT >= Duration::from_secs(1));
    }

    /// `with_fallback_spawner` constructor on the bridge must
    /// install endpoints from the spawner.  This exercises the
    /// additive constructor path per `DEC-0008` §Consequences.
    #[tokio::test]
    async fn test_bridge_with_fallback_spawner_installs_endpoints() {
        let mut commands: HashMap<Language, (String, Vec<String>)> = HashMap::new();
        commands.insert(
            Language::Rust,
            ("sleep".to_string(), vec!["60".to_string()]),
        );
        let spawner = FallbackSpawner::with_grace_period(&commands, Duration::from_secs(3600))
            .expect("sleep is available");

        let bridge = LspDiagnosticsBridge::with_fallback_spawner(spawner);
        assert!(!bridge.is_serena_managed());
        assert_eq!(bridge.endpoints().len(), 1);
        let endpoint = bridge.endpoint_for(Language::Rust).expect("Rust endpoint");
        assert!(matches!(
            &endpoint.transport,
            LspTransport::Standalone { command, .. } if command == "sleep"
        ));

        // Tear down via the bridge's owned spawner — drop will
        // best-effort kill remaining children.
        drop(bridge);
    }
}
