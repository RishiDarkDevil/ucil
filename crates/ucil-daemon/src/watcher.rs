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
