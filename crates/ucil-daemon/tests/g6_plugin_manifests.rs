//! End-to-end integration tests for the on-disk GitHub + Git +
//! Filesystem G6 (Platform) plugin manifests
//! (P3-W10-F05 / P3-W10-F06 / P3-W10-F07).
//!
//! Each test loads the on-disk manifest at
//! `plugins/platform/<name>/plugin.toml`, substitutes any sentinel
//! placeholders in `transport.args` (`${UCIL_GIT_MCP_REPO}` for git;
//! `${UCIL_FS_MCP_ALLOWED_PATH}` for filesystem), drives the manifest's
//! `transport.command` as a real subprocess via
//! [`ucil_daemon::PluginManager::health_check_with_timeout`], and
//! asserts the live `tools/list` reply contains an expected canonical
//! tool name.
//!
//! Mocking `tokio::process::Command`, the spawned MCP server, or the
//! JSON-RPC dialogue is forbidden — the WO-0069 contract carried by
//! this WO is precisely that real MCP-server subprocesses speak real
//! JSON-RPC over stdio exactly the same way a Claude Code / Cursor /
//! Cline client would consume them at runtime. Each test exercises the
//! full handshake [`ucil_daemon::PluginManager::health_check`] performs
//! (`initialize` → `notifications/initialized` → `tools/list`) end-to-
//! end against the real `npx -y <pinned-pkg>` (GitHub, Filesystem) or
//! `uvx <pinned-pkg>` (Git) invocation.
//!
//! Set `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1` only on truly offline CI
//! builds (skips ALL external plugin-manifest suites including this
//! G6 suite); set the G6-specific `UCIL_SKIP_PLATFORM_PLUGIN_E2E=1`
//! to skip ONLY this suite (so an operator can keep the WO-0044 /
//! WO-0069 / WO-0072 / WO-0074 regression coverage without paying the
//! additional ~30-second-cold-cache npx + uvx fetches for the G6
//! plugins). The verifier MUST NOT set EITHER opt-out, per WO-0074
//! `scope_in` #14 carried by this WO.
//!
//! GitHub MCP API-key gating (per WO-0069 Mem0 precedent applied to
//! `GITHUB_PERSONAL_ACCESS_TOKEN` in WO-0075 scope_in #7): the
//! `tools/list` round-trip works WITHOUT a PAT — the upstream binary
//! returns the static tool catalog regardless of token presence. This
//! is what the integration test asserts. The API-key-gated
//! `tools/call` smoke lives only in `scripts/verify/P3-W10-F05.sh`,
//! NOT here, since cargo-test is the load-bearing gate for the
//! manifest-shape regression coverage and must be reproducible without
//! operator-state environment variables.
//!
//! Tests are wrapped in `mod g6_plugin_manifests` so nextest reports
//! them as `g6_plugin_manifests::github_manifest_health_check`,
//! `g6_plugin_manifests::git_manifest_health_check`, and
//! `g6_plugin_manifests::filesystem_manifest_health_check` matching
//! the WO-0075 acceptance selectors. Same wrapper pattern as the
//! existing `mod g5_plugin_manifests` block in
//! `tests/g5_plugin_manifests.rs:50` (DEC-0007 frozen-selector
//! module-root placement; carried per WO-0068 lessons §"For planner"
//! frozen-test selector substring-match REQUIRES module-root
//! placement). NO `mod tests { ... }` nesting; the test functions
//! live at `mod g6_plugin_manifests` ROOT per WO-0073 lessons §"For
//! planner".
//!
//! This file is a peer of `tests/g3_plugin_manifests.rs` (G3 suite),
//! `tests/g4_plugin_manifests.rs` (G4 suite),
//! `tests/g5_plugin_manifests.rs` (G5 suite), and
//! `tests/plugin_manifests.rs` (WO-0044 G2 regression guard) — five
//! group-isolated suites kept distinct so each group's
//! `UCIL_SKIP_<GROUP>_PLUGIN_E2E` opt-out is scoped distinctly. Per
//! WO-0069 lessons §executor #2 ("write a SEPARATE integration test
//! file per phase/group" — single-file-per-group keeps the
//! architecture / knowledge / search / context / platform test
//! surfaces isolated and avoids cross-group flake propagation).

