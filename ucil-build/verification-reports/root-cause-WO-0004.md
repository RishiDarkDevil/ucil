# Root Cause Analysis: WO-0004 (init pipeline + CI)

**Analyst session**: rca-WO-0004-20260415
**Feature**: WO-0004 (features P0-W1-F04, P0-W1-F05, P0-W1-F06, P0-W1-F08)
**Attempts before RCA**: 1 (retry 1, verifier session `vrf-41a07ee5`)
**Branch inspected**: `feat/WO-0004-init-pipeline-and-ci` @ HEAD `42d3270`

---

## Failure pattern

Single rejection. All 6 acceptance criteria PASSED (tests green, clippy clean,
P0-W1-F08.sh green). Rejection was a **code-quality blocker**, not a test failure:
the verifier correctly identified that `.output().await` in `verify_plugin_health()`
has no `tokio::time::timeout` wrapper, violating the project-wide invariant.

The same finding was raised as **B1 (blocker)** by the critic report
(`ucil-build/critic-reports/WO-0004.md`) before the verifier ran, but the
executor wrote the ready-for-review marker at commit `42d3270` **without
addressing B1**. The verifier independently rediscovered the same defect.

---

## Root cause (confidence: 100%)

**File**: `crates/ucil-cli/src/commands/init.rs:178–182`
**Commit**: `a5a5470` (`feat(cli): add LlmProvider …`)

```rust
// lines 178-182  — in verify_plugin_health()
let kind = match tokio::process::Command::new(bin)
    .arg("--version")
    .output()        // ← awaited on line 181 with NO tokio::time::timeout
    .await
{
    Ok(out) if out.status.success() => PluginStatusKind::Ok,
    _ => PluginStatusKind::Degraded,
};
```

The `.output().await` probe runs for each of 6 P0 binaries
(`serena`, `rust-analyzer`, `pyright`, `ruff`, `eslint`, `shellcheck`) with no
bound on how long it may block. The project invariant in `CLAUDE.md` is
unambiguous:

> All async code uses `tokio::time::timeout` on any await that touches IO.

`tokio` is declared in the workspace `Cargo.toml` with `features = ["full"]`
(root `Cargo.toml:26`), so `tokio::time::timeout` is already available — **no
Cargo.toml changes are required**.

---

## Remediation (primary)

**Who**: executor
**What**: In `crates/ucil-cli/src/commands/init.rs`, add a named timeout
constant before `verify_plugin_health()` and wrap the `.output().await` in
`tokio::time::timeout`:

```rust
/// Maximum time to wait for a single plugin binary to respond to `--version`.
const PLUGIN_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub async fn verify_plugin_health() -> Vec<PluginStatus> {
    let mut statuses = Vec::with_capacity(P0_PLUGINS.len());
    for &bin in P0_PLUGINS {
        let output_result = tokio::time::timeout(
            PLUGIN_PROBE_TIMEOUT,
            tokio::process::Command::new(bin).arg("--version").output(),
        )
        .await;
        let kind = match output_result {
            Ok(Ok(out)) if out.status.success() => PluginStatusKind::Ok,
            _ => PluginStatusKind::Degraded,   // timeout, not-found, or non-zero exit
        };
        statuses.push(PluginStatus {
            name: bin.to_owned(),
            status: kind,
        });
    }
    statuses
}
```

**Acceptance**: All 6 existing acceptance criteria remain green. No additional
tests are required — `test_plugin_health_verification` (init.rs:423) already
calls `verify_plugin_health()` directly and will exercise the timeout-wrapped
path. Confirm `cargo clippy -p ucil-cli -- -D warnings` and
`cargo nextest run -p ucil-cli` both exit 0.

**Risk**: None — a 5-second timeout per binary is generous for a `--version`
call. Timed-out probes fall through to `PluginStatusKind::Degraded`, which is
already the graceful-degradation path. No behavioural change for healthy
installations.

---

## Secondary concerns (non-blocking for retry, should be addressed)

### S1 — Critic blockers must be fixed before writing ready-for-review marker

The critic report (`ucil-build/critic-reports/WO-0004.md`, verdict **BLOCKED**)
was committed before the executor wrote the ready-for-review marker at commit
`42d3270`. The executor wrote the marker anyway. This caused an avoidable
verifier rejection and wasted an attempt counter.

**Process fix**: The executor **must** address all critic blockers (items under
"Blockers — must fix before verifier") before writing the ready-for-review
marker. Warnings (W1–W5) are advisory; blockers are mandatory.

### S2 — Mutation check inconclusive for P0-W1-F04

`scripts/reality-check.sh P0-W1-F04` reports INCONCLUSIVE because the
acceptance tests live in `#[cfg(test)]` modules inside `init.rs` itself. When
the script stashes `init.rs`, the test functions disappear with the
implementation, so nextest finds 0 matching tests and exits 0 (the script
correctly flags this as inconclusive rather than claiming a false pass).

This is a known limitation of co-located unit tests. The verifier noted it as
a secondary concern, not the primary rejection reason. No fix required for this
retry — the mutation check inconclusive result does not trigger an escalation
escalation rule on its own (the primary rejection was B1).

If UCIL style guidelines later require integration-tier tests in `tests/` for
mutation-check compatibility, that is a separate ADR decision.

### S3 — CI uses `cargo test` instead of `cargo nextest`

`.github/workflows/ci.yml:25` runs `cargo test --workspace`. The project
standard (`rust-style.md`) is `cargo nextest`. Recommend switching to
`cargo nextest run --workspace` in the CI workflow to align with the project
default runner (per-test timeouts, retry support, better output). This was
critic warning W3.

### S4 — `uv sync --all-extras` vs `--all-packages`

`.github/workflows/ci.yml:74` runs `uv sync --all-extras` in the `ml/`
working directory. The WO implementation notes specified `--all-packages`, which
syncs every package in a uv workspace. If `ml/` becomes a multi-package uv
workspace, `--all-extras` will silently skip additional packages. Recommend
changing to `uv sync --all-packages` to be future-proof. This was critic
warning W4.

---

## Process note

The **sole hard blocker** is S2's underlying cause — B1 was known, documented,
and not fixed. The executor should fix `init.rs:178–182` as specified above,
commit (one commit, ≤50 lines, conventional commit trailer `Feature: P0-W1-F05`
since `verify_plugin_health()` is the F05 implementation), and push. No other
source file requires changes.

Optionally the executor may also fix S3 (swap `cargo test` → `cargo nextest run`
in ci.yml) and S4 (`--all-extras` → `--all-packages`) in the same or a
follow-up commit, but these are not required for the verifier to pass.

---

*Generated by root-cause-finder on 2026-04-15.*
