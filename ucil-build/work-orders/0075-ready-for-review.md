# WO-0075 — G6 Platform plugin manifests — READY FOR REVIEW

**Final commit sha**: `ea5fbd0a096567e695fd294d0c4fe43c895fc0c3`
**Branch**: `feat/WO-0075-g6-platform-plugin-manifests`
**Phase**: 3 (Week 10)
**Features**: P3-W10-F05 (GitHub MCP), P3-W10-F06 (Git MCP), P3-W10-F07 (Filesystem MCP)

## Commits (7 total)

```
ea5fbd0 feat(scripts/verify): replace P3-W10-F07 stub with real Filesystem MCP smoke
d5b2c19 feat(scripts/verify): replace P3-W10-F06 stub with real Git MCP smoke
8f7e9d2 feat(scripts/verify): replace P3-W10-F05 stub with real GitHub MCP smoke
38b1c3b feat(ucil-daemon/tests): add G6 plugin-manifest health-check integration tests
ce4079a feat(plugins/platform/filesystem): add G6 Filesystem MCP manifest + install hint
75a1bd3 feat(plugins/platform/git): add G6 Git MCP plugin manifest + install hint
501b632 feat(plugins/platform/github): add G6 GitHub MCP plugin manifest + install hint
```

## What I verified locally

### Per-feature acceptance

* `test -f plugins/platform/github/plugin.toml` — pass
* `test -f plugins/platform/git/plugin.toml` — pass
* `test -f plugins/platform/filesystem/plugin.toml` — pass
* TOML sanity for all 3 manifests (plugin.name == matching; plugin.category == "platform"; transport.type == "stdio"; capabilities.provides starts with "platform.") — pass
* `test -x scripts/devtools/install-{github,git,filesystem}-mcp.sh` — all 3 pass
* `bash scripts/devtools/install-{github,git,filesystem}-mcp.sh ; test $? -eq 0` — all 3 pass (uvx warm-up cached `mcp-server-git@2026.1.14`, npx warm-up cached `@modelcontextprotocol/server-github@2025.4.8`; filesystem warm-up `--help` unsupported but npm fetch primed cache as side-effect)
* `test -f crates/ucil-daemon/tests/g6_plugin_manifests.rs` — pass
* `grep -Eq '^[[:space:]]*(pub )?mod g6_plugin_manifests[[:space:]]*\{' crates/ucil-daemon/tests/g6_plugin_manifests.rs` — pass (`mod g6_plugin_manifests {` at line 64)
* `grep -c '#\[tokio::test\]' crates/ucil-daemon/tests/g6_plugin_manifests.rs` — 3 (>=3)

### Cargo test selectors (all green from clean state)

* `cargo test -p ucil-daemon --test g6_plugin_manifests g6_plugin_manifests::github_manifest_health_check` — `1 passed; 0 failed` in 5.79s
* `cargo test -p ucil-daemon --test g6_plugin_manifests g6_plugin_manifests::git_manifest_health_check` — `1 passed; 0 failed` in 0.48s
* `cargo test -p ucil-daemon --test g6_plugin_manifests g6_plugin_manifests::filesystem_manifest_health_check` — `1 passed; 0 failed` in 0.55s

### Verify scripts (all 3 green)

* `bash scripts/verify/P3-W10-F05.sh ; test $? -eq 0` — `[OK] P3-W10-F05` (tools/list captured 26 tools; tools/call gated on `GITHUB_PERSONAL_ACCESS_TOKEN` — `[SKIP]` since env unset)
* `bash scripts/verify/P3-W10-F06.sh ; test $? -eq 0` — `[OK] P3-W10-F06` (tools/list captured 12 tools; tools/call git_log returned 208 chars of commit-shaped output)
* `bash scripts/verify/P3-W10-F07.sh ; test $? -eq 0` — `[OK] P3-W10-F07` (tools/list captured 14 tools; tools/call read_file matched expected content; tools/call list_directory listed all 3 populated files)

### Word-ban

* `! grep -qiE 'mock|fake|stub' plugins/platform/{github,git,filesystem}/plugin.toml scripts/devtools/install-{github,git,filesystem}-mcp.sh scripts/verify/P3-W10-F0{5,6,7}.sh` — empty (production-side files clean)
* `! grep -qE 'TODO: implement acceptance test for P3-W10-F0[567]'` — empty (all 3 stubs replaced)

### Pre-flight (scope_in #15)

* `cargo build -p ucil-daemon` — clean
* `cargo test -p ucil-daemon --test g6_plugin_manifests` — all 3 frozen tests green
* `cargo clippy -p ucil-daemon --all-targets --tests -- -D warnings` — zero warnings
* `cargo fmt --all -- --check` — formatting clean
* `cargo test -p ucil-daemon --no-fail-fast` — 164 + 27 + 9 + 1 + 1 + 2 + 1 + 2 + 3 + 3 + 3 = 216 unit tests passing across 12 binaries; integration tests green; doc-tests green
* `command -v npx uvx python3` confirms PATH has all three required runtimes (npx 10.9.7; uvx 0.11.6; python3 3.13.7)

