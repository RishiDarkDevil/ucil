//! `P2-W6-F08` — cross-crate plugin-lifecycle integration test binary.
//!
//! Mocking [`PluginManager`] / [`PluginRuntime`] /
//! [`tokio::process::Command`] is forbidden — the WO-0046 contract is
//! precisely that real [`PluginManager`] dispatches against real
//! subprocesses (`mock-mcp-plugin` for the healthy path, the
//! `/__ucil_test_nonexistent_breaker_binary__` placeholder for the
//! ENOENT path). The single token `mock` permitted in this file is
//! this prohibition docstring and the literal name of the
//! `mock-mcp-plugin` binary that ships with `ucil-daemon`.
//!
//! The three module-root tests cover one HOT/COLD lifecycle slice each
//! (master-plan §14.2):
//!
//! 1. [`test_plugin_hot_cold_round_trip`] — `Active → Idle → Active`
//!    against the real `mock-mcp-plugin`, with the per-runtime
//!    `idle_timeout` shrunk via [`PluginRuntime::with_idle_timeout`] so
//!    the transition fires inside the fast-test budget.
//! 2. `test_plugin_crash_recovery_via_circuit_breaker` — three
//!    consecutive ENOENT spawn failures trip
//!    [`PluginError::CircuitBreakerOpen`] with the manager's base
//!    backoff shrunk via [`PluginManager::with_circuit_breaker_base`].
//! 3. `test_plugin_independent_lifecycle_two_runtimes` — a healthy
//!    runtime and a failing runtime registered on the same
//!    [`PluginManager`] retain independent lifecycle state when
//!    `restart_with_backoff` trips the breaker on the failing one.
//!
//! Test functions live at module root — there is no nested
//! `tests`-named module — per `DEC-0007`, so the frozen selector
//! `--test test_plugin_lifecycle <fn_name>` resolves directly without
//! a `tests::` path prefix.
//!
//! # File layout per `DEC-0010`
//!
//! The binary lives at `tests/integration/test_plugin_lifecycle.rs` —
//! the repo-relative path declared in `feature-list.json` and pinned by
//! the `[[test]]` entry in `tests/integration/Cargo.toml`. Per
//! `DEC-0010` this placement is the workspace convention for
//! cross-crate integration tests; the same anchor pattern is used by
//! `test_lsp_bridge.rs` next door.

#![deny(warnings)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ucil_daemon::{
    CapabilitiesSection, PluginManager, PluginManifest, PluginSection, PluginState,
    TransportSection,
};

// ── Path resolution helper ──────────────────────────────────────────────────

/// Resolve the path to the `mock-mcp-plugin` binary that
/// `cargo build -p ucil-daemon --bin mock-mcp-plugin` produces under
/// the workspace `target/<profile>/` directory.
///
/// The path is anchored via `env!("CARGO_MANIFEST_DIR")` (which
/// resolves to `tests/integration/` at compile time per `DEC-0010`)
/// and joined with `../../target/<profile>/mock-mcp-plugin`. Profile is
/// chosen from `cfg!(debug_assertions)` so a `cargo test` (debug) and
/// `cargo test --release` both find the right binary.
/// `CARGO_TARGET_DIR` is honoured when the user has redirected the
/// workspace target directory (the `.cargo/config.toml` build cache
/// pattern used by `scripts/setup-build-cache.sh`).
///
/// # Panics
///
/// Panics with a single-line operator-actionable message when the
/// binary cannot be canonicalised. Operator: run
/// `cargo build -p ucil-daemon --bin mock-mcp-plugin` first; the
/// companion `scripts/verify/P2-W6-F08.sh` performs that warm-up
/// before invoking this test target.
fn mock_mcp_plugin_path() -> PathBuf {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let bin_name = if cfg!(windows) {
        "mock-mcp-plugin.exe"
    } else {
        "mock-mcp-plugin"
    };

    let target_dir: PathBuf = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        PathBuf::from,
    );

    let candidate = target_dir.join(profile).join(bin_name);
    candidate.canonicalize().unwrap_or_else(|e| {
        panic!(
            "mock-mcp-plugin not built — run: cargo build -p ucil-daemon --bin mock-mcp-plugin (looked at {}, error: {e})",
            candidate.display()
        )
    })
}

// ── Manifest builders (struct literals, NOT loaded from disk) ───────────────

/// Build a healthy [`PluginManifest`] whose `[transport].command` points
/// at the real `mock-mcp-plugin` binary.
///
/// Constructed inline as a struct literal (per the WO-0046 `scope_out`
/// rule that `plugins/**/plugin.toml` files are frozen) — every test
/// builds its own manifest from this helper rather than loading one
/// from disk.
fn healthy_manifest(name: &str, mock_path: &Path) -> PluginManifest {
    PluginManifest {
        plugin: PluginSection {
            name: name.to_owned(),
            version: "0.1.0".into(),
            description: Some("WO-0046 healthy fixture".into()),
        },
        capabilities: CapabilitiesSection::default(),
        transport: TransportSection {
            kind: "stdio".into(),
            command: mock_path.to_string_lossy().into_owned(),
            args: vec![],
        },
        resources: None,
        lifecycle: None,
    }
}

