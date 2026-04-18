//! UCIL daemon file watcher — two detection paths for source-tree
//! changes.
//!
//! Master-plan §18 Phase 1 Week 3 line 1741 specifies the `ucil-daemon`
//! file watcher behind feature `P1-W3-F02`: editor/filesystem writes
//! must be collapsed through a 100 ms debounce window, while agent edits
//! performed by Claude Code / Cursor / Aider arrive via a `PostToolUse`
//! hook and SHOULD bypass the debouncer entirely (master-plan §14 lines
//! 1024-1025 — "two detection paths").
//!
//! This module exposes four typed values and one orchestrator:
//!
//! 1. [`FileEventKind`] — total classification of a file-change event,
//!    covering `Created` / `Modified` / `Removed` / `Renamed` and an
//!    `Other` catch-all for metadata / ACL tweaks that are irrelevant to
//!    Phase-1 indexing.
//! 2. [`EventSource`] — records which of the two detection paths produced
//!    an event: [`EventSource::NotifyDebounced`] for editor/filesystem
//!    writes, [`EventSource::PostToolUseHook`] for hook-driven agent
//!    edits.
//! 3. [`FileEvent`] — the value delivered through the async mpsc channel.
//!    Carries the `path`, the `kind`, and the `source`. No timestamp —
//!    hot-observation timestamps are a §12.1 concern and out of scope
//!    for `P1-W3-F02`; ordering through the mpsc channel is the only
//!    ordering contract this module commits to.
//! 4. [`WatcherError`] — typed error enum returned by the watcher
//!    constructor and the hook fast-path.
//!
//! The [`FileWatcher`] struct owns a `notify-debouncer-full` debouncer
//! and forwards its debounced output into a
//! `tokio::sync::mpsc::Sender<FileEvent>` via a blocking forwarder task
//! spawned on the tokio runtime. A second, synchronous method
//! ([`FileWatcher::notify_hook_event`]) sends directly through the same
//! sender *without* going through the debouncer — modelling the §14
//! line 1024 fast path.
//!
//! # Examples
//!
//! ```no_run
//! use std::path::Path;
//! use tokio::sync::mpsc;
//! use ucil_daemon::watcher::{FileWatcher, FileEvent};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let (tx, mut rx) = mpsc::channel::<FileEvent>(128);
//! let _watcher = FileWatcher::new(Path::new("."), tx)?;
//! while let Some(event) = rx.recv().await {
//!     println!("{event:?}");
//! }
//! # Ok(())
//! # }
//! ```

// Public API items share a name prefix with the module ("watcher" →
// "FileWatcher", "FileEvent", …); pedantic clippy would flag that. The
// convention matches `lifecycle::Lifecycle`, `plugin_manager::PluginManager`,
// and `session_manager::SessionManager` in this crate.
#![allow(clippy::module_name_repetitions)]

use std::{
    path::{Path, PathBuf},
    sync::mpsc as std_mpsc,
    time::Duration,
};

use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer_opt, DebounceEventResult, Debouncer, FileIdMap};
use thiserror::Error;
use tokio::{
    sync::mpsc::{self, error::TrySendError},
    task::JoinHandle,
};

/// Debounce window applied to editor / filesystem writes that arrive
/// through `notify-debouncer-full`.
///
/// Fixed at 100 ms per master-plan §18 Phase 1 Week 3 line 1741 and
/// §14 line 1025 — the constant lives at module scope so tests and
/// downstream consumers share a single source of truth. Hook-sourced
/// events ([`EventSource::PostToolUseHook`]) bypass this window entirely.
pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(100);

/// File-count threshold above which [`auto_select_backend`] upgrades from
/// `notify` (inotify / `FSEvents` / `ReadDirectoryChangesW`) to Watchman,
/// when a Watchman binary is present on the `PATH`.
///
/// Fixed at `50_000` per master-plan §2 line 138 — "file-watcher:
/// `notify` crate default / Watchman upgrade for repos `>` 50K files /
/// `PollWatcher` fallback for network mounts". The threshold lives as a
/// named `pub const` so downstream code paths (future `ucil init` CLI
/// wiring — WO-0039 is explicitly scoped out of CLI edits) can reference
/// the same policy value.
pub const WATCHMAN_AUTO_SELECT_THRESHOLD: usize = 50_000;

/// Poll interval used by the [`WatcherBackend::Poll`] backend.
///
/// Fixed at 2 seconds — a conservative trade-off for the network-mount
/// fallback path described in master-plan §2 line 138: short enough
/// that editor writes still feel near-live, long enough that the
/// `PollWatcher` re-scan cost stays bounded on large repos (the
/// `notify` crate's `PollWatcher` default is 30 s which is too slow for
/// agent-assisted workflows). The interval lives as a named `pub const`
/// so tests and downstream consumers share a single source of truth.
pub const POLL_WATCHER_INTERVAL: Duration = Duration::from_secs(2);

/// Which backend drives a [`FileWatcher`].
///
/// Master-plan §2 line 138 specifies three detection strategies for the
/// daemon file watcher:
///
/// 1. [`WatcherBackend::NotifyDebounced`] — the default path, using
///    `notify-debouncer-full` on top of `notify`'s recommended
///    watcher (inotify on Linux, `FSEvents` on macOS,
///    `ReadDirectoryChangesW` on Windows). Collapses rapid editor
///    writes through the [`DEBOUNCE_WINDOW`].
/// 2. [`WatcherBackend::Watchman`] — the optional upgrade for large
///    repositories (`>` [`WATCHMAN_AUTO_SELECT_THRESHOLD`] files) where
///    kernel-level watch descriptors become expensive. Driven by
///    spawning the external `watchman` binary and subscribing to a
///    recursive file-change feed.
/// 3. [`WatcherBackend::Poll`] — the explicit opt-in fallback for
///    network mounts (NFS, SMB, sshfs) where inotify and friends do
///    not deliver change events. Uses `notify::PollWatcher` at
///    [`POLL_WATCHER_INTERVAL`]; NOT auto-selected (callers opt in
///    because detecting network mounts cross-platform is out of
///    scope for this feature — see WO-0039 `scope_out`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WatcherBackend {
    /// `notify-debouncer-full` default — editor / filesystem writes
    /// with a 100 ms debounce window.
    NotifyDebounced,
    /// External `watchman` subscription — auto-selected for repos
    /// above [`WATCHMAN_AUTO_SELECT_THRESHOLD`].
    Watchman,
    /// `notify::PollWatcher` — explicit opt-in for network mounts.
    /// Events flow through the same debouncer pipeline as
    /// [`WatcherBackend::NotifyDebounced`] so consumers observe
    /// [`EventSource::NotifyDebounced`] on the output side (the
    /// detection path matters for CPU cost, not semantics).
    Poll,
}

