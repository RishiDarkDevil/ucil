//! UCIL session manager: per-session state keyed by UUID.
//!
//! A [`SessionManager`] maintains an in-memory map of [`SessionInfo`] records.
//! Each session is created for a specific git working directory, records the
//! current branch, and is addressable by its [`SessionId`].
//!
//! Git subprocess calls are wrapped in [`tokio::time::timeout`] (5 s) and use
//! [`tokio::process::Command`] so they are non-blocking in the async runtime.

// Public API items intentionally share a name prefix with the module
// ("session_manager" → "SessionId", "SessionInfo", "SessionManager",
// "SessionError").
#![allow(clippy::module_name_repetitions)]

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::session_ttl::{compute_expires_at, is_expired};

/// Default session TTL in seconds — 1 hour.
///
/// Re-export of [`crate::session_ttl::DEFAULT_TTL_SECS`] so existing
/// public paths (`ucil_daemon::DEFAULT_TTL_SECS`,
/// `ucil_daemon::session_manager::DEFAULT_TTL_SECS`) keep working
/// after the P1-W4-F07 extraction of TTL helpers into a dedicated
/// module.  See the `session_ttl` module rustdoc for the policy.
pub use crate::session_ttl::DEFAULT_TTL_SECS;

/// A single tool invocation recorded on a session's call history.
///
/// Recorded by [`SessionManager::record_call`]; `at` is a unix-seconds
/// timestamp captured at record time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRecord {
    /// Name of the tool that was invoked (e.g. `"ucil.pack_context"`).
    pub tool: String,
    /// Unix timestamp (seconds) at which the call was recorded.
    pub at: u64,
}

/// Return the current unix time in seconds, or 0 if the clock is before
/// `UNIX_EPOCH` (which should be impossible on any modern OS).
///
/// Mirrors the existing `created_at` computation in
/// [`SessionManager::create_session`] — no new error path is introduced
/// just to record state on a live session.
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unique session identifier (UUID v4).
///
/// Implemented as a transparent newtype over [`String`] so it serialises as a
/// plain JSON string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Metadata about a git worktree discovered via `git worktree list --porcelain`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree root.
    pub path: PathBuf,
    /// Branch name, or `None` for a detached HEAD.
    pub branch: Option<String>,
    /// The full SHA of the HEAD commit.
    pub head_sha: String,
}

/// Metadata stored for each live UCIL session.
///
/// The four state-tracking fields (`call_history`, `inferred_domain`,
/// `files_in_context`, `expires_at`) were added in Phase 1 Week 4
/// (feature P1-W4-F07). Each is annotated with `#[serde(default)]` so
/// older on-disk blobs — which did not know about these fields — continue
/// to deserialise; they default to empty / `None` / `0` respectively.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Unique session identifier.
    pub id: SessionId,
    /// Branch detected at session-creation time.
    pub branch: String,
    /// Working directory / worktree root for this session.
    pub worktree_root: PathBuf,
    /// Unix timestamp (seconds) when the session was created.
    pub created_at: u64,
    /// Ordered history of tool invocations on this session.
    #[serde(default)]
    pub call_history: Vec<CallRecord>,
    /// Domain inferred for this session (if any), e.g. `"backend-api"`.
    #[serde(default)]
    pub inferred_domain: Option<String>,
    /// Files currently in scope for this session. Uses `BTreeSet` for
    /// deterministic iteration order in tests and snapshots.
    #[serde(default)]
    pub files_in_context: BTreeSet<PathBuf>,
    /// Unix timestamp (seconds) at which this session expires and is
    /// eligible for purge by [`SessionManager::purge_expired`].
    #[serde(default)]
    pub expires_at: u64,
}

/// Errors that can arise from session-manager operations.
#[derive(Debug, Error)]
pub enum SessionError {
    /// A git subprocess exited with a non-zero status code.
    #[error("git command failed: {0}")]
    Git(String),
    /// A git subprocess exceeded the 5-second timeout.
    #[error("git command timed out")]
    Timeout,
    /// An OS-level I/O error while spawning a subprocess.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The given path is not inside a git repository.
    #[error("not a git repository: {}", .0.display())]
    NotAGitRepo(PathBuf),
}