### Forbidden-path invariants (acceptance_criteria)

* `git diff main..feat/WO-0075-g6-platform-plugin-manifests -- crates/ucil-daemon/src/` — 0 lines
* `git diff main..feat/WO-0075-g6-platform-plugin-manifests -- tests/fixtures/` — 0 lines
* `git diff main..feat/WO-0075-g6-platform-plugin-manifests -- ucil-build/feature-list.json ucil-build/feature-list.schema.json ucil-master-plan-v2.1-final.md` — 0 lines

### Coverage (AC23 substantive protocol per WO-0067..WO-0074)

* `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --tests --summary-only --json | jq '.data[0].totals.lines.percent'` — **89.66%** (>= 85.0% floor; preserved at >= pre-WO baseline since this WO is purely additive on tests + manifests + scripts).

### Git log invariants

* `git log feat/WO-0075-g6-platform-plugin-manifests ^main --merges | wc -l` — 0 (no merges)
* `git log feat/WO-0075-g6-platform-plugin-manifests ^main --pretty=format:%B | grep -Eq 'Work-order: WO-0075'` — pass (all 7 commits)
* `git log feat/WO-0075-g6-platform-plugin-manifests ^main --pretty=format:%B | grep -Eq 'Feature: P3-W10-F05'` — pass (3 commits: GitHub manifest + integration test + F05 verify script)
* `git log feat/WO-0075-g6-platform-plugin-manifests ^main --pretty=format:%B | grep -Eq 'Feature: P3-W10-F06'` — pass (3 commits: Git manifest + integration test + F06 verify script)
* `git log feat/WO-0075-g6-platform-plugin-manifests ^main --pretty=format:%B | grep -Eq 'Feature: P3-W10-F07'` — pass (3 commits: Filesystem manifest + integration test + F07 verify script)

## Live tool name capture (per WO-0074 §executor #1 lesson + WO-0075 scope_in #16)

Captured via `/tmp/wo-0075-capture.py` (one-shot driver script) running each upstream binary at the pinned version, sending `initialize` + `notifications/initialized` + `tools/list` over stdio, and recording the canonical `result.tools[].name` array verbatim.

### GitHub MCP (`@modelcontextprotocol/server-github@2025.4.8`)
26 tools: `create_or_update_file`, `search_repositories`, `create_repository`, `get_file_contents`, `push_files`, `create_issue`, `create_pull_request`, `fork_repository`, `create_branch`, `list_commits`, `list_issues`, `update_issue`, `add_issue_comment`, `search_code`, `search_issues`, `search_users`, `get_issue`, `get_pull_request`, `list_pull_requests`, `create_pull_request_review`, `merge_pull_request`, `get_pull_request_files`, `get_pull_request_status`, `update_pull_request_branch`, `get_pull_request_comments`, `get_pull_request_reviews`

* **Chosen M2 literal**: `list_pull_requests` (the upstream literal that maps verbatim to our declared `platform.github.list_pull_requests` capability — strongest detection signal for upstream renames)
* **Note**: there is no upstream `list_repositories` tool — the closest is `search_repositories`. The master-plan vocabulary for the capability NAMES (`platform.github.list_repositories` etc.) is independent of the upstream literal tool names per WO-0074 §executor #1.

### Git MCP (`mcp-server-git@2026.1.14`)
12 tools: `git_status`, `git_diff_unstaged`, `git_diff_staged`, `git_diff`, `git_commit`, `git_add`, `git_reset`, `git_log`, `git_create_branch`, `git_checkout`, `git_show`, `git_branch`

* **Chosen M2 literal**: `git_log` (canonical commit-history surface; maps verbatim to our `platform.git.log` capability)
* **Note**: there is no upstream `git_blame` tool — the master-plan vocabulary's `git.blame` capability name is forward-compatible; `git_diff` / `git_log` / `git_show` together span the blame use-case.

### Filesystem MCP (`@modelcontextprotocol/server-filesystem@2026.1.14`)
14 tools: `read_file`, `read_text_file`, `read_media_file`, `read_multiple_files`, `write_file`, `edit_file`, `create_directory`, `list_directory`, `list_directory_with_sizes`, `directory_tree`, `move_file`, `search_files`, `get_file_info`, `list_allowed_directories`

* **Chosen M2 literal**: `read_file` (canonical read surface; maps verbatim to our `platform.fs.read_file` capability)

## M1 mutation contract (transport.command poison)

Per WO-0075 scope_in #10 — verifier runs three M1 mutations sequentially. For each manifest in turn:

