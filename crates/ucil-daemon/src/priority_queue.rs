//! Priority indexing queue — recency-ordered queue of paths.
//!
//! Master-plan §18 Phase 1 Week 3 line 1745 specifies the second half of
//! `P1-W3-F08` (progressive startup): a "priority indexing queue" that
//! serves **recently-queried files first**. Master-plan §21.2 lines
//! 2196-2204 bind that queue to the 0-2 s startup budget — during Phase 1
//! (0-2 s) of the daemon boot sequence, the MCP server is LIVE and this
//! queue absorbs `tools/call`-driven priority hints so that Phase 2
//! (2 s – 2 min) indexing visits already-relevant paths before walking
//! the rest of the tree.
//!
//! This module exposes two types:
//!
//! 1. [`QueueEntry`] — a `(path, last_queried)` pair whose `Ord` places
//!    the newest [`std::time::Instant`] at the top of a
//!    [`std::collections::BinaryHeap`] (max-heap semantics reversed on
//!    `last_queried`).
//! 2. [`PriorityIndexingQueue`] — a `Mutex<BinaryHeap<QueueEntry>>`
//!    wrapper with `enqueue` / `touch` / `pop` / `peek` / `len` /
//!    `is_empty`. All methods acquire the mutex and recover from
//!    `PoisonError` via
//!    [`std::sync::PoisonError::into_inner`] — the heap invariant is
//!    consistent at every panic point because each mutation is a single
//!    heap insert or pop.
//!
//! `touch(path)` is an alias for `enqueue(path)`: it pushes a fresh
//! [`std::time::Instant`] so the just-queried path outranks every other
//! entry on the next `pop()`. Older entries for the same path remain in
//! the heap and surface in their original order once the fresher entry
//! has been consumed — consistent with the "recently queried first"
//! feature description.
//!
//! # Examples
//!
//! ```
//! use std::path::PathBuf;
//! use ucil_daemon::priority_queue::PriorityIndexingQueue;
//!
//! let queue = PriorityIndexingQueue::new();
//! queue.enqueue(PathBuf::from("src/a.rs"));
//! queue.enqueue(PathBuf::from("src/b.rs"));
//! assert_eq!(queue.len(), 2);
//!
//! // Most recently enqueued pops first.
//! let top = queue.pop().expect("non-empty");
//! assert_eq!(top.path, PathBuf::from("src/b.rs"));
//! ```

// Public API items share a name prefix with the module ("priority_queue"
// → `PriorityIndexingQueue`, `QueueEntry`). Matches the convention
// established in `plugin_manager::PluginManager`, `session_manager::SessionManager`,
// `watcher::FileWatcher`, and `lifecycle::Lifecycle`.
#![allow(clippy::module_name_repetitions)]

use std::{cmp::Ordering, collections::BinaryHeap, path::PathBuf, sync::Mutex, time::Instant};

/// A single entry in the priority indexing queue.
///
/// `last_queried` is a wall-clock [`Instant`] captured at the moment the
/// path was enqueued. The `Ord` impl reverses the natural `Instant`
/// ordering so a [`BinaryHeap<QueueEntry>`] — a max-heap — pops the
/// *newest* [`Instant`] first, realising the "recently queried files
/// first" invariant from master-plan §18 Phase 1 Week 3 line 1745.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEntry {
    /// The file path being priority-indexed.
    pub path: PathBuf,
    /// Wall-clock moment at which the path was most recently touched.
    pub last_queried: Instant,
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap. We want the *newest* Instant at the
        // top, so we reverse the natural Instant ordering: the larger
        // Instant wins the max-heap comparison.
        self.last_queried.cmp(&other.last_queried)
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Recency-ordered priority queue of file paths.
///
/// Internally a `Mutex<BinaryHeap<QueueEntry>>`. The queue is **sync-
/// safe via [`std::sync::Mutex`]** — NOT [`tokio::sync::Mutex`] — because
/// every public method is synchronous: no `.await` is ever held under the
/// lock, so the tokio-docs guidance to prefer `tokio::sync::Mutex` does
/// not apply. `PoisonError` is handled internally via
/// [`std::sync::PoisonError::into_inner`]: the heap invariant is
/// consistent at any panic point because each mutation is a single heap
/// insert or pop, so readers can safely observe post-panic state.
#[derive(Debug, Default)]
pub struct PriorityIndexingQueue {
    inner: Mutex<BinaryHeap<QueueEntry>>,
}