/// Timeout applied to every git subprocess call.
const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Manages UCIL sessions in memory.
///
/// All state-mutating methods take `&self`; the internal map is protected by a
/// [`tokio::sync::RwLock`].  The manager can be cheaply cloned (it wraps an
/// [`Arc`]).
///
/// # Examples
///
/// ```no_run
/// use ucil_daemon::session_manager::SessionManager;
///
/// # #[tokio::main]
/// # async fn main() {
/// let sm = SessionManager::new();
/// let id = sm
///     .create_session(std::env::current_dir().unwrap().as_path())
///     .await
///     .unwrap();
/// let info = sm.get_session(&id).await.unwrap();
/// println!("branch: {}", info.branch);
/// # }
/// ```
#[derive(Debug)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<SessionId, SessionInfo>>>,
}

impl SessionManager {
    /// Create a new, empty `SessionManager`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session rooted at `workdir`.
    ///
    /// Detects the current git branch via [`Self::detect_branch`] and stores
    /// a new [`SessionInfo`] keyed by a fresh [`SessionId`].  Each call
    /// returns a distinct UUID.
    ///
    /// # Errors
    ///
    /// Propagates any [`SessionError`] returned by [`Self::detect_branch`].
    #[must_use = "the returned SessionId is needed to retrieve the session later"]
    pub async fn create_session(&self, workdir: &Path) -> Result<SessionId, SessionError> {
        let branch = Self::detect_branch(workdir).await?;
        let id = SessionId::new();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let info = SessionInfo {
            id: id.clone(),
            branch,
            worktree_root: workdir.to_path_buf(),
            created_at,
            call_history: Vec::new(),
            inferred_domain: None,
            files_in_context: BTreeSet::new(),
            expires_at: compute_expires_at(created_at, DEFAULT_TTL_SECS),
        };
        self.sessions.write().await.insert(id.clone(), info);
        Ok(id)
    }

    /// Append a [`CallRecord`] to the session's `call_history`.
    ///
    /// The `at` field is stamped with the current unix time (seconds).
    /// Returns `Some(())` if the session existed, `None` otherwise — this
    /// mirrors the `Option`-based missing-key convention used by
    /// [`SessionManager::get_session`].
    pub async fn record_call(&self, id: &SessionId, tool: &str) -> Option<()> {
        let record = CallRecord {
            tool: tool.to_owned(),
            at: now_unix_secs(),
        };
        self.sessions
            .write()
            .await
            .get_mut(id)
            .map(|info| info.call_history.push(record))
    }

    /// Insert `file` into the session's `files_in_context` set.
    ///
    /// Duplicate paths are de-duplicated by the underlying
    /// [`std::collections::BTreeSet`]; calling this twice with the same path
    /// is a no-op on the set but still returns `Some(())`.
    pub async fn add_file_to_context(&self, id: &SessionId, file: PathBuf) -> Option<()> {
        self.sessions.write().await.get_mut(id).map(|info| {
            info.files_in_context.insert(file);
        })
    }

