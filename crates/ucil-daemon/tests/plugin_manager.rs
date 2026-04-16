//! End-to-end integration tests for the `plugin_manager` skeleton.
//!
//! These tests exercise [`PluginManager::spawn`] and
//! [`PluginManager::health_check`] against a **real** subprocess — the
//! `mock-mcp-plugin` binary co-located under `tests/support/`.  Mocking
//! `tokio::process::Command` or the child's stdio is explicitly
//! forbidden by this crate's invariants; every test here launches an
//! actual OS process.
//!
//! The cargo build sets `CARGO_BIN_EXE_mock-mcp-plugin` at test-compile
//! time so the path to the built binary is available to integration
//! tests at runtime without hand-rolling a target-dir lookup.
//!
//! The tests are wrapped in a `mod plugin_manager` block so nextest
//! reports them under `plugin_manager::*` — matching the Work-Order-0009
//! acceptance selector `cargo nextest run -p ucil-daemon plugin_manager::`.
//!
//! [`PluginManager::spawn`]: ucil_daemon::PluginManager::spawn
//! [`PluginManager::health_check`]: ucil_daemon::PluginManager::health_check

mod plugin_manager {
    use std::path::PathBuf;

    use ucil_daemon::{HealthStatus, PluginError, PluginManager, PluginManifest};

    /// Absolute path to the compiled `mock-mcp-plugin` binary.
    fn mock_plugin_path() -> PathBuf {
        PathBuf::from(env!("CARGO_BIN_EXE_mock-mcp-plugin"))
    }

    /// Write a plugin.toml manifest whose `transport.command` points at
    /// the compiled mock binary and return the manifest path.
    fn write_mock_manifest(tmp: &tempfile::TempDir, name: &str) -> PathBuf {
        let manifest_path = tmp.path().join(format!("{name}.toml"));
        let body = format!(
            r#"[plugin]
name = "{name}"
version = "0.1.0"
description = "In-tree mock plugin for integration tests."

[transport]
type = "stdio"
command = "{cmd}"
"#,
            name = name,
            cmd = mock_plugin_path().display(),
        );
        std::fs::write(&manifest_path, body).expect("write manifest");
        manifest_path
    }

    #[tokio::test]
    async fn spawn_and_health_check_returns_mock_tools() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest_path = write_mock_manifest(&tmp, "mock-integration");
        let manifest = PluginManifest::from_path(&manifest_path).expect("parse");

        let health = PluginManager::health_check(&manifest)
            .await
            .expect("health_check");

        assert_eq!(health.name, "mock-integration");
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(
            health.tools,
            vec!["echo".to_owned(), "reverse".to_owned()],
            "mock plugin advertises exactly these two tools"
        );
    }

    #[tokio::test]
    async fn discover_finds_mock_manifest_and_health_check_succeeds() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _ = write_mock_manifest(&tmp, "discover-a");
        let _ = write_mock_manifest(&tmp, "discover-b");

        let manifests = PluginManager::discover(tmp.path()).expect("discover");
        assert_eq!(manifests.len(), 2, "expected to find both manifests");
        assert_eq!(manifests[0].plugin.name, "discover-a");
        assert_eq!(manifests[1].plugin.name, "discover-b");

        // Drive a real health check on one of the discovered manifests
        // to prove the whole chain (discover → from_path → spawn →
        // tools/list) works end-to-end.
        let health = PluginManager::health_check(&manifests[0])
            .await
            .expect("health_check on discovered manifest");
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(health.tools.len(), 2);
    }

    #[tokio::test]
    async fn spawn_fails_cleanly_when_command_is_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest_path = tmp.path().join("ghost.toml");
        std::fs::write(
            &manifest_path,
            r#"[plugin]
name = "ghost"
version = "0.0.0"

[transport]
type = "stdio"
command = "/definitely/not/on/this/filesystem/ghost-binary"
"#,
        )
        .unwrap();

        let manifest = PluginManifest::from_path(&manifest_path).expect("parse");

        // `spawn` returns the error directly; `health_check` propagates
        // it through the same variant.  Both code paths matter, so we
        // test both.
        let spawn_err = PluginManager::spawn(&manifest).expect_err("missing binary");
        assert!(
            matches!(spawn_err, PluginError::Spawn { .. }),
            "expected Spawn error, got {spawn_err:?}"
        );

        let hc_err = PluginManager::health_check(&manifest)
            .await
            .expect_err("missing binary");
        assert!(
            matches!(hc_err, PluginError::Spawn { .. }),
            "expected Spawn error from health_check, got {hc_err:?}"
        );
    }
}