impl PriorityIndexingQueue {
    /// Construct an empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue `path` with `last_queried = Instant::now()`.
    ///
    /// The entry is pushed onto the heap; on next `pop()` the entry with
    /// the newest [`Instant`] wins. Duplicate paths are permitted — each
    /// enqueue produces a distinct [`QueueEntry`] with its own timestamp.
    #[tracing::instrument(
        level = "trace",
        name = "ucil.daemon.priority_queue.enqueue",
        skip(self),
        fields(path = %path.display()),
    )]
    pub fn enqueue(&self, path: PathBuf) {
        let entry = QueueEntry {
            path,
            last_queried: Instant::now(),
        };
        let mut heap = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        heap.push(entry);
    }

    /// Re-prioritise `path` by pushing a fresh [`Instant`].
    ///
    /// This is an alias for [`Self::enqueue`]. Any stale older entry for
    /// the same path remains in the heap and surfaces in its original
    /// position once the fresher entry has been popped — consistent with
    /// "recently queried files first".
    #[tracing::instrument(
        level = "trace",
        name = "ucil.daemon.priority_queue.touch",
        skip(self),
        fields(path = %path.display()),
    )]
    pub fn touch(&self, path: PathBuf) {
        self.enqueue(path);
    }

    /// Pop the entry with the newest [`Instant`] from the queue.
    #[must_use]
    pub fn pop(&self) -> Option<QueueEntry> {
        let mut heap = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        heap.pop()
    }

    /// Peek at (clone) the entry with the newest [`Instant`] without
    /// removing it.
    #[must_use]
    pub fn peek(&self) -> Option<QueueEntry> {
        let heap = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        heap.peek().cloned()
    }

    /// Number of entries currently in the queue.
    #[must_use]
    pub fn len(&self) -> usize {
        let heap = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        heap.len()
    }

    /// `true` when the queue has zero entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let heap = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        heap.is_empty()
    }
}

// ── Module-root acceptance tests (flat; NO `mod tests { }` wrapper) ─────────
//
// DEC-0005 mandates module-root flat tests so that nextest selectors of
// the form `priority_queue::test_*` resolve directly. Matches the
// precedent set by `watcher::test_*`, `call_hierarchy::test_*`,
// `quality_pipeline::test_*`, and `knowledge_graph::test_*`.

#[cfg(test)]
#[test]
fn test_queue_entry_ord_is_newest_first() {
    use std::time::Duration;

    let older = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("Instant arithmetic — now() minus 1s cannot underflow on any supported platform");
    let newer = Instant::now();
    let a = QueueEntry {
        path: PathBuf::from("/tmp/older"),
        last_queried: older,
    };
    let b = QueueEntry {
        path: PathBuf::from("/tmp/newer"),
        last_queried: newer,
    };

    let mut heap: BinaryHeap<QueueEntry> = BinaryHeap::new();
    heap.push(a);
    heap.push(b.clone());

    let top = heap.peek().expect("heap must have a top entry");
    assert_eq!(
        top, &b,
        "BinaryHeap top must be the entry with the newer Instant",
    );
}

#[cfg(test)]
#[test]
fn test_enqueue_and_pop_single_path() {
    let queue = PriorityIndexingQueue::new();
    let path = PathBuf::from("/tmp/single.rs");
    queue.enqueue(path.clone());
    assert_eq!(queue.len(), 1);
    let popped = queue.pop().expect("must pop the enqueued entry");
    assert_eq!(popped.path, path);
    assert_eq!(queue.len(), 0);
}

#[cfg(test)]
#[test]
fn test_touch_reorders_by_recency() {
    use std::thread::sleep;
    use std::time::Duration;

    let queue = PriorityIndexingQueue::new();
    let a = PathBuf::from("/tmp/a.rs");
    let b = PathBuf::from("/tmp/b.rs");

    queue.enqueue(a.clone());
    // 2 ms gap guarantees a strictly-later `Instant` on every tier-1 CI
    // target (Linux nanosecond resolution, macOS coarser but still
    // well-under 1 ms of noise).
    sleep(Duration::from_millis(2));
    queue.enqueue(b.clone());

    // Re-prioritise `a` — it should now be the newest entry.
    sleep(Duration::from_millis(2));
    queue.touch(a.clone());

    // Pop order: newest `a` touch, then `b` (second enqueue), then the
    // stale `a` first-enqueue entry.
    let first = queue.pop().expect("first pop");
    assert_eq!(first.path, a, "touched path must come off the heap first");
    let second = queue.pop().expect("second pop");
    assert_eq!(
        second.path, b,
        "second-enqueued path must come off before the stale first-enqueue",
    );
    let third = queue.pop().expect("third pop");
    assert_eq!(
        third.path, a,
        "stale first-enqueue of `a` surfaces last — duplicates are preserved",
    );
    assert!(queue.is_empty(), "queue must be empty after three pops");
}

#[cfg(test)]
#[test]
fn test_len_tracks_entries() {
    let queue = PriorityIndexingQueue::new();
    queue.enqueue(PathBuf::from("/tmp/a"));
    queue.enqueue(PathBuf::from("/tmp/b"));
    queue.enqueue(PathBuf::from("/tmp/c"));
    assert_eq!(queue.len(), 3);
    assert!(!queue.is_empty());

    let _ = queue.pop();
    let _ = queue.pop();
    let _ = queue.pop();
    assert_eq!(queue.len(), 0);
    assert!(queue.is_empty());
}

#[cfg(test)]
#[test]
fn test_peek_does_not_consume() {
    let queue = PriorityIndexingQueue::new();
    let path = PathBuf::from("/tmp/peek.rs");
    queue.enqueue(path.clone());

    let first = queue.peek().expect("first peek");
    let second = queue.peek().expect("second peek");
    assert_eq!(first.path, path);
    assert_eq!(second.path, path);
    assert_eq!(queue.len(), 1, "peek must not consume the top entry");
}