/// Runtime capability descriptor for an installed Watchman binary.
///
/// Returned by [`detect_watchman`]. Carries only the absolute path of
/// the binary we found on the `PATH`; a future WO may extend this
/// struct with a parsed `watchman --version` probe when that field
/// becomes load-bearing (e.g. when a specific Watchman RPC surface
/// requires a minimum version). Kept deliberately minimal for WO-0039.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchmanCapability {
    /// Absolute path to the `watchman` executable resolved on `PATH`.
    pub binary: PathBuf,
}

/// Probe the current `PATH` for a `watchman` binary.
///
/// Returns `Some(WatchmanCapability { binary })` iff
/// `which::which("watchman")` resolves successfully — i.e. the caller's
/// process environment has a Watchman executable on the search path.
/// Returns `None` otherwise (the common case on fresh developer
/// machines).
///
/// This function does NOT spawn Watchman or issue any RPC; it's a
/// cheap env-var + filesystem probe, suitable for calling at daemon
/// startup and inside [`auto_select_backend`]. No caching: callers
/// that need to invalidate a prior negative should re-probe.
///
/// Tracing span: `ucil.daemon.watcher.detect_watchman` per master-plan
/// §15.2.
#[must_use]
#[tracing::instrument(name = "ucil.daemon.watcher.detect_watchman", level = "debug")]
pub fn detect_watchman() -> Option<WatchmanCapability> {
    match which::which("watchman") {
        Ok(binary) => Some(WatchmanCapability { binary }),
        Err(err) => {
            tracing::debug!(
                target: "ucil.daemon.watcher",
                error = %err,
                "watchman binary not found on PATH"
            );
            None
        }
    }
}

/// Count files under `root`, stopping after at most `cap + 1` entries.
///
/// Uses `walkdir::WalkDir` to walk `root` recursively and returns the
/// number of *file* entries (directories, symlink targets, and I/O
/// errors are filtered out) — but short-circuits once `cap + 1`
/// entries have been seen. That early-exit means the walker's cost
/// stays in `O(cap)` on repositories far larger than `cap`, which is
/// critical for the
/// [`WATCHMAN_AUTO_SELECT_THRESHOLD`]-is-50K check: a full walk of a
/// 10-million-file repo would cost seconds, and we only need to know
/// whether the count is above the threshold.
///
/// The returned value is in `[0, cap + 1]`. Callers should compare
/// with `>` against their threshold (`count > cap` ⇔ "above
/// threshold"); an exact count in the `≤ cap` range is only
/// meaningful for small trees.
///
/// Walker errors (permission denied, broken symlinks) are silently
/// skipped via `filter_map(Result::ok)` — the walker is used here for
/// a coarse size check, not a correctness-critical traversal. A
/// future WO can tighten the error handling if the metric becomes
/// load-bearing.
#[must_use]
pub fn count_files_capped(root: &Path, cap: usize) -> usize {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .take(cap.saturating_add(1))
        .count()
}

/// Pick the best [`WatcherBackend`] for `root` given a file-count
/// `threshold`.
///
/// Returns [`WatcherBackend::Watchman`] iff BOTH conditions hold:
///
/// 1. A `watchman` binary is on the `PATH` ([`detect_watchman`] returns
///    `Some`).
/// 2. The recursive file count under `root` is strictly greater than
///    `threshold` ([`count_files_capped`] returns a value `>` `threshold`).
///
/// Otherwise returns [`WatcherBackend::NotifyDebounced`] — the
/// default, inotify / `FSEvents` / `ReadDirectoryChangesW`-backed path.
///
/// [`WatcherBackend::Poll`] is deliberately never returned from this
/// function: auto-detecting network mounts cross-platform is out of
/// scope for WO-0039 (see `scope_out` in the work-order). Callers
/// that know they are on an NFS / sshfs root should pass
/// [`WatcherBackend::Poll`] directly to
/// [`FileWatcher::new_with_backend`].
///
/// Master-plan §2 line 138 specifies the 50K-file threshold; callers
/// typically pass [`WATCHMAN_AUTO_SELECT_THRESHOLD`] here.
#[must_use]
pub fn auto_select_backend(root: &Path, threshold: usize) -> WatcherBackend {
    if detect_watchman().is_some() && count_files_capped(root, threshold) > threshold {
        WatcherBackend::Watchman
    } else {
        WatcherBackend::NotifyDebounced
    }
}

/// Total classification of a file-change event.
///
/// This enum is *total* over the event kinds we route through the
/// watcher pipeline: `notify::EventKind::{Create, Modify, Remove}` map
/// to the obvious variants, `notify::EventKind::Modify(ModifyKind::Name(_))`
/// maps to [`FileEventKind::Renamed`], and everything else
/// (`Access`, metadata-only `Modify`, `Any`, `Other`) maps to
/// [`FileEventKind::Other`] so the match is exhaustive without
/// silently swallowing unknown kinds. See master-plan §18 Phase 1 Week
/// 3 line 1741 (feature `P1-W3-F02` — editor events classified).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileEventKind {
    /// A new path appeared in the watched tree.
    Created,
    /// An existing path's contents or metadata changed.
    Modified,
    /// A path was removed from the watched tree.
    Removed,
    /// A path was renamed. Covers `notify::EventKind::Modify(ModifyKind::Name(_))`.
    Renamed,
    /// A change we don't classify for Phase-1 indexing — metadata
    /// tweaks, access events, platform-specific kinds. Surfaced
    /// rather than silently dropped so a future consumer can opt in.
    Other,
}