1. `md5sum plugins/platform/<name>/plugin.toml > /tmp/wo-0075-<name>-orig.md5`
2. Edit the targeted `plugin.toml`: replace `command = "npx"` (github + filesystem) or `command = "uvx"` (git) with `command = "/__ucil_test_nonexistent_M1__"`.
3. Run the targeted health-check test under `g6_plugin_manifests`.
4. **Expected**: test fails with a `PluginError::Spawn { source: io::Error { kind: NotFound, .. } }` chain (or analogous `tokio::process::Command::spawn` ENOENT-class error).
5. Restore: `git checkout -- plugins/platform/<name>/plugin.toml`.
6. Confirm: `md5sum -c /tmp/wo-0075-<name>-orig.md5` returns OK.

## M2 mutation contract (expected-tool-name regression)

Per WO-0075 scope_in #11 — verifier runs three M2 mutations sequentially. For each test in turn:

1. `md5sum crates/ucil-daemon/tests/g6_plugin_manifests.rs > /tmp/wo-0075-g6tests-orig.md5` (one snapshot covers all three; restore between mutations)
2. Edit the test file: replace the targeted assertion literal with `"NON_EXISTENT_TOOL_M2"`:
   * GitHub: replace `"list_pull_requests"` (line ~244 in the M2-target — the assertion on `(SA1)`)
   * Git: replace `"git_log"` (line ~329 in the M2-target — the assertion on `(SA2)`)
   * Filesystem: replace `"read_file"` (line ~410 in the M2-target — the assertion on `(SA3)`)
3. Run the targeted health-check test.
4. **Expected**: structured panic carrying the full live tool-list with the SA-tag, e.g.:
   ```
   thread 'g6_plugin_manifests::github_manifest_health_check' panicked at ...:
   (SA1) expected `NON_EXISTENT_TOOL_M2` tool in advertised set; got: ["create_or_update_file", "search_repositories", ...]
   ```
5. Restore: `git checkout -- crates/ucil-daemon/tests/g6_plugin_manifests.rs`.
6. Confirm via `md5sum -c /tmp/wo-0075-g6tests-orig.md5`.

## Lessons applied

* **WO-0044** — paired-manifest template (ast-grep + probe original recipe); G6 extends to 3 features
* **WO-0069** — paired-manifest module-root placement; substring selector resolves cleanly; API-key short-circuit gate per Mem0 precedent applied to F05 `GITHUB_PERSONAL_ACCESS_TOKEN`
* **WO-0072** — M1/M2 mutation contract shape (transport.command poison + expected-tool-name regression); applied verbatim per-feature
* **WO-0073** — G3→G4→G5→G6 template port; new `g6_plugin_manifests.rs` peer file; `UCIL_SKIP_PLATFORM_PLUGIN_E2E` opt-out
* **WO-0074 §executor #1** — capture canonical names from live binary BEFORE writing assertion literals; preferred upstream literals over kebab-case translation (snake_case `list_pull_requests` / `git_log` / `read_file` exactly as emitted)
* **WO-0074 §executor #2** — used `--help` for warm-up, NOT `--mcp` (the GitHub/Git/Filesystem MCP servers ARE MCP servers by default; invoking in MCP mode would block on stdin)
* **WO-0074 §executor #3** — documented `UCIL_SKIP_PLATFORM_PLUGIN_E2E` in test rustdoc + all 3 verify scripts; defaults to RUN
* **WO-0074 §executor #4** — used python polling with deadline over bash sleep for tools/call wall-time-sensitive smokes (F05 may reach api.github.com over HTTPS; F06/F07 use python for consistency + JSON-RPC parsing reliability)
* **WO-0074 §executor #5** — copied `tests/fixtures/rust-project` into a `mktemp -d` tmpdir BEFORE invoking the upstream Git MCP binary (forbidden_paths preserves fixture pristineness even if upstream writes side-files)
* **WO-0074 §verifier** — UCIL_SKIP_EXTERNAL_PLUGIN_TESTS and UCIL_SKIP_PLATFORM_PLUGIN_E2E MUST NOT be exported when verifier runs `cargo test g6_plugin_manifests::*` and `bash scripts/verify/P3-W10-F0{5,6,7}.sh` (per WO-0075 scope_in #14)

## Anti-laziness contract — confirm

* No `todo!()` / `unimplemented!()` / `NotImplementedError` / `pass`-only bodies in shipped code
* No `#[ignore]` / `.skip()` / `xfail` / `it.skip` / commented-out assertions
* No mocking of Serena, LSP, SQLite, LanceDB, Docker — the integration test spawns real `npx`/`uvx` MCP server subprocesses
* The `//! Mocking ... is forbidden` rustdoc declaration in g6_plugin_manifests.rs is a NEGATIVE assertion forbidding mocking, NOT a mock implementation (per WO-0048 line 363 + WO-0072 §executor exemption)
* No edits to `feature-list.json`, master plan, fixtures, daemon-src, schema, ADRs, or other forbidden_paths
* All 7 commits Conventional-Commit-formatted with `Phase: 3` + correct `Feature:` + `Work-order: WO-0075` trailers
* Branch fully pushed to origin

Ready for `critic` review and `verifier` independent verification.
