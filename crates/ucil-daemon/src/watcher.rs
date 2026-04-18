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
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
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
}

/// File-change watcher for a single root directory.
///
/// Wraps a `notify-debouncer-full` instance and a forwarder task that
/// translates its `std::sync::mpsc` output into typed [`FileEvent`]s
/// on an async channel. See module-level docs for the two detection
/// paths this struct orchestrates.
pub struct FileWatcher {
    sender: mpsc::Sender<FileEvent>,
    /// Kept alive so dropping the [`FileWatcher`] stops the notify
    /// backend thread (via the upstream crate's `Drop` impl).
    _debouncer: Debouncer<notify::RecommendedWatcher, FileIdMap>,
    /// Forwarder task — joined implicitly when the watcher drops and
    /// the std-side sender is closed by the debouncer's `Drop`.
    _forwarder: JoinHandle<()>,
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

impl FileWatcher {
    /// Start watching `root` recursively and forward debounced events to
    /// `sender`.
    ///
    /// The `notify-debouncer-full` backend runs on its own OS thread
    /// (as `notify` does by design). This constructor spawns a
    /// `tokio::task::spawn_blocking` forwarder that drains the
    /// debouncer's `std::sync::mpsc` receiver and pushes typed
    /// [`FileEvent`]s (with [`EventSource::NotifyDebounced`]) into the
    /// caller-supplied async `sender`.
    ///
    /// Tracing span: `ucil.daemon.watcher.new` (master-plan §15.2).
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
        // Bridge: the debouncer calls the closure from its own thread.
        // We forward the raw result into a std mpsc and let the
        // spawn_blocking forwarder translate + push onto the async
        // channel. `std::sync::mpsc::channel` is sufficient here — we
        // never need select-style polling on the std side.
        let (std_tx, std_rx) = std_mpsc::channel::<DebounceEventResult>();
        let mut debouncer = new_debouncer(DEBOUNCE_WINDOW, None, move |result| {
            // If the forwarder has dropped, the std-side send fails
            // silently — that's fine because the watcher is shutting
            // down; nothing else to do here.
            let _ = std_tx.send(result);
        })
        .map_err(WatcherError::Notify)?;

        debouncer
            .watcher()
            .watch(root, RecursiveMode::Recursive)
            .map_err(WatcherError::Notify)?;

        // Forwarder: drain the std mpsc from a blocking task so we do
        // not block the tokio runtime. `blocking_send` would block the
        // runtime thread itself, so we use the dedicated blocking pool.
        let forwarder_tx = sender.clone();
        let forwarder = tokio::task::spawn_blocking(move || {
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
                                if forwarder_tx.blocking_send(file_event).is_err() {
                                    // Receiver dropped — stop draining.
                                    return;
                                }
                            }
                        }
                    }
                    Err(errors) => {
                        // Surface debouncer errors through tracing; we do
                        // not convert them into `FileEvent`s because the
                        // mpsc only carries typed events. A follow-up WO
                        // can introduce an error channel if needed.
                        tracing::warn!(
                            target: "ucil.daemon.watcher",
                            error_count = errors.len(),
                            "debouncer reported errors"
                        );
                    }
                }
            }
        });

        Ok(Self {
            sender,
            _debouncer: debouncer,
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