    /// Filter `candidates` against the session's `files_in_context`,
    /// returning only the paths the agent does NOT already have.
    ///
    /// Implements session-scoped result deduplication per master-plan
    /// §5.2 line 459 ("Session dedup: don't return same code block twice
    /// in a session") and §6.3 line 666 ("1. Session dedup: remove
    /// results the agent already has (`files_in_context`)"); see also
    /// §18 Phase 2 Week 7 line 1782 ("Session deduplication tracking").
    ///
    /// # Invariants
    ///
    /// - If `id` does not name a live session — either it was never
    ///   created or [`SessionManager::purge_expired`] has retained it
    ///   out — the candidates are returned unchanged. This is the
    ///   structural realisation of the master-plan invariant
    ///   "session-scoped dedup store is cleared on session expiry":
    ///   the dedup state is the `files_in_context` field on
    ///   [`SessionInfo`], so the moment the session is purged the
    ///   future-dedup pass-through is automatic — no separate cleanup
    ///   step is needed.
    /// - Order is preserved: the kept entries appear in the same order
    ///   as in `candidates`.
    /// - Equality is `PathBuf` equality (lexical), not filesystem
    ///   canonicalisation; callers that need canonicalised matching
    ///   must normalise both sides before populating the session.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::PathBuf;
    /// use ucil_daemon::session_manager::SessionManager;
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// let sm = SessionManager::new();
    /// let id = sm
    ///     .create_session(std::env::current_dir().unwrap().as_path())
    ///     .await
    ///     .unwrap();
    /// sm.add_file_to_context(&id, PathBuf::from("src/lib.rs"))
    ///     .await
    ///     .unwrap();
    /// let candidates = vec![
    ///     PathBuf::from("src/lib.rs"),
    ///     PathBuf::from("src/main.rs"),
    /// ];
    /// let kept = sm.dedup_against_context(&id, candidates).await;
    /// assert_eq!(kept, vec![PathBuf::from("src/main.rs")]);
    /// # }
    /// ```
    pub async fn dedup_against_context(
        &self,
        id: &SessionId,
        candidates: Vec<PathBuf>,
    ) -> Vec<PathBuf> {
        let sessions = self.sessions.read().await;
        match sessions.get(id) {
            Some(info) => candidates
                .into_iter()
                .filter(|p| !info.files_in_context.contains(p))
                .collect(),
            None => candidates,
        }
    }

    /// Bulk-insert `files` into the session's `files_in_context` set
    /// under a single write-lock acquisition.
    ///
    /// This is the bulk companion of [`SessionManager::add_file_to_context`]
    /// and is intended for the post-fusion path of multi-result tools
    /// (e.g. `search_code` returning N file paths in one shot — see
    /// master-plan §6.3 line 666). One write-lock per call beats N
    /// round-trips through `add_file_to_context`.
    ///
    /// The signature takes `&[PathBuf]` so callers can pass either a
    /// `&Vec<PathBuf>` or a slice without an extra allocation. Returns
    /// `Some(())` when the session exists, `None` when it does not —
    /// the same shape as `add_file_to_context`.
    ///
    /// `BTreeSet::extend` de-duplicates internally, so calling this
    /// twice with overlapping slices is a no-op on duplicates.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::PathBuf;
    /// use ucil_daemon::session_manager::SessionManager;
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// let sm = SessionManager::new();
    /// let id = sm
    ///     .create_session(std::env::current_dir().unwrap().as_path())
    ///     .await
    ///     .unwrap();
    /// let hits = vec![
    ///     PathBuf::from("src/lib.rs"),
    ///     PathBuf::from("src/main.rs"),
    /// ];
    /// sm.add_files_to_context(&id, &hits).await.unwrap();
    /// # }
    /// ```
    pub async fn add_files_to_context(&self, id: &SessionId, files: &[PathBuf]) -> Option<()> {
        self.sessions
            .write()
            .await
            .get_mut(id)
            .map(|info| info.files_in_context.extend(files.iter().cloned()))
    }

    /// Set the session's `inferred_domain` field.
    ///
    /// Overwrites any prior value.
    pub async fn set_inferred_domain(&self, id: &SessionId, domain: String) -> Option<()> {
        self.sessions
            .write()
            .await
            .get_mut(id)
            .map(|info| info.inferred_domain = Some(domain))
    }

    /// Set the session's TTL, recomputing `expires_at = created_at + ttl_secs`.
    ///
    /// A `ttl_secs` of 0 yields `expires_at == created_at`, which means
    /// [`SessionManager::purge_expired`] with any `now_secs >= created_at`
    /// will remove the session.
    pub async fn set_ttl(&self, id: &SessionId, ttl_secs: u64) -> Option<()> {
        self.sessions
            .write()
            .await
            .get_mut(id)
            .map(|info| info.expires_at = compute_expires_at(info.created_at, ttl_secs))
    }