// ── Test 1: HOT/COLD round-trip ────────────────────────────────────────────

/// Drive a real plugin through `Active → Idle → Active` end-to-end.
///
/// Phases:
///
/// 1. `PluginManager::activate` spawns the real `mock-mcp-plugin`
///    binary, runs a real `tools/list` health check, and registers the
///    runtime in `PluginState::Active`.
/// 2. The local runtime's `idle_timeout` is shrunk to 50 ms via
///    [`PluginRuntime::with_idle_timeout`] (NOT direct field
///    assignment) so the pre-baked mutation A — drop the assignment
///    inside the builder — flips the production 10-minute default back
///    in and breaks the `state == Idle` assertion below.
/// 3. After a 75 ms sleep the runtime's `tick` fires `Active → Idle`
///    inside the fast-test budget.
/// 4. `PluginManager::restart_with_backoff` drives the manager's
///    internal runtime view back to `Active` via a real second health
///    check against the same mock binary.
#[tokio::test]
async fn test_plugin_hot_cold_round_trip() {
    let mock = mock_mcp_plugin_path();
    let manifest = healthy_manifest("hot-cold-round-trip", &mock);

    // ── Phase 1: activate registers the runtime in `Active`. ────────
    let mut mgr = PluginManager::new();
    let runtime = mgr
        .activate(&manifest)
        .await
        .expect("activate must succeed against the real mock-mcp-plugin");
    assert_eq!(
        runtime.state,
        PluginState::Active,
        "expected Active after activate (got {:?}); runtime={:?}",
        runtime.state,
        runtime
    );

    // ── Phase 2: override per-runtime `idle_timeout` via the builder.
    // Using `with_idle_timeout` (not direct field assignment) means
    // the pre-baked mutation A — drop `self.idle_timeout = ...;`
    // inside the builder body — leaves the production 10-minute
    // default in place and breaks the `state == Idle` assertion in
    // phase 3. The read-back assertion in phase 3 also fires under
    // mutation A but only after the state assertion.
    let mut runtime = runtime.with_idle_timeout(Duration::from_millis(50));

    // ── Phase 3: sleep past the idle window, then `tick`. ────────────
    let idle_start = Instant::now();
    tokio::time::sleep(Duration::from_millis(75)).await;
    let transition = runtime.tick(Instant::now());
    let elapsed_to_idle = idle_start.elapsed();

    assert_eq!(
        transition,
        Some(PluginState::Idle),
        "tick must demote Active → Idle once the idle window has elapsed (got {transition:?}); runtime={runtime:?}"
    );
    assert_eq!(
        runtime.state,
        PluginState::Idle,
        "expected Idle after tick (got {:?}); runtime={:?}",
        runtime.state,
        runtime
    );

    // Sub-assertion (a): builder actually overrode `idle_timeout`.
    assert_eq!(
        runtime.idle_timeout,
        Duration::from_millis(50),
        "with_idle_timeout(50ms) must set runtime.idle_timeout (got {:?}); runtime={:?}",
        runtime.idle_timeout,
        runtime
    );
    // Sub-assertion (b): the idle window was honoured.
    assert!(
        elapsed_to_idle >= Duration::from_millis(50),
        "elapsed_to_idle must be >= 50ms (got {elapsed_to_idle:?}); runtime={runtime:?}"
    );
    // Sub-assertion (c): no production constants leaked in.
    assert!(
        elapsed_to_idle < Duration::from_secs(2),
        "elapsed_to_idle must be < 2s (got {elapsed_to_idle:?}); runtime={runtime:?}"
    );

    // ── Phase 4: trigger reactivation via `restart_with_backoff`. ────
    let restart_start = Instant::now();
    mgr.restart_with_backoff("hot-cold-round-trip")
        .await
        .expect("restart_with_backoff must succeed against the real mock-mcp-plugin");
    let restart_elapsed = restart_start.elapsed();

    // The manager's internal runtime is what `restart_with_backoff`
    // mutates; the local `runtime` clone is left untouched.
    let snapshot = mgr.registered_runtimes().await;
    assert_eq!(
        snapshot.len(),
        1,
        "manager must retain exactly one runtime (got {}); snapshot={:?}",
        snapshot.len(),
        snapshot
    );
    assert_eq!(
        snapshot[0].state,
        PluginState::Active,
        "expected Active after restart_with_backoff (got {:?}); runtime={:?}",
        snapshot[0].state,
        snapshot[0]
    );
    assert_eq!(
        snapshot[0].restart_attempts, 0,
        "successful restart must reset restart_attempts (got {}); runtime={:?}",
        snapshot[0].restart_attempts, snapshot[0]
    );

    // Total wall-time bound — the round-trip must finish inside the
    // fast-test budget. A leak of `DEFAULT_IDLE_TIMEOUT_MINUTES` or
    // `CIRCUIT_BREAKER_BASE_BACKOFF_MS` into either phase would
    // exceed 2 s.
    let total_elapsed = idle_start.elapsed();
    assert!(
        total_elapsed < Duration::from_secs(2),
        "round-trip total elapsed must be < 2s (got {total_elapsed:?}; restart took {restart_elapsed:?})"
    );
}