/// Which of the two detection paths produced a [`FileEvent`].
///
/// Master-plan §14 lines 1024-1025 specifies the two-path design:
/// agent edits (Claude Code, Cursor, Aider, …) arrive via a
/// `PostToolUse` hook and bypass the debouncer;
/// editor / human edits arrive through `notify-debouncer-full` with
/// the [`DEBOUNCE_WINDOW`] applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventSource {
    /// Event originated from `notify-debouncer-full` — editor /
    /// filesystem write path, debounced by [`DEBOUNCE_WINDOW`].
    NotifyDebounced,
    /// Event originated from a `PostToolUse` hook invocation — agent
    /// edit fast path that bypasses the debouncer (master-plan §14
    /// line 1024).
    PostToolUseHook,
}

/// A typed file-change event delivered through the watcher's async
/// channel.
///
/// Carries the `path`, the `kind`, and the `source`. No timestamp field
/// is present — hot-observation timestamps are a §12.1 concern and are
/// intentionally out of scope for `P1-W3-F02`. The ordering of
/// `FileEvent`s through the mpsc channel is the only ordering contract
/// this module commits to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvent {
    /// Absolute or root-relative path that changed (whichever notify
    /// surfaced — we do not canonicalise here).
    pub path: PathBuf,
    /// Classification of the change.
    pub kind: FileEventKind,
    /// Detection path that produced the event.
    pub source: EventSource,
}

/// Errors returned by [`FileWatcher`] operations.
///
/// `#[non_exhaustive]` so future variants (backpressure, recovery) can
/// land without a SemVer-breaking change.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WatcherError {
    /// `notify` backend surfaced a filesystem or platform error.
    #[error("notify error: {0}")]
    Notify(#[from] notify::Error),
    /// `notify-debouncer-full` surfaced a debouncer-side error.
    /// The upstream crate emits errors as `Vec<notify::Error>`; we
    /// squash the vector to a `String` at the boundary so the enum
    /// stays simple — callers who need per-error structure should
    /// inspect the forwarder task's trace output.
    #[error("debouncer error: {0}")]
    Debouncer(String),
    /// The async sender's receiver side has been dropped, or the
    /// bounded channel is full and cannot accept another event. Both
    /// surface as "channel closed" for `P1-W3-F02`; backpressure
    /// semantics are a follow-up concern.
    #[error("watcher channel closed")]
    ChannelClosed,
    /// An I/O error occurred outside the `notify` layer (e.g. the
    /// watched root could not be stat'd).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The `watchman` subprocess could not be spawned (e.g. binary
    /// vanished between [`detect_watchman`] and the backend-startup
    /// call). Surfaced only from the [`WatcherBackend::Watchman`]
    /// dispatch path in [`FileWatcher::new_with_backend`]. No
    /// `#[from]` conversion on this variant because
    /// [`WatcherError::Io`] already captures generic
    /// `std::io::Error`; this variant exists so callers can
    /// distinguish "could not run watchman" from "could not stat the
    /// watched root" without inspecting the underlying error string.
    #[error("watchman spawn: {0}")]
    WatchmanSpawn(std::io::Error),
    /// A line emitted by the watchman subscription could not be
    /// decoded as a JSON event. The underlying `serde_json::Error`
    /// carries the offending line for diagnostics. Surfaced through
    /// tracing from the Watchman forwarder task and NOT propagated
    /// through the watcher channel (the channel carries typed
    /// events, not errors) — the variant exists for future
    /// error-channel integrations and so the constructor can bubble
    /// a startup decode error (e.g. from the initial subscribe
    /// acknowledgement) if one occurs synchronously.
    #[error("watchman json decode: {0}")]
    WatchmanDecode(serde_json::Error),
}

/// File-change watcher for a single root directory.
///
/// Wraps one of three backends — [`notify`]'s recommended watcher
/// (default), [`notify::PollWatcher`] (network-mount fallback), or an
/// external `watchman` subprocess (large-repo upgrade) — together
/// with a forwarder task that emits typed [`FileEvent`]s on an async
/// channel. See [`WatcherBackend`] for the selection policy and
/// module-level docs for the two detection paths this struct
/// orchestrates.
pub struct FileWatcher {
    sender: mpsc::Sender<FileEvent>,
    /// Kept alive so dropping the [`FileWatcher`] stops the backend
    /// (debouncer threads via their upstream `Drop` impls; watchman
    /// child via `kill_on_drop`).
    _backend: BackendHandle,
    /// Forwarder task — joined implicitly when the watcher drops and
    /// the backend-side sender closes.
    _forwarder: JoinHandle<()>,
}

/// Private handle union for the three concrete backend shapes. Kept
/// private because callers consume events through the shared
/// [`FileEvent`] channel — the exact backend is an implementation
/// detail they do not need to inspect. The variants are held purely
/// for their `Drop` side effects (stop the backend thread / kill the
/// subprocess), so the payloads are never read after construction —
/// `#[allow(dead_code)]` silences the read-never-read lint without
/// dropping the payloads (which would break the lifetime contract).
#[allow(dead_code)]
enum BackendHandle {
    /// `notify-debouncer-full` with `notify::RecommendedWatcher`
    /// underneath (inotify / `FSEvents` / `ReadDirectoryChangesW`).
    NotifyDebounced(Debouncer<notify::RecommendedWatcher, FileIdMap>),
    /// `notify-debouncer-full` with `notify::PollWatcher` underneath —
    /// network-mount fallback that re-scans the tree at
    /// [`POLL_WATCHER_INTERVAL`].
    Poll(Debouncer<notify::PollWatcher, FileIdMap>),
    /// Spawned `watchman` subprocess with `kill_on_drop(true)` — the
    /// child is killed automatically when the [`FileWatcher`] drops.
    Watchman(tokio::process::Child),
}

