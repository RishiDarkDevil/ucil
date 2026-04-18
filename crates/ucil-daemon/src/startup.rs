//! Progressive startup orchestrator — bootstraps the MCP server and the
//! priority indexing queue for `P1-W3-F08`.
//!
//! Master-plan §18 Phase 1 Week 3 line 1745 is the authoritative spec:
//!
//! > Week 3 Feature 6: Progressive startup: MCP server available
//! > immediately, priority indexing queue.
//!
//! The 0-2 s budget comes from §21.2 lines 2196-2204 (daemon startup
//! phases): Phase 1 [0-2 s] — "MCP server is LIVE, 5 core tools accept
//! queries". The daemon startup sequence in §10.2 lines 998-1008 places
//! "Start MCP server — begin accepting queries" as step 8; this module
//! captures the wall-clock at the start of step 8 and signals through a
//! [`ReadyHandle`] once the server has successfully emitted its first
//! JSON-RPC response frame.
//!
//! The orchestrator exposes four items:
//!
//! 1. [`STARTUP_DEADLINE`] — the 2 s budget from §21.2.
//! 2. [`StartupError`] — typed error enum via `thiserror`.
//! 3. [`ProgressiveStartup`] — owns an [`crate::server::McpServer`] and
//!    a shared reference to a
//!    [`crate::priority_queue::PriorityIndexingQueue`] (behind
//!    [`std::sync::Arc`]). [`ProgressiveStartup::start`] spawns the
//!    server's `serve` loop on a tokio task and returns a
//!    `(JoinHandle, ReadyHandle)` pair.
//! 4. [`ReadyHandle`] — an async future that resolves with the measured
//!    wall-clock duration from `start()` kick-off to the server's first
//!    successful response frame.
//!
//! A private `ReadyProbeWriter` adapter wraps the outbound
//! [`tokio::io::AsyncWrite`] and signals a `oneshot::Sender` on the first
//! complete JSON-RPC response frame (detected by a trailing `\n` AND the
//! inbound body carrying an `"id"` token). Subsequent writes pass
//! through unchanged. Integration with the real indexer task is out of
//! scope for this work-order — see §10.2 step 7 (background indexer).
//!
//! Helper [`handle_call_for_priority`] walks a JSON-RPC `tools/call`
//! argument payload's `current_task.files_in_context` array (CEQP
//! universal per §8.2) and calls
//! [`crate::priority_queue::PriorityIndexingQueue::touch`] for each
//! string entry, so the priority-ordering invariant is exercisable
//! end-to-end without wiring a real indexer.

// Public API items share a name prefix with the module ("startup" →
// `ProgressiveStartup`, `StartupError`, `ReadyHandle`). Matches the
// `watcher::FileWatcher`, `plugin_manager::PluginManager`, and
// `session_manager::SessionManager` convention.
#![allow(clippy::module_name_repetitions)]

use std::{
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use serde_json::Value;
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};

use crate::{priority_queue::PriorityIndexingQueue, server::McpError, server::McpServer};

/// Wall-clock budget for "daemon start → first MCP response".
///
/// Fixed at 2000 ms per master-plan §21.2 lines 2196-2204 (Phase 1
/// [0-2 s]) and the `P1-W3-F08` description ("within 2 seconds").
pub const STARTUP_DEADLINE: Duration = Duration::from_millis(2000);