    /// Remove every session whose `expires_at <= now_secs`.
    ///
    /// Takes a single write lock, iterates once via
    /// [`std::collections::HashMap::retain`], and returns the count of
    /// entries removed.
    pub async fn purge_expired(&self, now_secs: u64) -> usize {
        let mut sessions = self.sessions.write().await;
        let before = sessions.len();
        sessions.retain(|_, info| !is_expired(info.expires_at, now_secs));
        let after = sessions.len();
        drop(sessions);
        before - after
    }

    /// Detect the current git branch for the repository at `workdir`.
    ///
    /// Runs `git rev-parse --abbrev-ref HEAD` with a 5-second timeout.
    /// Returns the branch name, or `"HEAD:<short-sha>"` for a detached HEAD.
    ///
    /// # Errors
    ///
    /// - [`SessionError::NotAGitRepo`] — `workdir` is not inside a git repo.
    /// - [`SessionError::Timeout`] — the git subprocess exceeded 5 seconds.
    /// - [`SessionError::Io`] — the subprocess could not be spawned.
    pub async fn detect_branch(workdir: &Path) -> Result<String, SessionError> {
        let output = tokio::time::timeout(GIT_TIMEOUT, async {
            tokio::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(workdir)
                .output()
                .await
        })
        .await
        .map_err(|_| SessionError::Timeout)?
        .map_err(SessionError::Io)?;

        if !output.status.success() {
            return Err(SessionError::NotAGitRepo(workdir.to_path_buf()));
        }

        let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();

        // Detached HEAD: `git rev-parse --abbrev-ref HEAD` prints "HEAD".
        // Fall back to `git rev-parse --short HEAD` for a human-readable label.
        if branch == "HEAD" {
            let sha_out = tokio::time::timeout(GIT_TIMEOUT, async {
                tokio::process::Command::new("git")
                    .args(["rev-parse", "--short", "HEAD"])
                    .current_dir(workdir)
                    .output()
                    .await
            })
            .await
            .map_err(|_| SessionError::Timeout)?
            .map_err(SessionError::Io)?;

            let short = String::from_utf8_lossy(&sha_out.stdout).trim().to_owned();
            return Ok(format!("HEAD:{short}"));
        }

        Ok(branch)
    }

    /// Discover all git worktrees for the repository that contains `repo_root`.
    ///
    /// Runs `git worktree list --porcelain` and parses the structured output.
    ///
    /// # Errors
    ///
    /// - [`SessionError::Git`] — the git command failed.
    /// - [`SessionError::Timeout`] — the git subprocess exceeded 5 seconds.
    /// - [`SessionError::Io`] — the subprocess could not be spawned.
    pub async fn discover_worktrees(repo_root: &Path) -> Result<Vec<WorktreeInfo>, SessionError> {
        let output = tokio::time::timeout(GIT_TIMEOUT, async {
            tokio::process::Command::new("git")
                .args(["worktree", "list", "--porcelain"])
                .current_dir(repo_root)
                .output()
                .await
        })
        .await
        .map_err(|_| SessionError::Timeout)?
        .map_err(SessionError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            return Err(SessionError::Git(stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_worktree_porcelain(&stdout))
    }

    /// Look up a session by its [`SessionId`].
    ///
    /// Returns `None` if no session with that ID exists.
    pub async fn get_session(&self, id: &SessionId) -> Option<SessionInfo> {
        self.sessions.read().await.get(id).cloned()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse the output of `git worktree list --porcelain`.
///
/// Blocks are separated by blank lines.  Each block looks like:
///
/// ```text
/// worktree /abs/path
/// HEAD <full-sha>
/// branch refs/heads/<name>
/// ```
///
/// or, for a detached HEAD:
///
/// ```text
/// worktree /abs/path
/// HEAD <full-sha>
/// detached
/// ```
fn parse_worktree_porcelain(output: &str) -> Vec<WorktreeInfo> {
    let mut result = Vec::new();

    for block in output.split("\n\n") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let mut path: Option<PathBuf> = None;
        let mut head_sha = String::new();
        let mut branch: Option<String> = None;

        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(rest.trim()));
            } else if let Some(rest) = line.strip_prefix("HEAD ") {
                rest.trim().clone_into(&mut head_sha);
            } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
                branch = Some(rest.trim().to_owned());
            }
            // "detached" line → branch stays None — correct behaviour.
        }