/// Decoded payload of a single JSONL line emitted by a Watchman
/// subscription. Only the fields we act on are deserialised; unknown
/// keys in the feed are ignored by `serde_json`'s default behaviour.
///
/// Private to the Watchman backend — not part of the public API.
#[derive(Debug, serde::Deserialize)]
struct WatchmanEvent {
    name: String,
    #[serde(default)]
    exists: bool,
    #[serde(default)]
    new: bool,
}

/// Map a `notify::EventKind` to our total [`FileEventKind`].
///
/// Module-private helper — the forwarder task is the only caller.
/// `Access(_)` maps to [`FileEventKind::Other`] because Phase-1 indexing
/// does not care about read-only access events. `Modify(Name(_))` maps
/// to [`FileEventKind::Renamed`] even though it shares the
/// `notify::EventKind::Modify` outer variant with content / metadata
/// modifications.
#[must_use]
#[inline]
pub(crate) const fn map_notify_kind(kind: notify::EventKind) -> FileEventKind {
    use notify::event::{EventKind, ModifyKind};
    match kind {
        EventKind::Create(_) => FileEventKind::Created,
        EventKind::Modify(ModifyKind::Name(_)) => FileEventKind::Renamed,
        EventKind::Modify(_) => FileEventKind::Modified,
        EventKind::Remove(_) => FileEventKind::Removed,
        EventKind::Access(_) | EventKind::Any | EventKind::Other => FileEventKind::Other,
    }
}

/// Spawn the blocking forwarder that drains a debouncer's std-mpsc
/// output and pushes typed [`FileEvent`]s with
/// [`EventSource::NotifyDebounced`] onto the async `sender`. Shared by
/// the `NotifyDebounced` and `Poll` constructors — both debouncer
/// shapes produce the same `DebounceEventResult` stream on the std
/// side, so the forwarder body is identical.
fn spawn_debouncer_forwarder(
    std_rx: std_mpsc::Receiver<DebounceEventResult>,
    sender: mpsc::Sender<FileEvent>,
) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        while let Ok(result) = std_rx.recv() {
            match result {
                Ok(events) => {
                    for ev in events {
                        let kind = map_notify_kind(ev.event.kind);
                        for path in &ev.event.paths {
                            let file_event = FileEvent {
                                path: path.clone(),
                                kind,
                                source: EventSource::NotifyDebounced,
                            };
                            if sender.blocking_send(file_event).is_err() {
                                return;
                            }
                        }
                    }
                }
                Err(errors) => {
                    tracing::warn!(
                        target: "ucil.daemon.watcher",
                        error_count = errors.len(),
                        "debouncer reported errors"
                    );
                }
            }
        }
    })
}