mod g6_plugin_manifests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use ucil_daemon::{HealthStatus, PluginManager, PluginManifest};

    /// Generous first-run npx + uvx download budget — `npx -y <pkg>`
    /// and `uvx <pkg>` may fetch tarballs + transitive deps on a cold
    /// cache (the GitHub + Filesystem MCP packages each pull a few
    /// hundred small Node deps; mcp-server-git pulls a Python venv
    /// plus dulwich + GitPython). Subsequent runs hit the cache and
    /// complete in well under a second; the production-default
    /// `HEALTH_CHECK_TIMEOUT_MS` (5 s) is therefore fine for steady-
    /// state daemon ticks but inadequate for the very first post-
    /// install integration-test run on a fresh workstation. Mirror the
    /// WO-0044 / WO-0069 / WO-0072 / WO-0074 budget exactly.
    const FIRST_RUN_TIMEOUT_MS: u64 = 90_000;

    /// Walks up from this crate's manifest dir (`crates/ucil-daemon`) to
    /// the workspace root so the on-disk plugin manifests can be loaded
    /// regardless of the directory `cargo test` is invoked from.
    fn repo_root() -> PathBuf {
        // `CARGO_MANIFEST_DIR` for ucil-daemon is `<repo>/crates/ucil-daemon`;
        // two parents up is the workspace root.
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace root above crates/ucil-daemon")
            .to_path_buf()
    }

    /// Honours the `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS` opt-out for
    /// air-gapped CI runners that cannot reach npm + pypi at all (this
    /// is the same global opt-out honoured by the WO-0044
    /// `tests/plugin_manifests.rs`, WO-0069 `tests/g3_plugin_manifests.rs`,
    /// WO-0072 `tests/g4_plugin_manifests.rs`, and WO-0074
    /// `tests/g5_plugin_manifests.rs` suites) AND the G6-specific
    /// `UCIL_SKIP_PLATFORM_PLUGIN_E2E` opt-out for operators that want
    /// to keep the existing G2/G3/G4/G5 regression coverage but skip
    /// the additional GitHub + Git + Filesystem cold-cache budget.
    /// Either env set means "skip this test"; the verifier MUST NOT
    /// set either, per WO-0075 `scope_in` #14.
    fn skip_via_env() -> bool {
        std::env::var("UCIL_SKIP_EXTERNAL_PLUGIN_TESTS").is_ok()
            || std::env::var("UCIL_SKIP_PLATFORM_PLUGIN_E2E").is_ok()
    }

    /// Substitutes a sentinel placeholder token in `manifest.transport.args`
    /// with `replacement`, returning the patched manifest. Used by the
    /// Git and Filesystem tests (the GitHub manifest has no placeholder).
    /// The sentinel match is exact-string equality (each arg slot either
    /// equals the placeholder verbatim or is left alone) — substring
    /// matching is deliberately NOT performed since it would be brittle
    /// to manifest authors who interpolate the placeholder into a longer
    /// arg (none currently do).
    fn substitute_arg(
        mut manifest: PluginManifest,
        placeholder: &str,
        replacement: &str,
    ) -> PluginManifest {
        for arg in manifest.transport.args.iter_mut() {
            if arg == placeholder {
                *arg = replacement.to_string();
            }
        }
        manifest
    }

    /// Copies the read-only `tests/fixtures/rust-project` fixture into a
    /// freshly-created tmpdir, returning the absolute path to the copy.
    /// Per WO-0074 `scope_in` lesson §executor #5: the fixture must be
    /// copied into a tmpdir BEFORE invoking the upstream binary so any
    /// side-files the MCP server writes do not pollute the read-only
    /// fixture tree (forbidden_paths in WO-0075).
    fn copy_fixture_to_tmpdir(name: &str, fixture: &str) -> PathBuf {
        let tmp = tempfile::Builder::new()
            .prefix(name)
            .tempdir()
            .expect("create tmpdir for fixture copy");
        let dst = tmp.path().join("fixture-copy");
        let src = repo_root().join(fixture);
        copy_dir_recursive(&src, &dst).expect("copy fixture into tmpdir");
        let path = dst.clone();
        // Leak the TempDir so the directory persists for the duration
        // of the test (the spawned MCP server keeps a handle on it).
        // tokio runtime tear-down at test-end reclaims the tmpfs space
        // when the OS unlinks the orphaned dir.
        std::mem::forget(tmp);
        path
    }

    /// Manual recursive directory copy (std has no recursive `fs::copy`).
    fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let from = entry.path();
            let to = dst.join(entry.file_name());
            let ty = entry.file_type()?;
            if ty.is_dir() {
                copy_dir_recursive(&from, &to)?;
            } else if ty.is_file() {
                fs::copy(&from, &to)?;
            }
            // Symlinks are ignored — the rust-project fixture has none.
        }
        Ok(())
    }

    /// Asserts the tmpdir copy IS a git repo (has a .git directory).
    /// The Git MCP server requires a git-repo path; `tests/fixtures/
    /// rust-project` is committed AS a git submodule-style snapshot so
    /// the .git/ subdirectory survives the recursive copy. If the copy
    /// pipeline ever changes shape, this assertion catches it loudly.
    fn assert_is_git_repo(path: &Path) {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("rev-parse")
            .arg("--git-dir")
            .output()
            .expect("invoke git rev-parse");
        assert!(
            status.status.success(),
            "(SA0) tmpdir copy at {path:?} is not a git repo per `git rev-parse --git-dir`; \
             stderr: {}",
            String::from_utf8_lossy(&status.stderr),
        );
    }

    /// Initializes a small git repo in `path` if it isn't one already.
    /// Used as a fallback when the rust-project fixture is checked into
    /// the parent repo as plain files (no nested .git dir survives the
    /// outer commit). Adds a single commit so `git_log` has something
    /// to show.
    fn init_git_repo_if_needed(path: &Path) {
        // Try `git -C <path> rev-parse --git-dir`; if it fails, init.
        let probe = Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("rev-parse")
            .arg("--git-dir")
            .output()
            .expect("invoke git rev-parse");
        if probe.status.success() {
            return;
        }
        let init = Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("init")
            .arg("-q")
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed in {path:?}");
        let add = Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("add")
            .arg("-A")
            .output()
            .expect("git add");
        assert!(add.status.success(), "git add failed in {path:?}");
        // Use deterministic author so `git_log` output is stable.
        let commit = Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("-c")
            .arg("user.name=ucil-test")
            .arg("-c")
            .arg("user.email=ucil-test@example.invalid")
            .arg("commit")
            .arg("-q")
            .arg("-m")
            .arg("ucil-wo-0075 test commit")
            .output()
            .expect("git commit");
        assert!(
            commit.status.success(),
            "git commit failed in {path:?}: stderr={}",
            String::from_utf8_lossy(&commit.stderr),
        );
    }

    /// Creates a fresh tmpdir, populates it with 3 small known-content
    /// text files, and returns the absolute path. Used by the Filesystem
    /// MCP test which needs an allow-list path with predictable content.
    /// Per WO-0075 `scope_in` #9: do NOT copy fixtures here — fabricate
    /// the test inputs from scratch to keep the test hermetic.
    fn fabricate_filesystem_tmpdir(name: &str) -> PathBuf {
        let tmp = tempfile::Builder::new()
            .prefix(name)
            .tempdir()
            .expect("create tmpdir for filesystem allow-list");
        let dir = tmp.path().to_path_buf();
        fs::write(dir.join("hello.txt"), "hello, ucil!\n").expect("write hello.txt");
        fs::write(dir.join("readme.md"), "# G6 Filesystem Test\n").expect("write readme.md");
        fs::write(dir.join("data.json"), "{\"k\":\"v\"}\n").expect("write data.json");
        std::mem::forget(tmp);
        dir
    }

    #[tokio::test]
    async fn github_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/platform/github/plugin.toml");
        let manifest = PluginManifest::from_path(&manifest_path).expect("parse github plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the npx cost).
        assert_eq!(manifest.plugin.name, "github");
        assert_eq!(manifest.transport.kind, "stdio");
        assert!(
            !manifest.capabilities.provides.is_empty(),
            "github manifest must declare at least one provided capability",
        );
        assert!(
            manifest
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("platform.")),
            "github manifest must declare its capabilities under the platform.* namespace, got: {:?}",
            manifest.capabilities.provides,
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check github MCP server");

        assert_eq!(health.name, "github");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "github health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "github advertised zero tools");
        // `list_pull_requests` is the canonical PR-list tool advertised
        // by `@modelcontextprotocol/server-github@2025.4.8` (alongside
        // 25 other tools — full live list captured in the manifest's
        // top-of-file rustdoc comment). The master-plan §3.1 line 314
        // + §4.6 line 348 + §5.6 vocabulary maps directly:
        //   PR list → list_pull_requests
        // Pinning on `list_pull_requests` (the upstream literal that
        // matches our declared `platform.github.list_pull_requests`
        // capability verbatim) gives the strongest detection signal
        // for upstream renames mirroring the WO-0072 / WO-0074
        // rationale. Note: snake_case `list_pull_requests` is the
        // upstream literal as emitted by `tools/list` — preferred over
        // kebab-case translation per WO-0074 scope_in #1.
        // The `tools/list` round-trip works WITHOUT a PAT — the
        // upstream returns the static catalog regardless of token
        // presence. The API-key-gated `tools/call` smoke lives in
        // scripts/verify/P3-W10-F05.sh, not here, since cargo-test
        // must be reproducible without operator-state env vars.
        assert!(
            health.tools.iter().any(|t| t == "list_pull_requests"),
            "(SA1) expected `list_pull_requests` tool in advertised set; got: {:?}",
            health.tools,
        );
    }

    #[tokio::test]
    async fn git_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/platform/git/plugin.toml");
        let manifest_raw =
            PluginManifest::from_path(&manifest_path).expect("parse git plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the uvx cost).
        assert_eq!(manifest_raw.plugin.name, "git");
        assert_eq!(manifest_raw.transport.kind, "stdio");
        assert!(
            !manifest_raw.capabilities.provides.is_empty(),
            "git manifest must declare at least one provided capability",
        );
        assert!(
            manifest_raw
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("platform.")),
            "git manifest must declare its capabilities under the platform.* namespace, got: {:?}",
            manifest_raw.capabilities.provides,
        );

        // Substitute `${UCIL_GIT_MCP_REPO}` with a tmpdir copy of the
        // rust-project fixture (WO-0074 §executor #5 — copy BEFORE
        // invoking). The fixture is checked into the parent repo as
        // plain files, so the inner .git dir does NOT survive the
        // outer commit; we re-init a one-commit git repo inside the
        // tmpdir copy so `git_log` has commits to show.
        let repo_dir = copy_fixture_to_tmpdir("ucil-wo-0075-git-", "tests/fixtures/rust-project");
        init_git_repo_if_needed(&repo_dir);
        assert_is_git_repo(&repo_dir);

        let manifest = substitute_arg(
            manifest_raw,
            "${UCIL_GIT_MCP_REPO}",
            repo_dir.to_str().expect("repo_dir is utf-8"),
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check git MCP server");

        assert_eq!(health.name, "git");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "git health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "git advertised zero tools");
        // `git_log` is the canonical commit-history tool advertised by
        // `mcp-server-git@2026.1.14` (alongside 11 other tools — full
        // live list captured in the manifest's top-of-file rustdoc
        // comment). The master-plan §3.1 line 315 + §4.6 line 349
        // vocabulary maps directly:
        //   git log → git_log
        // Pinning on `git_log` (the upstream literal that matches our
        // declared `platform.git.log` capability verbatim) gives the
        // strongest detection signal for upstream renames.
        // Note: snake_case `git_log` is the upstream literal as emitted
        // by `tools/list` — preferred over kebab-case translation per
        // WO-0074 scope_in #1.
        assert!(
            health.tools.iter().any(|t| t == "git_log"),
            "(SA2) expected `git_log` tool in advertised set; got: {:?}",
            health.tools,
        );
    }

    #[tokio::test]
    async fn filesystem_manifest_health_check() {
        if skip_via_env() {
            return;
        }
        let manifest_path = repo_root().join("plugins/platform/filesystem/plugin.toml");
        let manifest_raw =
            PluginManifest::from_path(&manifest_path).expect("parse filesystem plugin.toml");

        // Manifest sanity (cheap pre-flight before paying the npx cost).
        assert_eq!(manifest_raw.plugin.name, "filesystem");
        assert_eq!(manifest_raw.transport.kind, "stdio");
        assert!(
            !manifest_raw.capabilities.provides.is_empty(),
            "filesystem manifest must declare at least one provided capability",
        );
        assert!(
            manifest_raw
                .capabilities
                .provides
                .iter()
                .all(|c| c.starts_with("platform.")),
            "filesystem manifest must declare its capabilities under the platform.* namespace, got: {:?}",
            manifest_raw.capabilities.provides,
        );

        // Fabricate a fresh tmpdir with known-content files (per
        // WO-0075 scope_in #9 — NOT a fixture copy; the tmpdir is
        // populated from scratch to keep the test hermetic).
        let allow_dir = fabricate_filesystem_tmpdir("ucil-wo-0075-fs-");

        let manifest = substitute_arg(
            manifest_raw,
            "${UCIL_FS_MCP_ALLOWED_PATH}",
            allow_dir.to_str().expect("allow_dir is utf-8"),
        );

        let health = PluginManager::health_check_with_timeout(&manifest, FIRST_RUN_TIMEOUT_MS)
            .await
            .expect("health-check filesystem MCP server");

        assert_eq!(health.name, "filesystem");
        assert_eq!(
            health.status,
            HealthStatus::Ok,
            "filesystem health-check returned non-Ok status: {:?}",
            health.status,
        );
        assert!(!health.tools.is_empty(), "filesystem advertised zero tools",);
        // `read_file` is the canonical read tool advertised by
        // `@modelcontextprotocol/server-filesystem@2026.1.14`
        // (alongside 13 other tools — full live list captured in the
        // manifest's top-of-file rustdoc comment). The master-plan
        // §3.1 line 316 + §4.6 line 350 vocabulary maps directly:
        //   fs.read_file → read_file
        // Pinning on `read_file` (the upstream literal that matches
        // our declared `platform.fs.read_file` capability verbatim)
        // gives the strongest detection signal for upstream renames.
        // Note: snake_case `read_file` is the upstream literal as
        // emitted by `tools/list` — preferred over kebab-case
        // translation per WO-0074 scope_in #1.
        assert!(
            health.tools.iter().any(|t| t == "read_file"),
            "(SA3) expected `read_file` tool in advertised set; got: {:?}",
            health.tools,
        );
    }
}