        if let Some(p) = path {
            result.push(WorktreeInfo {
                path: p,
                branch,
                head_sha,
            });
        }
    }

    result
}

// P1-W4-F07 acceptance test. Placed at MODULE ROOT (NOT inside
// `#[cfg(test)] mod tests { }`) so the frozen nextest selector
// `session_manager::test_session_state_tracking` matches by exact path
// segments — nextest treats a `::tests::` nesting as a distinct path
// segment. See DEC-0005 and the WO-0007 rejection history.
#[cfg(test)]
#[tokio::test]
// DEC-0011: the `env_guard()` MutexGuard is held across the `git` spawn
// awaits on purpose — that is what fences concurrent PATH mutators.
// `#[tokio::test]` runs on a single-threaded runtime, so the
// non-`Send` guard cannot cross thread boundaries; the
// deadlock-mitigation `clippy::await_holding_lock` guards against does
// not apply here.
#[allow(clippy::await_holding_lock)]
async fn test_session_state_tracking() {
    // DEC-0011: fence PATH mutations in watcher tests
    let _g = crate::test_support::env_guard();
    let repo = std::env::current_dir().expect("current dir");
    let sm = SessionManager::new();
    let id = sm
        .create_session(&repo)
        .await
        .expect("create_session inside a git repo should succeed");

    // (2) Two calls → call_history has length 2 in insertion order.
    sm.record_call(&id, "ucil.pack_context")
        .await
        .expect("session exists");
    sm.record_call(&id, "ucil.who_calls")
        .await
        .expect("session exists");
    {
        let info = sm.get_session(&id).await.expect("session");
        assert_eq!(info.call_history.len(), 2, "two calls were recorded");
        assert_eq!(info.call_history[0].tool, "ucil.pack_context");
        assert_eq!(info.call_history[1].tool, "ucil.who_calls");
    }

    // (3) Two distinct files + a duplicate → set size stays 2.
    let f1 = PathBuf::from("src/lib.rs");
    let f2 = PathBuf::from("src/main.rs");
    sm.add_file_to_context(&id, f1.clone())
        .await
        .expect("session exists");
    sm.add_file_to_context(&id, f2.clone())
        .await
        .expect("session exists");
    sm.add_file_to_context(&id, f1.clone())
        .await
        .expect("session exists (duplicate)");
    {
        let info = sm.get_session(&id).await.expect("session");
        assert_eq!(
            info.files_in_context.len(),
            2,
            "BTreeSet de-dupes identical paths"
        );
        assert!(info.files_in_context.contains(&f1));
        assert!(info.files_in_context.contains(&f2));
    }

    // (4) Inferred-domain round-trip.
    sm.set_inferred_domain(&id, "backend-api".to_owned())
        .await
        .expect("session exists");
    {
        let info = sm.get_session(&id).await.expect("session");
        assert_eq!(info.inferred_domain.as_deref(), Some("backend-api"));
    }

    // (5) TTL + purge: ttl=1s means expires_at = created_at+1; purging
    // at created_at+2 must remove exactly this one session and get_session
    // must subsequently return None.
    let created_at = sm.get_session(&id).await.expect("session").created_at;
    sm.set_ttl(&id, 1).await.expect("session exists");
    let removed = sm.purge_expired(created_at + 2).await;
    assert_eq!(removed, 1, "exactly one session was expired and purged");
    assert!(
        sm.get_session(&id).await.is_none(),
        "purged session must no longer be retrievable"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // DEC-0011 — see `test_session_state_tracking`.
    async fn create_session_returns_fresh_uuid_each_call() {
        // DEC-0011: fence PATH mutations in watcher tests
        let _g = crate::test_support::env_guard();
        let repo = std::env::current_dir().expect("current dir");
        let sm = SessionManager::new();
        let id1 = sm.create_session(&repo).await.expect("first session");
        let id2 = sm.create_session(&repo).await.expect("second session");
        assert_ne!(
            id1, id2,
            "each create_session call must return a distinct SessionId"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // DEC-0011 — see `test_session_state_tracking`.
    async fn detect_branch_returns_non_empty_inside_git_repo() {
        // DEC-0011: fence PATH mutations in watcher tests
        let _g = crate::test_support::env_guard();
        let repo = std::env::current_dir().expect("current dir");
        let branch = SessionManager::detect_branch(&repo)
            .await
            .expect("detect_branch should succeed inside a git repo");
        assert!(!branch.is_empty(), "branch name must not be empty");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // DEC-0011 — see `test_session_state_tracking`.
    async fn detect_branch_errors_outside_git_repo() {
        // DEC-0011: fence PATH mutations in watcher tests
        let _g = crate::test_support::env_guard();
        // tempfile::TempDir creates a unique directory under /tmp, which is
        // NOT inside any git repository on a standard Linux system.
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let result = SessionManager::detect_branch(tmp.path()).await;
        assert!(
            result.is_err(),
            "detect_branch must return Err when called outside a git repo"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // DEC-0011 — see `test_session_state_tracking`.
    async fn discover_worktrees_returns_at_least_one() {
        // DEC-0011: fence PATH mutations in watcher tests
        let _g = crate::test_support::env_guard();
        let repo = std::env::current_dir().expect("current dir");
        let worktrees = SessionManager::discover_worktrees(&repo)
            .await
            .expect("discover_worktrees should succeed inside a git repo");
        assert!(
            !worktrees.is_empty(),
            "should discover at least one worktree (the main repo)"
        );
    }

    #[tokio::test]
    async fn get_session_returns_none_for_unknown_id() {
        let sm = SessionManager::new();
        let unknown = SessionId("00000000-0000-0000-0000-000000000000".to_owned());
        assert!(sm.get_session(&unknown).await.is_none());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // DEC-0011 — see `test_session_state_tracking`.
    async fn get_session_returns_some_after_create() {
        // DEC-0011: fence PATH mutations in watcher tests
        let _g = crate::test_support::env_guard();
        let repo = std::env::current_dir().expect("current dir");
        let sm = SessionManager::new();
        let id = sm.create_session(&repo).await.expect("create");
        let info = sm.get_session(&id).await.expect("should find session");
        assert_eq!(info.id, id);
        assert!(!info.branch.is_empty());
        assert_eq!(info.worktree_root, repo);
    }

    #[test]
    fn parse_worktree_porcelain_main_and_linked_and_detached() {
        let input = concat!(
            "worktree /repo\n",
            "HEAD abc123\n",
            "branch refs/heads/main\n",
            "\n",
            "worktree /repo-wt/feat\n",
            "HEAD def456\n",
            "branch refs/heads/feat/foo\n",
            "\n",
            "worktree /repo-wt/det\n",
            "HEAD ghi789\n",
            "detached\n",
            "\n",
        );
        let wts = parse_worktree_porcelain(input);
        assert_eq!(wts.len(), 3, "expected 3 worktrees");
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert_eq!(wts[0].head_sha, "abc123");
        assert_eq!(wts[1].branch.as_deref(), Some("feat/foo"));
        assert_eq!(wts[1].head_sha, "def456");
        assert_eq!(wts[2].branch, None, "detached HEAD should have None branch");
        assert_eq!(wts[2].head_sha, "ghi789");
    }
}