impl FileWatcher {
    /// Start watching `root` recursively with the default
    /// [`WatcherBackend::NotifyDebounced`] backend.
    ///
    /// Thin delegate to [`FileWatcher::new_with_backend`] — the body
    /// calls `Self::new_with_backend(root, sender,
    /// WatcherBackend::NotifyDebounced)` so every WO-0026 call site
    /// remains byte-identical across the WO-0039 refactor.
    ///
    /// Tracing span: `ucil.daemon.watcher.new` (master-plan §15.2) —
    /// kept on the thin wrapper so existing traces stay stable.
    ///
    /// # Errors
    ///
    /// Returns [`WatcherError::Notify`] if the `notify` backend cannot
    /// be constructed or cannot begin watching `root` (e.g. the path
    /// does not exist or is not readable).
    #[tracing::instrument(
        name = "ucil.daemon.watcher.new",
        level = "debug",
        skip(sender),
        fields(root = %root.display())
    )]
    pub fn new(root: &Path, sender: mpsc::Sender<FileEvent>) -> Result<Self, WatcherError> {
        Self::new_with_backend(root, sender, WatcherBackend::NotifyDebounced)
    }

    /// Start watching `root` recursively with an explicit
    /// [`WatcherBackend`].
    ///
    /// Dispatches on `backend`:
    ///
    /// - [`WatcherBackend::NotifyDebounced`] — the default path from
    ///   WO-0026. Events flow through `notify-debouncer-full` with
    ///   [`DEBOUNCE_WINDOW`] applied.
    /// - [`WatcherBackend::Poll`] — uses `notify::PollWatcher` at
    ///   [`POLL_WATCHER_INTERVAL`] underneath the same debouncer
    ///   pipeline, so the emitted [`FileEvent`]s look identical to
    ///   `NotifyDebounced` events from the consumer's POV (same
    ///   `source`, same `kind` mapping). Use this backend only on
    ///   roots where inotify / `FSEvents` do not fire (NFS, SMB, sshfs).
    /// - [`WatcherBackend::Watchman`] — spawns the external `watchman`
    ///   binary via `tokio::process::Command` and subscribes to a
    ///   recursive file-change feed. The subprocess is killed
    ///   automatically when this [`FileWatcher`] drops
    ///   (`kill_on_drop(true)`).
    ///
    /// Tracing span: `ucil.daemon.watcher.new_with_backend`
    /// (master-plan §15.2).
    ///
    /// # Errors
    ///
    /// - [`WatcherError::Notify`] — the selected `notify` backend
    ///   could not be constructed or could not begin watching `root`.
    /// - [`WatcherError::WatchmanSpawn`] — the `watchman` binary
    ///   could not be launched (missing from `PATH`, permission
    ///   denied, etc.) when `backend ==
    ///   WatcherBackend::Watchman`.
    #[tracing::instrument(
        name = "ucil.daemon.watcher.new_with_backend",
        level = "debug",
        skip(sender),
        fields(root = %root.display(), backend = ?backend)
    )]
    pub fn new_with_backend(
        root: &Path,
        sender: mpsc::Sender<FileEvent>,
        backend: WatcherBackend,
    ) -> Result<Self, WatcherError> {
        match backend {
            WatcherBackend::NotifyDebounced => Self::new_notify_debounced(root, sender),
            WatcherBackend::Poll => Self::new_poll(root, sender),
            WatcherBackend::Watchman => Self::new_watchman(root, sender),
        }
    }

    /// Construct the default recommended-watcher + debouncer backend.
    fn new_notify_debounced(
        root: &Path,
        sender: mpsc::Sender<FileEvent>,
    ) -> Result<Self, WatcherError> {
        let (std_tx, std_rx) = std_mpsc::channel::<DebounceEventResult>();
        let mut debouncer = new_debouncer_opt::<_, notify::RecommendedWatcher, FileIdMap>(
            DEBOUNCE_WINDOW,
            None,
            move |result| {
                let _ = std_tx.send(result);
            },
            FileIdMap::new(),
            notify::Config::default(),
        )
        .map_err(WatcherError::Notify)?;

        debouncer
            .watcher()
            .watch(root, RecursiveMode::Recursive)
            .map_err(WatcherError::Notify)?;

        let forwarder = spawn_debouncer_forwarder(std_rx, sender.clone());

        Ok(Self {
            sender,
            _backend: BackendHandle::NotifyDebounced(debouncer),
            _forwarder: forwarder,
        })
    }

    /// Construct the [`notify::PollWatcher`] + debouncer backend for
    /// network mounts.
    fn new_poll(root: &Path, sender: mpsc::Sender<FileEvent>) -> Result<Self, WatcherError> {
        let (std_tx, std_rx) = std_mpsc::channel::<DebounceEventResult>();
        let config = notify::Config::default().with_poll_interval(POLL_WATCHER_INTERVAL);
        let mut debouncer = new_debouncer_opt::<_, notify::PollWatcher, FileIdMap>(
            DEBOUNCE_WINDOW,
            None,
            move |result| {
                let _ = std_tx.send(result);
            },
            FileIdMap::new(),
            config,
        )
        .map_err(WatcherError::Notify)?;

        debouncer
            .watcher()
            .watch(root, RecursiveMode::Recursive)
            .map_err(WatcherError::Notify)?;

        let forwarder = spawn_debouncer_forwarder(std_rx, sender.clone());

        Ok(Self {
            sender,
            _backend: BackendHandle::Poll(debouncer),
            _forwarder: forwarder,
        })
    }

    /// Construct the external `watchman` subprocess backend.
    ///
    /// Spawns `watchman` with `kill_on_drop(true)` so the child
    /// process terminates when this [`FileWatcher`] drops. Writes a
    /// JSON `subscribe` command to the subprocess' stdin, then spawns
    /// an async forwarder task that reads JSONL events from stdout
    /// and emits [`FileEvent`]s on `sender`. Events carry
    /// [`EventSource::NotifyDebounced`] — Watchman is a detection
    /// backend swap, not a semantic channel change.
    fn new_watchman(root: &Path, sender: mpsc::Sender<FileEvent>) -> Result<Self, WatcherError> {
        use std::process::Stdio;
        let capability = detect_watchman().ok_or_else(|| {
            WatcherError::WatchmanSpawn(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "watchman binary not found on PATH",
            ))
        })?;

        let subscribe_cmd = serde_json::json!([
            "subscribe",
            root.display().to_string(),
            "ucil-daemon",
            {
                "expression": ["allof", ["type", "f"]],
                "fields": ["name", "type", "exists", "new"]
            }
        ])
        .to_string();

        let mut child = tokio::process::Command::new(&capability.binary)
            .arg("--no-spawner")
            .arg("--server-encoding=json")
            .arg("--json-command")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(WatcherError::WatchmanSpawn)?;

        let stdin = child.stdin.take().ok_or_else(|| {
            WatcherError::WatchmanSpawn(std::io::Error::other(
                "watchman child exposed no stdin handle",
            ))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            WatcherError::WatchmanSpawn(std::io::Error::other(
                "watchman child exposed no stdout handle",
            ))
        })?;

        let forwarder_tx = sender.clone();
        let root_pathbuf = root.to_path_buf();
        let forwarder = tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

            let mut stdin = stdin;
            if stdin.write_all(subscribe_cmd.as_bytes()).await.is_err() {
                return;
            }
            if stdin.write_all(b"\n").await.is_err() {
                return;
            }
            drop(stdin);

            let mut lines = BufReader::new(stdout).lines();
            loop {
                let Ok(Some(line)) = lines.next_line().await else {
                    break;
                };
                let ev: WatchmanEvent = match serde_json::from_str(&line) {
                    Ok(e) => e,
                    Err(err) => {
                        tracing::debug!(
                            target: "ucil.daemon.watcher",
                            error = %err,
                            line = %line,
                            "watchman line did not decode as event; skipping"
                        );
                        continue;
                    }
                };
                let path = root_pathbuf.join(&ev.name);
                let kind = if !ev.exists {
                    FileEventKind::Removed
                } else if ev.new {
                    FileEventKind::Created
                } else {
                    FileEventKind::Modified
                };
                let file_event = FileEvent {
                    path,
                    kind,
                    source: EventSource::NotifyDebounced,
                };
                if forwarder_tx.send(file_event).await.is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            sender,
            _backend: BackendHandle::Watchman(child),
            _forwarder: forwarder,
        })
    }

    /// Emit a file event produced by a `PostToolUse` hook invocation.
    ///
    /// Bypasses the debouncer entirely (master-plan §14 line 1024) and
    /// stamps the event with [`EventSource::PostToolUseHook`]. Sends
    /// synchronously via `Sender::try_send` so the method is callable
    /// from any context (including the daemon's hook-receiving sync
    /// path) and cannot starve the tokio runtime.
    ///
    /// Tracing span: `ucil.daemon.watcher.hook` (master-plan §15.2).
    ///
    /// # Errors
    ///
    /// Returns [`WatcherError::ChannelClosed`] when the receiver half of
    /// the async channel has been dropped *or* the bounded channel is
    /// currently full. The `Full`→`ChannelClosed` collapse is
    /// deliberate for `P1-W3-F02`; backpressure is a follow-up WO.
    #[tracing::instrument(
        name = "ucil.daemon.watcher.hook",
        level = "debug",
        skip(self),
        fields(path = %path.display())
    )]
    pub fn notify_hook_event(
        &self,
        path: PathBuf,
        kind: FileEventKind,
    ) -> Result<(), WatcherError> {
        let event = FileEvent {
            path,
            kind,
            source: EventSource::PostToolUseHook,
        };
        match self.sender.try_send(event) {
            Ok(()) => Ok(()),
            Err(TrySendError::Closed(_) | TrySendError::Full(_)) => {
                Err(WatcherError::ChannelClosed)
            }
        }
    }
}