/// Slack budget applied inside [`ReadyHandle::wait`] so the
/// [`StartupError::DeadlineExceeded`] path is reachable without the
/// caller having to wrap the await in its own `timeout`. Kept tight —
/// the caller asserts against [`STARTUP_DEADLINE`], not against the
/// slack-inflated budget.
const READY_WAIT_SLACK: Duration = Duration::from_millis(500);

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors produced by the progressive-startup orchestrator.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StartupError {
    /// The [`ReadyHandle`] did not resolve inside
    /// [`STARTUP_DEADLINE`] + slack. `elapsed_ms` is the observed wait,
    /// `budget_ms` is `STARTUP_DEADLINE` for reference.
    #[error("startup deadline exceeded after {elapsed_ms} ms (budget = {budget_ms} ms)")]
    DeadlineExceeded {
        /// Observed wait, in milliseconds.
        elapsed_ms: u64,
        /// Configured budget, in milliseconds — always
        /// [`STARTUP_DEADLINE`] as millis.
        budget_ms: u64,
    },
    /// The spawned server task panicked or was cancelled.
    #[error("server task panicked")]
    ServerTaskJoin(#[from] tokio::task::JoinError),
    /// The server returned a typed MCP transport error.
    #[error("mcp transport error: {0}")]
    Mcp(#[from] McpError),
}

// ── Orchestrator ─────────────────────────────────────────────────────────────

/// Progressive-startup orchestrator.
///
/// Owns an [`McpServer`] and a shared handle to a
/// [`PriorityIndexingQueue`]. The queue is shared by [`std::sync::Arc`]
/// so downstream integration (the real indexer task, a hook receiver,
/// etc.) can retain a handle after [`Self::start`] has consumed the
/// orchestrator.
#[derive(Debug)]
pub struct ProgressiveStartup {
    server: McpServer,
    queue: Arc<PriorityIndexingQueue>,
}

impl ProgressiveStartup {
    /// Bundle a ready-to-serve [`McpServer`] with the shared priority
    /// queue.
    #[must_use]
    pub const fn new(server: McpServer, queue: Arc<PriorityIndexingQueue>) -> Self {
        Self { server, queue }
    }

    /// Borrow the shared priority queue. Useful for tests and for code
    /// that needs to keep priming the queue after `start` has been
    /// called.
    #[must_use]
    pub const fn queue(&self) -> &Arc<PriorityIndexingQueue> {
        &self.queue
    }

    /// Spawn the MCP server loop on a tokio task and return
    /// `(JoinHandle, ReadyHandle)`.
    ///
    /// The [`JoinHandle`] resolves with the inner
    /// [`Result<(), McpError>`] produced by
    /// [`McpServer::serve`]. The [`ReadyHandle`] resolves with the
    /// wall-clock [`Duration`] between this `start` call and the
    /// server's first successful JSON-RPC response frame.
    ///
    /// `reader` and `writer` must be the server-side halves of the
    /// wire transport; the caller retains the client-side halves.
    #[tracing::instrument(
        level = "info",
        name = "ucil.daemon.startup.start",
        skip(self, reader, writer),
        fields(queue_len = self.queue.len()),
    )]
    pub fn start<R, W>(
        self,
        reader: R,
        writer: W,
    ) -> (JoinHandle<Result<(), McpError>>, ReadyHandle)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let start_instant = Instant::now();
        let (tx, rx) = oneshot::channel::<Duration>();
        let probe_writer = ReadyProbeWriter::new(writer, start_instant, tx);
        let server = self.server;
        let handle = tokio::spawn(async move { server.serve(reader, probe_writer).await });
        (handle, ReadyHandle { rx })
    }
}

// ── ReadyHandle ──────────────────────────────────────────────────────────────

/// Future-like handle that resolves with the measured startup duration.
///
/// The [`ReadyHandle::wait`] method wraps the inner
/// [`oneshot::Receiver`] in a [`tokio::time::timeout`] with
/// [`STARTUP_DEADLINE`] plus a small internal slack so a caller that
/// never probes the result still surfaces
/// [`StartupError::DeadlineExceeded`] eventually rather than hanging
/// forever.
#[derive(Debug)]
pub struct ReadyHandle {
    rx: oneshot::Receiver<Duration>,
}

impl ReadyHandle {
    /// Await the startup signal.
    ///
    /// Returns the wall-clock elapsed between
    /// [`ProgressiveStartup::start`] and the server's first successful
    /// response frame.
    ///
    /// # Errors
    ///
    /// * [`StartupError::DeadlineExceeded`] — no response inside
    ///   [`STARTUP_DEADLINE`] plus the internal slack budget.
    /// * Any other `StartupError` variant if the inner oneshot is
    ///   closed before the signal arrives (the probe writer was
    ///   dropped without completing a frame) — reported as
    ///   `DeadlineExceeded` with `elapsed_ms = 0` because no partial
    ///   duration is available.
    pub async fn wait(self) -> Result<Duration, StartupError> {
        let budget = STARTUP_DEADLINE + READY_WAIT_SLACK;
        let budget_ms = u64::try_from(STARTUP_DEADLINE.as_millis()).unwrap_or(u64::MAX);
        let started = Instant::now();
        match timeout(budget, self.rx).await {
            Ok(Ok(duration)) => Ok(duration),
            Ok(Err(_canceled)) => {
                // Sender dropped without signalling. Report as a
                // deadline-exceeded with the observed wait so the caller
                // has a non-None time to assert against.
                let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                Err(StartupError::DeadlineExceeded {
                    elapsed_ms,
                    budget_ms,
                })
            }
            Err(_elapsed) => {
                let elapsed_ms = u64::try_from(budget.as_millis()).unwrap_or(u64::MAX);
                Err(StartupError::DeadlineExceeded {
                    elapsed_ms,
                    budget_ms,
                })
            }
        }
    }
}

// ── ReadyProbeWriter ─────────────────────────────────────────────────────────

/// Writer adapter that signals a `oneshot::Sender<Duration>` on the
/// first complete JSON-RPC response frame emitted by the wrapped writer.
///
/// A frame is "complete" when the adapter has observed (a) a
/// [`AsyncWrite::poll_write`] buffer containing the token `"id"` — every
/// JSON-RPC response envelope the server produces carries an `id` field
/// (see `server.rs::jsonrpc_error` / `handle_tools_list` /
/// `handle_initialize` / `handle_tools_call`) — AND (b) a subsequent
/// (or the same) `poll_write` whose final byte is `\n`, the per-frame
/// terminator. The server's `serve` loop splits a response into two
/// `write_all` calls (the JSON body, then the `\n`), so tracking the
/// two markers as separate booleans across calls is required. Once the
/// oneshot fires, subsequent writes pass through untouched.
struct ReadyProbeWriter<W> {
    inner: W,
    start: Instant,
    tx: Option<oneshot::Sender<Duration>>,
    seen_id: bool,
    seen_newline: bool,
}

