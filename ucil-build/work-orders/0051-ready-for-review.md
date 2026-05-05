# WO-0051 — ready-for-review

**Feature**: P2-W7-F07 (ripgrep plugin manifest + smoke)
**Branch**: `feat/WO-0051-ripgrep-plugin-manifest-and-smoke`
**Final commit (pre-marker)**: `82c5c71f32bd2292dd13b74b19809a4579a67c24`
**Worktree**: `../ucil-wt/WO-0051`

## What I verified locally

- **AC01–AC03** — `plugins/search/ripgrep/plugin.toml`, `scripts/verify/P2-W7-F07.sh`, `scripts/devtools/install-ripgrep.sh` all exist; the two scripts are executable.
- **AC04–AC06** — manifest `[plugin].name = "ripgrep"`; `provides = ["search.text"]` exact form; no `"main"` moving-ref string anywhere in the manifest.
- **AC07** — `cargo test -p ucil-daemon --test plugin_manifests plugin_manifests::ripgrep_manifest_parses -- --nocapture` exits 0; cargo summary `test result: ok. 1 passed; 0 failed` matches the WO-0042 alternation regex.
- **AC08** — `grep -nE '^\s*fn ripgrep_manifest_parses' crates/ucil-daemon/tests/plugin_manifests.rs` returns exactly one line at `tests/plugin_manifests.rs:157`, inside the existing `mod plugin_manifests { ... }` wrapper. The `#[test]` attribute is on the line immediately above.
- **AC09** — `bash scripts/verify/P2-W7-F07.sh` exits 0 with `[OK] P2-W7-F07` final line. All `[INFO]` checkpoints (`rg --version`, manifest present, cargo test PASS, rg --json structural markers PASS, .gitignore-respect PASS) print as expected.
- **AC10** — `rg --json 'fn evaluate' tests/fixtures/rust-project` produces JSON output containing `"type":"match"`, `"path"`, and `fn evaluate`. Confirmed via the verify-script's three `grep -q` checks.
- **AC11** — `rg --files-with-matches 'fn evaluate' .` at the repo root returns `tests/fixtures/rust-project/src/util.rs` and contains zero entries matching `(^|/)target/`. The workspace was warmed via `cargo build --workspace --quiet` before the check so `target/` exists on disk.
- **AC12** — WO-0044 regression sentinels: both `plugin_manifests::ast_grep_manifest_health_check` and `plugin_manifests::probe_manifest_health_check` still pass (real `npx -y` MCP-server round-trips; this WO did not touch the existing manifests).
- **AC13** — WO-0042/WO-0043 regression sentinels: `plugin_manager::test_manifest_parser`, `plugin_manager::test_lifecycle_state_machine`, `plugin_manager::test_hot_reload`, `plugin_manager::test_circuit_breaker`, `plugin_manager::test_hot_cold_lifecycle` all pass (5 passed; 0 failed).
- **AC14** — Phase-1 e2e regressions: `cargo test -p ucil-daemon --test e2e_mcp_stdio --test e2e_mcp_with_kg` exits 0 (1 passed each, 0 failed).
- **AC15** — `cargo test --workspace --no-fail-fast` exits 0 across every crate; zero failures observed in the summary lines.
- **AC16** — `cargo clippy -p ucil-daemon --all-targets -- -D warnings` exits 0.
- **AC17** — `cargo fmt --check` exits 0 (the post-tool-use formatter touched `tests/plugin_manifests.rs` after my initial Edit; final state is rustfmt-clean).
- **AC18** — `shellcheck` is not on PATH on this build host; the verify script's preamble prints `[INFO] shellcheck not on PATH; skipping shellcheck step.` and continues, matching the WO-0044 fallback convention.
- **AC19** (mutation #1 — `provides = []`): `sed -i 's|provides = \["search.text"\]|provides = []|' plugins/search/ripgrep/plugin.toml`. Re-run of `cargo test plugin_manifests::ripgrep_manifest_parses` panics at `tests/plugin_manifests.rs:167:9` with `capabilities.provides must include search.text; observed []`. Restored via `git checkout --`; greens.
- **AC20** (mutation #2 — `name = "x"`): `sed -i 's|^name = "ripgrep"|name = "x"|' plugins/search/ripgrep/plugin.toml`. Re-run panics at `tests/plugin_manifests.rs:162:9` with `assertion left == right failed: plugin.name must be exactly ripgrep; observed x`. Restored; greens.
- **AC21** (mutation #3 — drop `--json`): `sed -i "s|rg --json 'fn evaluate'|rg 'fn evaluate'|" scripts/verify/P2-W7-F07.sh`. Re-run of `bash scripts/verify/P2-W7-F07.sh` fails with `[FAIL] P2-W7-F07: ripgrep --json output missing structural marker '"type":"match"'`. Restored; greens.
- **AC22** — `git diff --name-only main...HEAD` (post-marker) lists only the 5 expected paths: `crates/ucil-daemon/tests/plugin_manifests.rs`, `plugins/search/ripgrep/plugin.toml`, `scripts/devtools/install-ripgrep.sh`, `scripts/verify/P2-W7-F07.sh`, `ucil-build/work-orders/0051-ready-for-review.md`.
- **AC23–AC26** — Negative diff checks all return empty: no `Cargo.toml` / `Cargo.lock` / `rust-toolchain.toml` changes, no `tests/fixtures/**` changes, no `feature-list.json` / `feature-list.schema.json` changes, no master-plan changes.
- **AC27** — Stub-scan on the added-line diff (`grep -E '^\+' /tmp/wo-0051-diff.patch | grep -E 'todo!|unimplemented!|TODO|FIXME'`) returns zero hits. The single `TODO` match in the raw diff was on a removed line (the deleted F07 stub).
- **AC28** — 4 commits on the feature branch matching the planned ladder: `feat(plugins): …manifest`, `chore(devtools): …install-ripgrep.sh`, `test(daemon): …ripgrep_manifest_parses`, `feat(verify): …P2-W7-F07.sh`. Each ≤ ~125 LOC.
- **AC29** — `git rev-parse HEAD` matches `git rev-parse @{u}` after each commit's push; `git status --porcelain` empty before this marker.

## Notes for the verifier

- The `[transport]` table in `plugins/search/ripgrep/plugin.toml` is a declarative sentinel per DEC-0009; the daemon's `text_search.rs` (WO-0035) runs ripgrep in-process and never spawns this transport as an MCP server. Calling `PluginManager::health_check` on the manifest would correctly fail because `rg --version` exits without speaking JSON-RPC — that is the desired behaviour, not a bug.
- The new test lives inside the existing `mod plugin_manifests { … }` wrapper at `crates/ucil-daemon/tests/plugin_manifests.rs:36` (per the WO-0044 convention required by nextest's `--test plugin_manifests <selector>` resolution). DEC-0007 module-root placement is honoured at the wrapper module's root.
- The verify script invokes `cargo build --workspace --quiet` before the `.gitignore`-respect sub-check to ensure `target/` exists; this is intentional so the assertion has something to (correctly) skip. Subsequent runs hit the cached target dir and the build is a fast no-op.
