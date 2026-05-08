---
id: 20260508T1435Z-wo-0081-dec-0024-tool-surface-mismatch
date: 2026-05-08T14:35Z
authored_by: WO-0081 executor
blocks_loop: true
severity: high
requires_planner_action: true
requires_user_action: true
related_to:
  - WO-0081 (test-runner-mcp G8 plugin manifest)
  - DEC-0024 (revive test-runner-mcp via npm — material errors in source data)
  - proposed-DEC-0025 (correction proposal authored alongside this escalation)
---

# WO-0081 — DEC-0024 source-data errors discovered during pre-flight live capture

## Summary

The WO-0081 executor performed the planner-prescribed Tier-1 live capture
(scope_in #5) against the upstream test-runner-mcp MCP server. The
capture revealed two material errors in DEC-0024's premises that
together make the work-order's hard ACs unsatisfiable as written:

1. **`test-runner-mcp` is NOT published to npm.** The bare name returns
   404 from the npm registry. Only the third-party scoped mirror
   `@iflow-mcp/mcp-test-runner@0.2.1` exists.
2. **The upstream advertises ONE tool, not six.** Live `tools/list`
   against `npx -y @iflow-mcp/mcp-test-runner@0.2.1` returns a single
   `run_tests` tool whose input schema carries a `framework` enum
   (bats/pytest/flutter/jest/go/rust/generic). DEC-0024 §Decision
   point 3 mistook this enum for a 6-tool surface.

scope_in #5 directs the executor to "pivot down to the latest preceding
ref exposing the canonical surface" — but the upstream's git history
shows the single-tool-with-framework-enum pattern has existed
unchanged since the repo was initialized (commit `463bd5d6`,
2025-01-18). There is no preceding ref that exposes a 6-tool surface,
because that surface never existed.

scope_in #2 grants Tier-2 fallback authority (vendor a ~200-LOC TS
wrapper at `plugin/test-runner-mcp/`) for exactly this scenario. The
executor pauses to check with the user before exercising it because:

1. Tier-2 explodes the WO's anticipated ~80-LOC commit budget.
2. Tier-2 introduces a net-new TS build pipeline (npm install, dist/
   build, dist artifact mgmt) that no other UCIL plugin manifest
   carries.
3. The 6-tool prescription itself was the planner research error — the
   master-plan §4.8 line 405 spec ("unified test execution covers Rust,
   Python, TypeScript via MCP") is functional, not structural, and is
   satisfied by either Tier-1 (iflow-mcp mirror's `run_tests` dispatcher
   with `framework=rust` / `pytest` / `jest`) OR Tier-2 (in-house 6-tool
   wrapper).

## Live capture artifacts

- `npm view test-runner-mcp` → 404
- `npm view mcp-test-runner` → 404
- `npm view @modelcontextprotocol/server-test-runner` → 404
- `npm view @iflow-mcp/mcp-test-runner version` → `0.2.1`
- `npm view @iflow-mcp/mcp-test-runner` → `bin: mcp-test-runner`,
  `dependencies: { @modelcontextprotocol/sdk: ^1.1.0 }`,
  `published 2026-01-08 by chatflowdev`,
  `shasum: d6ccbd99f3c9b599216e3d9f655b6cb22e33867f`
- `gh api repos/privsim/mcp-test-runner/commits` → latest sha
  `83c84ed053f534774f7de935aeaa7698a5e5f9dc` (2025-11-09 "Merge pull
  request #2 ... add MCP server badge")
- `gh api repos/privsim/mcp-test-runner/tags` → `[]` (no tags)
- Initialize + tools/list transcript saved to `/tmp/wo-0081-capture.out`
  - `serverInfo.name = "test-runner"`, `serverInfo.version = "0.1.0"`
  - `tools[]`: 1 entry, name=`run_tests`, framework enum has 7 values

## Available paths (per proposed-DEC-0025 §Decision options)

- **Option A (recommended)** — Land Tier-1 with iflow-mcp mirror; amend
  AC #4 to assert on framework-enum coverage instead of tool count.
  Preserves DEC-0024's spirit at minimal cost.
- **Option B** — Pivot to Tier-2 vendored wrapper. Implements DEC-0024's
  6-tool prescription verbatim at the cost of net-new build infrastructure.
- **Option C** — Re-defer F07 by superseding DEC-0024. Phase 3 reverts to
  20/45 pending a fresh F07 strategy.

The full options analysis lives in
`ucil-build/decisions/proposed-DEC-0025-test-runner-mcp-tool-surface-correction.md`.

## What I tried

1. Created worktree `feat/WO-0081-test-runner-mcp-plugin-manifest` from
   main. Worktree state: clean; ready to receive code under any of the
   three options.
2. Probed npm and GitHub upstream for the metadata DEC-0024 claimed.
3. Drove a live MCP `initialize` + `notifications/initialized` +
   `tools/list` handshake via `/tmp/wo-0081-capture.py` against
   `npx -y @iflow-mcp/mcp-test-runner@0.2.1` — full transcript at
   `/tmp/wo-0081-capture.out`.
4. Checked upstream `src/index.ts` at every commit in repo history to
   confirm the single-tool-with-framework-enum pattern has always been
   the upstream surface (it has).
5. Authored `proposed-DEC-0025-test-runner-mcp-tool-surface-correction.md`
   with Options A/B/C analysis.

## What I did NOT do

- Did NOT write `plugins/testing/test-runner/plugin.toml` (would need to
  pick a Tier and amend AC #4 first).
- Did NOT write `scripts/devtools/install-test-runner-mcp.sh` (same).
- Did NOT replace `scripts/verify/P3-W11-F07.sh` (same).
- Did NOT modify `crates/ucil-daemon/tests/g8_plugin_manifests.rs`
  (the new test's literal-tool-name assertion is path-dependent on the
  Tier choice).
- Did NOT silently substitute `@iflow-mcp/mcp-test-runner` for the
  planner's `test-runner-mcp` (silent deviation forbidden by anti-
  laziness contract).
- Did NOT relax AC #4 unilaterally (the AC is in the work-order; only
  the user/planner can amend it via ADR).

## Recommended next action

User reviews `proposed-DEC-0025` and selects Option A / B / C. Planner
then re-emits WO-0081 (or WO-0082 if F07 is re-deferred) with the AC
text adjusted to match the chosen option.

If Option A is selected, the WO can be re-emitted without much
re-work — the executor will land:
- `plugins/testing/test-runner/plugin.toml` with
  `command="npx"`, `args=["-y", "@iflow-mcp/mcp-test-runner@0.2.1"]`
- `[plugin] name = "test-runner"` (matches `serverInfo.name`)
- `[capabilities] provides = ["testing.run"]` (single dispatcher; or
  multiple `testing.<framework>.run` capabilities, planner's call)
- `[capabilities] languages = ["rust", "python", "typescript",
  "javascript", "go", "shell", "dart"]` (covers framework enum
  bats/pytest/flutter/jest/go/rust/generic)
- New test pinning on `run_tests` (the upstream literal) AND on
  framework enum coverage `>=6` instead of tool count `>=6`.

resolved: false
