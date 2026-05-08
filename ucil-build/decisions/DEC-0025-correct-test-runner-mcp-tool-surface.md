---
id: DEC-0025
status: accepted
date: 2026-05-08
authored_by: monitor session (with explicit user authorization 2026-05-08T11:30Z — "Can you fix this? And ensure that these are not deferred and properly handled end to end without slacking off." Implicitly extends to correcting source-data errors discovered during empirical validation.)
amends: DEC-0024
supersedes: none
superseded_by: none
related_to:
  - DEC-0024 (revive test-runner-mcp via npm — material errors corrected here)
  - DEC-0021 (defer test-runner-mcp — original deferral, was correct on bare-name 404)
  - WO-0081 (test-runner-mcp G8 plugin manifest — halted by executor pending this ADR)
  - escalation 20260508T1435Z-wo-0081-dec-0024-tool-surface-mismatch.md
  - proposed-DEC-0025 (executor's authored proposal — accepted into this final form)
  - master plan §4.8 (test-runner-mcp P0 — multi-language)
---

# DEC-0025: Correct DEC-0024 — `test-runner-mcp` upstream is `@iflow-mcp/mcp-test-runner` with single-tool framework-enum dispatcher

## Status
Accepted, 2026-05-08. **Amends DEC-0024.** Authored from monitor session
based on the WO-0081 executor's live-capture forensics + their proposed
DEC-0025 (accepted as-is, this file is the final ADR). User authorization
extends from the original revival authorization to cover correction of
empirically-discovered source-data errors.

## Context

DEC-0024 (2026-05-08) reversed DEC-0021's deferral of P3-W11-F07 on the
basis that `test-runner-mcp` is published to npm by privsim and exposes
a 6-tool surface. The WO-0081 executor performed the planner-prescribed
pre-flight live capture per scope_in #5 and discovered **two material
errors in DEC-0024's source data**:

### Error 1: bare `test-runner-mcp` is NOT on npm

```
$ npm view test-runner-mcp version
npm error 404 Not Found - GET https://registry.npmjs.org/test-runner-mcp
```

The only related npm publication is the third-party mirror
`@iflow-mcp/mcp-test-runner@0.2.1` (sha256
`d6ccbd99f3c9b599216e3d9f655b6cb22e33867f`, published 2026-01-08 by
`chatflowdev`). The upstream `privsim/mcp-test-runner` GitHub repo has
**never published to npm under their own name**.

DEC-0024's research-report cited "npm test-runner-mcp" but did not
verify with `npm view` — the report conflated the GitHub repo path with
an npm package name.

### Error 2: upstream advertises ONE `run_tests` tool, NOT six tools

Live `tools/list` against `npx -y @iflow-mcp/mcp-test-runner@0.2.1`:

- `tools[]`: 1 entry, name=`run_tests`
- `inputSchema.framework`: enum=`["bats", "pytest", "flutter", "jest",
  "go", "rust", "generic"]`
- `serverInfo.name = "test-runner"`, `serverInfo.version = "0.1.0"`

DEC-0024 §Decision point 3 mistook the framework enum (7 values) for a
6-tool surface. The upstream `src/index.ts` at every commit in repo
history (back to `463bd5d6` 2025-01-18 "init") confirms the
single-tool-with-framework-enum pattern has ALWAYS been the design.

## Decision

**Adopt Option A from proposed-DEC-0025** (executor's recommendation):

1. **Manifest target**: `@iflow-mcp/mcp-test-runner@0.2.1` (the only
   published surface; SHA-pinned to `d6ccbd99f3c9b599216e3d9f655b6cb22e33867f`
   for stability per master-plan §13 pinned-immutable-ref policy).
2. **Manifest fields**:
   - `[plugin] name = "test-runner"` (matches `serverInfo.name`)
   - `command = "npx"`, `args = ["-y", "@iflow-mcp/mcp-test-runner@0.2.1"]`
   - `[capabilities] provides = ["testing.run"]` (single dispatcher
     pattern; framework selection is via the `framework` input arg)
   - `[capabilities] languages = ["rust", "python", "typescript",
     "javascript", "go", "shell", "dart"]` (covers the framework enum
     bats/pytest/flutter/jest/go/rust/generic — Dart for flutter,
     shell for bats/generic)
3. **Acceptance criterion #4 amendment** — DEC-0024 §Decision point 3's
   "assert health.tools.len() >= 6" is REPLACED with:
   - assert that `run_tests` is advertised (the canonical upstream tool name)
   - AND assert that the tool's `framework` enum exposes ≥ 6 framework
     values from the canonical set {bats, pytest, flutter, jest, go,
     rust, cargo, generic, vitest}
4. **Rejecting Option B** (vendor in-house TS wrapper at
   `plugin/test-runner-mcp/`): explodes WO commit budget to 200+ LOC
   net-new TS infrastructure that no other UCIL plugin manifest carries;
   master plan §4.8 line 405 ("unified test execution") is functional
   not structural and is satisfied by the iflow-mcp dispatcher pattern.
   Option B remains as a Phase 7 hardening fallback if the iflow-mcp
   mirror rots.
5. **Rejecting Option C** (re-defer to Phase 7): the upstream surface
   exists today, just not where DEC-0024 said it did; deferring would
   regress P3 ceiling unnecessarily.

## Rationale

- **Empirical validation beat web-search assertions**. The executor's
  decision to actually run `npx` + `tools/list` and capture the live
  surface caught both errors in DEC-0024. This validates the pattern of
  having executors do hands-on probing as part of pre-flight (scope_in #5).
- **Functional spec is satisfied**. Master plan §4.8 line 405
  "unified test execution covers Rust, Python, TypeScript via MCP" is
  satisfied by the iflow-mcp `run_tests` dispatcher with framework={rust,
  pytest, jest}. The 6-tool count was a misreading, not a master-plan
  requirement.
- **SHA-pinning protects against upstream drift**. The iflow-mcp mirror
  has only 2 versions (both early 2025) and a low star count (~16
  upstream), so SHA-pinning is defensive. If `@iflow-mcp/mcp-test-runner`
  rots, supersede this ADR with Option B.
- **Anti-laziness contract holds**. The executor refused to silently
  deviate (didn't substitute the iflow-mcp name for the planner's
  bare name; didn't relax AC #4 unilaterally; halted and escalated
  per scope_in #5 + #2 boundary).

## Consequences

- **Planner re-emits WO-0081** (or WO-0081-bis) with:
  - manifest install path: `npx -y @iflow-mcp/mcp-test-runner@0.2.1`
  - `[plugin] name = "test-runner"` (NOT `test-runner-mcp`)
  - acceptance criterion #4 reads: "assert `run_tests` advertised AND
    framework enum exposes ≥ 6 of the canonical set"
- **WO-0081 attempts counter** stays at whatever the executor left it
  (likely 0 or 1 — verifier never ran). Per the harness contract,
  attempts only increments on verifier rejections; an executor halt
  with escalation does not increment attempts.
- **Lessons-learned for planner**: when authoring revival ADRs, the
  research-report claims about npm/PyPI publication MUST be verified
  with `npm view <pkg> version` / `pip download <pkg>` BEFORE the ADR
  lands. Web search alone is insufficient — search snippets can conflate
  GitHub repo paths with npm package names.
- **Lessons-learned for executor**: the live-capture-before-implement
  pattern (`npx -y ... | initialize | tools/list`) caught two material
  errors at zero cost. Apply this pattern to all manifest-revival WOs.

## Revisit trigger

If `@iflow-mcp/mcp-test-runner` is unmaintained for 12 months OR removed
from npm OR the upstream `privsim/mcp-test-runner` repo is archived,
supersede this ADR with Option B (vendor in-house TS wrapper).

If the upstream `privsim/mcp-test-runner` author publishes to npm under
their own name, supersede with a fresh ADR pointing at that publication
(more authoritative than the third-party mirror).

## References

- DEC-0024 (amended): revive-test-runner-mcp-via-npm.md
- proposed-DEC-0025 (executor proposal, accepted): proposed-DEC-0025-test-runner-mcp-tool-surface-correction.md
- WO-0081 escalation: ucil-build/escalations/20260508T1435Z-wo-0081-dec-0024-tool-surface-mismatch.md
- Live capture transcript: ucil-build/escalations/wo-0081-tools-list-capture.txt
- npm registry probes documented in proposed-DEC-0025
- GitHub upstream: https://github.com/privsim/mcp-test-runner
- npm published: https://www.npmjs.com/package/@iflow-mcp/mcp-test-runner
- master plan §4.8 line 405 (test-runner-mcp P0 — multi-language)