// ── Module-root acceptance tests (F02 oracle) ────────────────────────────────
//
// The tests below live at module root (NOT inside a `mod tests { … }`
// block) per DEC-0005: the frozen `watcher::` selector in
// `feature-list.json` is a module prefix, and keeping module-root
// placement means `watcher::test_*` rather than
// `watcher::tests::test_*`. Mirrors the WO-0023 `call_hierarchy::test_*`
// precedent.

#[cfg(test)]
#[test]
fn test_event_kind_mapping_covers_create_modify_remove() {
    use notify::event::{AccessKind, CreateKind, EventKind, ModifyKind, RemoveKind, RenameMode};

    assert_eq!(
        map_notify_kind(EventKind::Create(CreateKind::File)),
        FileEventKind::Created,
    );
    assert_eq!(
        map_notify_kind(EventKind::Create(CreateKind::Folder)),
        FileEventKind::Created,
    );
    assert_eq!(
        map_notify_kind(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Content
        ))),
        FileEventKind::Modified,
    );
    assert_eq!(
        map_notify_kind(EventKind::Modify(ModifyKind::Name(RenameMode::Any))),
        FileEventKind::Renamed,
    );
    assert_eq!(
        map_notify_kind(EventKind::Remove(RemoveKind::File)),
        FileEventKind::Removed,
    );
    assert_eq!(
        map_notify_kind(EventKind::Access(AccessKind::Any)),
        FileEventKind::Other,
    );
    assert_eq!(map_notify_kind(EventKind::Any), FileEventKind::Other);
    assert_eq!(map_notify_kind(EventKind::Other), FileEventKind::Other);
}