impl<W> ReadyProbeWriter<W> {
    const fn new(inner: W, start: Instant, tx: oneshot::Sender<Duration>) -> Self {
        Self {
            inner,
            start,
            tx: Some(tx),
            seen_id: false,
            seen_newline: false,
        }
    }
}

impl<W> AsyncWrite for ReadyProbeWriter<W>
where
    W: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let poll = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(written)) = &poll {
            if *written > 0 && self.tx.is_some() {
                let slice = &buf[..*written];
                if !self.seen_id && slice.windows(4).any(|w| w == b"\"id\"") {
                    self.seen_id = true;
                }
                if !self.seen_newline && matches!(slice.last(), Some(b'\n')) {
                    self.seen_newline = true;
                }
                if self.seen_id && self.seen_newline {
                    let elapsed = self.start.elapsed();
                    if let Some(sender) = self.tx.take() {
                        // Ignore send failure — the receiver may already
                        // be dropped if the caller never awaited
                        // `ReadyHandle`.
                        let _ = sender.send(elapsed);
                    }
                }
            }
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

// ── Priority-queue helper ────────────────────────────────────────────────────

/// Walk `arguments.current_task.files_in_context` (CEQP universal per
/// master-plan §8.2) and call
/// [`crate::priority_queue::PriorityIndexingQueue::touch`] for each
/// string entry.
///
/// The touch order matches the array order: the last string entry is
/// the most-recently-touched, and therefore pops first from the queue.
pub fn handle_call_for_priority(queue: &PriorityIndexingQueue, arguments: &Value) {
    let Some(files) = arguments
        .get("current_task")
        .and_then(|ct| ct.get("files_in_context"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for entry in files {
        if let Some(path_str) = entry.as_str() {
            queue.touch(PathBuf::from(path_str));
        }
    }
}

// ── Module-root acceptance tests (flat; NO `mod tests { }` wrapper) ─────────

#[cfg(test)]
#[test]
fn test_startup_deadline_constant_is_2s() {
    assert_eq!(STARTUP_DEADLINE, Duration::from_millis(2000));
}

#[cfg(test)]
#[test]
fn test_handle_call_for_priority_touches_files_in_context() {
    use serde_json::json;

    let queue = PriorityIndexingQueue::new();
    let arguments = json!({
        "current_task": {
            "files_in_context": ["/tmp/a.rs", "/tmp/b.rs"]
        }
    });
    handle_call_for_priority(&queue, &arguments);
    assert_eq!(queue.len(), 2, "both paths must be enqueued");
    let first = queue.pop().expect("first pop");
    assert_eq!(
        first.path,
        PathBuf::from("/tmp/b.rs"),
        "last-touched path pops first",
    );
    let second = queue.pop().expect("second pop");
    assert_eq!(
        second.path,
        PathBuf::from("/tmp/a.rs"),
        "earlier-touched path pops next",
    );
}

#[cfg(test)]
#[test]
fn test_handle_call_for_priority_ignores_missing_files_key() {
    use serde_json::json;

    let queue = PriorityIndexingQueue::new();
    let arguments = json!({ "reason": "x" });
    handle_call_for_priority(&queue, &arguments);
    assert!(
        queue.is_empty(),
        "missing current_task.files_in_context → queue stays empty",
    );
}

#[cfg(test)]
#[tokio::test]
async fn test_ready_handle_resolves_on_first_write() {
    use tokio::io::{duplex, AsyncWriteExt};

    let (writer_half, mut reader_half) = duplex(1024);
    let start = Instant::now();
    let (tx, rx) = oneshot::channel::<Duration>();
    let mut probe = ReadyProbeWriter::new(writer_half, start, tx);

    probe
        .write_all(
            br#"{"jsonrpc":"2.0","id":1,"result":{}}
"#,
        )
        .await
        .expect("write frame");
    probe.flush().await.expect("flush frame");

    let handle = ReadyHandle { rx };
    // Outer cap: the inner READY_WAIT_SLACK already enforces
    // STARTUP_DEADLINE + slack; this second timeout is a belt-and-braces
    // safety that the whole test returns within 500 ms.
    let resolved = timeout(Duration::from_millis(500), handle.wait())
        .await
        .expect("outer wait must finish within 500 ms")
        .expect("ready handle must resolve with a Duration");
    assert!(
        resolved <= Duration::from_millis(500),
        "reported duration must be within 500 ms, got {resolved:?}",
    );

    // Drain the duplex so nothing is leaked — the far end holds the
    // written bytes; read them to unblock any background flush.
    let mut scratch = [0u8; 64];
    let _ = tokio::io::AsyncReadExt::read(&mut reader_half, &mut scratch).await;
}
