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
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

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
        };
        self.sessions.write().await.insert(id.clone(), info);
        Ok(id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_session_returns_fresh_uuid_each_call() {
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
    async fn detect_branch_returns_non_empty_inside_git_repo() {
        let repo = std::env::current_dir().expect("current dir");
        let branch = SessionManager::detect_branch(&repo)
            .await
            .expect("detect_branch should succeed inside a git repo");
        assert!(!branch.is_empty(), "branch name must not be empty");
    }

    #[tokio::test]
    async fn detect_branch_errors_outside_git_repo() {
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
    async fn discover_worktrees_returns_at_least_one() {
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
    async fn get_session_returns_some_after_create() {
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