/// Drain the receiver until a timeout elapses, collecting every event that
/// arrives. Each individual `recv` await is bounded by `per_recv` so the
/// test fails fast on a deadlocked channel rather than waiting forever.
#[cfg(test)]
async fn drain_until_quiet(
    rx: &mut mpsc::Receiver<FileEvent>,
    overall: Duration,
    per_recv: Duration,
) -> Vec<FileEvent> {
    let start = std::time::Instant::now();
    let mut events = Vec::new();
    while start.elapsed() < overall {
        match tokio::time::timeout(per_recv, rx.recv()).await {
            Ok(Some(ev)) => events.push(ev),
            // `Ok(None)` = channel closed; `Err(_)` = per-recv timeout
            // elapsed with nothing pending. Either is a signal to stop
            // draining — the stream is quiet.
            Ok(None) | Err(_) => break,
        }
    }
    events
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_notify_emits_event_after_debounce() {
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    let (tx, mut rx) = mpsc::channel::<FileEvent>(32);

    let _watcher = FileWatcher::new(tempdir.path(), tx).expect("create watcher");

    // Give notify a brief moment to register its kernel watch before we
    // poke the filesystem. 50 ms is short vs. the 2 s receive budget and
    // substantially longer than inotify's registration latency.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let path = tempdir.path().join("hello.txt");
    std::fs::write(&path, b"hi").expect("write test file");

    // Debounce is 100 ms; budget 2 s to tolerate CI jitter.
    let ev = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for watcher event")
        .expect("channel closed before event arrived");

    assert_eq!(ev.source, EventSource::NotifyDebounced);
    // Path classification can be `Created` or `Modified` depending on
    // platform — accept either; the F02 contract is "editor event
    // classified", not a specific kind.
    assert!(
        matches!(
            ev.kind,
            FileEventKind::Created | FileEventKind::Modified | FileEventKind::Other
        ),
        "unexpected kind for write event: {:?}",
        ev.kind
    );
    // Path must match (modulo canonicalisation — on macOS tempdir may
    // resolve through `/private/var/...`).
    assert!(
        ev.path.ends_with("hello.txt"),
        "expected path to end with hello.txt, got {}",
        ev.path.display()
    );
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_notify_debounces_editor_writes() {
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    let (tx, mut rx) = mpsc::channel::<FileEvent>(64);

    let _watcher = FileWatcher::new(tempdir.path(), tx).expect("create watcher");

    // Let notify register before we start hammering the file.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let path = tempdir.path().join("editor.txt");
    for i in 0..5 {
        std::fs::write(&path, format!("line {i}\n").as_bytes()).expect("write");
        // 5 ms between writes — well under the 100 ms debounce window.
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // After the last write, wait long enough for the debouncer to
    // flush, then keep draining until 500 ms of quiet passes.
    let events = drain_until_quiet(
        &mut rx,
        Duration::from_millis(1_500),
        Duration::from_millis(500),
    )
    .await;

    // The debouncer MUST collapse 5 rapid writes into a small batch.
    // Real inotify / FSEvents may split into 1–3 events depending on
    // kernel + filesystem; the F02 contract is `received_count <= 3`
    // from 5 writes — strictly less than the raw event count.
    assert!(
        events.len() <= 3,
        "expected <= 3 debounced events from 5 rapid writes, got {}: {:?}",
        events.len(),
        events
    );
    assert!(
        !events.is_empty(),
        "debouncer must emit at least one event for 5 rapid writes"
    );
    // Every event must carry the NotifyDebounced source.
    for ev in &events {
        assert_eq!(ev.source, EventSource::NotifyDebounced);
    }
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_post_tool_use_hook_bypasses_debounce() {
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    let (tx, mut rx) = mpsc::channel::<FileEvent>(32);

    let watcher = FileWatcher::new(tempdir.path(), tx).expect("create watcher");

    // Fire 3 hook events back-to-back — deliberately with NO sleep
    // between calls to probe that the call-site is not gated by the
    // debouncer's 100 ms window.
    let paths: Vec<PathBuf> = (0..3)
        .map(|i| tempdir.path().join(format!("hook-{i}.rs")))
        .collect();
    let start = std::time::Instant::now();
    for p in &paths {
        watcher
            .notify_hook_event(p.clone(), FileEventKind::Modified)
            .expect("hook event send");
    }

    // All 3 must arrive within the debounce window (100 ms) — if the
    // events were routed through the debouncer we'd see at most one
    // batch after ~100 ms of timer tick.
    let mut received = Vec::new();
    for _ in 0..3 {
        let ev = tokio::time::timeout(DEBOUNCE_WINDOW, rx.recv())
            .await
            .expect("hook event did not arrive within DEBOUNCE_WINDOW — did it bypass?")
            .expect("channel closed before hook event arrived");
        received.push(ev);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < DEBOUNCE_WINDOW,
        "3 hook events took {elapsed:?} to arrive — must be < {DEBOUNCE_WINDOW:?}"
    );

    assert_eq!(received.len(), 3);
    for (got, expected) in received.iter().zip(paths.iter()) {
        assert_eq!(got.source, EventSource::PostToolUseHook);
        assert_eq!(got.kind, FileEventKind::Modified);
        assert_eq!(&got.path, expected);
    }
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_hook_event_source_is_distinct() {
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    let (tx, mut rx) = mpsc::channel::<FileEvent>(32);

    let watcher = FileWatcher::new(tempdir.path(), tx).expect("create watcher");

    // Let notify register before writing.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 1. Notify path — create a real file, observe a debounced event.
    let notify_path = tempdir.path().join("from-editor.txt");
    std::fs::write(&notify_path, b"edit").expect("write");

    let notify_event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out on notify event")
        .expect("channel closed before notify event");
    assert_eq!(
        notify_event.source,
        EventSource::NotifyDebounced,
        "filesystem write must carry NotifyDebounced source"
    );

    // Drain any additional debounced events from the write so the hook
    // event below is the next thing the channel delivers.
    let _residue = drain_until_quiet(
        &mut rx,
        Duration::from_millis(300),
        Duration::from_millis(150),
    )
    .await;

    // 2. Hook path — same file path, but via `notify_hook_event`.
    watcher
        .notify_hook_event(notify_path.clone(), FileEventKind::Modified)
        .expect("hook send");
    let hook_event = tokio::time::timeout(DEBOUNCE_WINDOW, rx.recv())
        .await
        .expect("timed out on hook event")
        .expect("channel closed before hook event");
    assert_eq!(
        hook_event.source,
        EventSource::PostToolUseHook,
        "hook-sourced event must carry PostToolUseHook source"
    );

    // The source field is the distinguishing observable between the
    // two detection paths for the same path — §14 lines 1024-1025.
    assert_ne!(notify_event.source, hook_event.source);
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_watcher_shutdown_is_clean() {
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    let (tx, mut rx) = mpsc::channel::<FileEvent>(32);

    {
        let _watcher = FileWatcher::new(tempdir.path(), tx).expect("create watcher");
        // Let notify register.
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Watcher drops here.
    }

    // After the watcher drops, its upstream `notify-debouncer-full`
    // backend stops the event thread (via its own `Drop` impl). Give
    // the runtime a moment to quiesce, then write to the file.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let path = tempdir.path().join("after-drop.txt");
    std::fs::write(&path, b"post-shutdown").expect("write");

    // Shutdown is "clean" if EITHER:
    //   (a) the receiver returns `None` (sender side was the watcher's
    //       clone and the forwarder task has terminated), OR
    //   (b) no new event is observed within 500 ms (the watcher is
    //       quiescent; no phantom forwards).
    // Both outcomes are valid — document the disjunction so a future
    // platform change that tips the balance one way doesn't require a
    // test rewrite. Any `Ok(Some(_))` means the forwarder is still
    // alive after drop — a real regression.
    let outcome = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
    if let Ok(Some(ev)) = outcome {
        panic!(
            "received unexpected event after watcher drop: {ev:?} — \
             debouncer or forwarder is still alive"
        );
    }
    // Reaching here means either:
    //   - `Ok(None)` — channel closed (ideal), OR
    //   - `Err(_)`   — 500 ms quiet (acceptable).
}

// ── WO-0039 F03 acceptance tests ─────────────────────────────────────────────
//
// Watchman detection + backend selection tests. Kept at module root per
// DEC-0005 so the frozen F03 selector `watcher::test_watchman_detection`
// resolves without a `tests::` prefix.
//
// The `PATH`-manipulating tests serialise through the crate-scoped
// `test_support::ENV_GUARD` so they cannot interleave with each other
// *or* with other modules' tests that spawn subprocesses via PATH
// lookup (e.g. `session_manager::tests::*` spawning `git` via
// `tokio::process::Command::new("git")`). Under `cargo test` all
// `#[test]`s in this crate share one process, so a module-local mutex
// would fence watcher-vs-watcher only and still lose to concurrent
// `git` spawns during the blank-PATH window. See DEC-0011.

#[cfg(test)]
use crate::test_support::PathRestoreGuard as RestorePath;

/// Create a fake executable `watchman` shim at `<dir>/watchman` with
/// mode `0o755`. File contents are an empty-shebang shell script; the
/// shim is never executed by WO-0039 tests (only located via
/// `which::which`), so any readable+executable file suffices.
#[cfg(all(test, unix))]
fn create_watchman_shim(dir: &Path) -> PathBuf {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let shim_path = dir.join("watchman");
    let mut file = std::fs::File::create(&shim_path).expect("create watchman shim");
    writeln!(file, "#!/bin/sh").expect("write shebang");
    writeln!(file, "exit 0").expect("write body");
    drop(file);
    let mut perms = std::fs::metadata(&shim_path)
        .expect("stat shim")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&shim_path, perms).expect("chmod shim");
    shim_path
}

/// F03 frozen acceptance selector: `watcher::test_watchman_detection`.
///
/// Two-phase probe on a tempdir: (1) empty `PATH` → `detect_watchman`
/// returns `None`; (2) `PATH` pointing at a tempdir containing a
/// `watchman` shim with mode `0o755` → `detect_watchman` returns
/// `Some(WatchmanCapability { binary })` whose `binary` resolves to
/// the shim path. The original `PATH` is restored on drop via
/// [`RestorePath`] so other tests running concurrently (under the
/// env-guard mutex) observe an unchanged environment.
#[cfg(all(test, unix))]
#[test]
fn test_watchman_detection() {
    let _guard = RestorePath::new();
    let empty = tempfile::TempDir::new().expect("create empty tempdir");
    std::env::set_var("PATH", empty.path());
    assert!(
        detect_watchman().is_none(),
        "watchman should not be found on an empty PATH"
    );

    let shim_dir = tempfile::TempDir::new().expect("create shim tempdir");
    let shim_path = create_watchman_shim(shim_dir.path());
    std::env::set_var("PATH", shim_dir.path());

    let capability = detect_watchman().expect("watchman should be detected via fake shim");

    // `which` canonicalises the resolved path on some platforms.
    // Compare against the canonical shim path too.
    let expected_canonical = std::fs::canonicalize(&shim_path).expect("canonicalise shim");
    let got_canonical =
        std::fs::canonicalize(&capability.binary).expect("canonicalise detected binary");
    assert_eq!(got_canonical, expected_canonical);
}

#[cfg(test)]
#[test]
fn test_count_files_capped_below_cap() {
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    for i in 0..3 {
        std::fs::File::create(tempdir.path().join(format!("file-{i}.txt")))
            .expect("create test file");
    }
    assert_eq!(count_files_capped(tempdir.path(), 10), 3);
}

#[cfg(test)]
#[test]
fn test_count_files_capped_stops_early() {
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    for i in 0..20 {
        std::fs::File::create(tempdir.path().join(format!("file-{i}.txt")))
            .expect("create test file");
    }
    let count = count_files_capped(tempdir.path(), 5);
    // Walker early-exits after cap + 1 entries — actual value may be
    // 6 or less depending on walkdir traversal order, but MUST NOT be
    // 20 (the true file count) because that would indicate the
    // `take(cap + 1)` short-circuit was not applied.
    assert!(
        count <= 6,
        "expected count_files_capped to early-exit at cap+1=6, got {count}"
    );
}

#[cfg(all(test, unix))]
#[test]
fn test_auto_select_backend_returns_notify_when_watchman_absent() {
    let _guard = RestorePath::new();
    let empty = tempfile::TempDir::new().expect("create empty tempdir");
    std::env::set_var("PATH", empty.path());

    let root = tempfile::TempDir::new().expect("create root tempdir");
    assert_eq!(
        auto_select_backend(root.path(), WATCHMAN_AUTO_SELECT_THRESHOLD),
        WatcherBackend::NotifyDebounced,
        "no watchman on PATH must yield NotifyDebounced regardless of threshold"
    );
}

#[cfg(all(test, unix))]
#[test]
fn test_auto_select_backend_returns_watchman_when_available_and_above_threshold() {
    let _guard = RestorePath::new();
    let root = tempfile::TempDir::new().expect("create root tempdir");
    for i in 0..11 {
        std::fs::File::create(root.path().join(format!("file-{i}.rs"))).expect("create test file");
    }

    let shim_dir = tempfile::TempDir::new().expect("create shim tempdir");
    let _shim = create_watchman_shim(shim_dir.path());
    std::env::set_var("PATH", shim_dir.path());

    assert_eq!(
        auto_select_backend(root.path(), 10),
        WatcherBackend::Watchman,
        "watchman present AND count (11) > threshold (10) must select Watchman"
    );
}

#[cfg(all(test, unix))]
#[test]
fn test_auto_select_backend_returns_notify_when_below_threshold() {
    let _guard = RestorePath::new();
    let root = tempfile::TempDir::new().expect("create root tempdir");
    for i in 0..3 {
        std::fs::File::create(root.path().join(format!("file-{i}.rs"))).expect("create test file");
    }

    let shim_dir = tempfile::TempDir::new().expect("create shim tempdir");
    let _shim = create_watchman_shim(shim_dir.path());
    std::env::set_var("PATH", shim_dir.path());

    assert_eq!(
        auto_select_backend(root.path(), 10),
        WatcherBackend::NotifyDebounced,
        "watchman present but count (3) <= threshold (10) must stay on NotifyDebounced"
    );
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_poll_backend_delivers_events() {
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    let (tx, mut rx) = mpsc::channel::<FileEvent>(32);

    let _watcher = FileWatcher::new_with_backend(tempdir.path(), tx, WatcherBackend::Poll)
        .expect("create poll watcher");

    // Give PollWatcher a moment to perform its initial scan so the
    // subsequent write is observed as a *change* rather than the
    // baseline.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let path = tempdir.path().join("poll-hello.txt");
    std::fs::write(&path, b"poll").expect("write test file");

    // PollWatcher's scan-and-diff emits BOTH a parent-directory mtime
    // change and the new file entry across poll cycles, in no
    // guaranteed order. Collect events until we see one for
    // poll-hello.txt OR the 10 s budget elapses (poll interval is
    // POLL_WATCHER_INTERVAL = 2 s, debouncer adds up to
    // DEBOUNCE_WINDOW = 100 ms on top).
    let start = std::time::Instant::now();
    let deadline = Duration::from_secs(10);
    let mut matched: Option<FileEvent> = None;
    while start.elapsed() < deadline {
        match tokio::time::timeout(deadline.saturating_sub(start.elapsed()), rx.recv()).await {
            Ok(Some(ev)) => {
                if ev.path.ends_with("poll-hello.txt") {
                    matched = Some(ev);
                    break;
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
    let ev = matched.expect("poll backend never delivered event for poll-hello.txt");

    assert_eq!(
        ev.source,
        EventSource::NotifyDebounced,
        "Poll events flow through the same debouncer pipeline and \
         carry EventSource::NotifyDebounced"
    );
    assert!(
        ev.path.ends_with("poll-hello.txt"),
        "expected path to end with poll-hello.txt, got {}",
        ev.path.display()
    );
}
