---
id: proposed-DEC-0025
status: proposed
date: 2026-05-08
authored_by: WO-0081 executor (live-capture forensics)
supersedes: none
amends: DEC-0024
related_to:
  - WO-0081 (test-runner-mcp G8 plugin manifest)
  - DEC-0024 (revive test-runner-mcp via npm — material errors discovered)
  - master plan §4.8 (test-runner-mcp P0 — multi-language unified test execution)
---

# proposed-DEC-0025: Correct DEC-0024 — `test-runner-mcp` is NOT on npm; upstream advertises ONE `run_tests` tool, not six

## Status

Proposed, 2026-05-08. Authored by the WO-0081 executor after the live `npx`
+ `tools/list` capture against `privsim/mcp-test-runner` revealed two
material errors in DEC-0024 that block WO-0081's hard acceptance criteria.

**Author has HALTED the executor loop and emitted an escalation
(ucil-build/escalations/20260508T1435Z-wo-0081-dec-0024-tool-surface-mismatch.md)
pending user review.**

## Context

DEC-0024 (2026-05-08) reversed DEC-0021's deferral of P3-W11-F07 on the
basis that `test-runner-mcp` is allegedly published to npm by privsim and
exposes a 6-tool surface (bats/pytest/flutter/jest/go/cargo). The
WO-0081 executor performed the planner-prescribed pre-flight live capture
per scope_in #5 and discovered:

### Error 1: bare `test-runner-mcp` is NOT on npm

```
$ npm view test-runner-mcp version
npm error code E404
npm error 404 Not Found - GET https://registry.npmjs.org/test-runner-mcp - Not found
npm error 404  'test-runner-mcp@*' is not in this registry.
```

The only related npm publication is the third-party mirror
`@iflow-mcp/mcp-test-runner@0.2.1` (sha256
`d6ccbd99f3c9b599216e3d9f655b6cb22e33867f`, published 2026-01-08 by
`chatflowdev <chatflowdev@gmail.com>` / `qystart <987472953@qq.com>`),
which carries the upstream privsim/mcp-test-runner code verbatim under a
**different scoped name**. The upstream `privsim/mcp-test-runner`
GitHub repo (16 stars, 13 commits, latest commit `83c84ed0`
2025-11-09) has **NEVER been published to npm by the upstream author
under any name** — only the iflow-mcp mirror exists.

The `@iflow-mcp/mcp-test-runner` package itself instructs in its README
to `npm install test-runner-mcp` — but that bare name resolves to 404.
The iflow-mcp mirror's `bin` field exposes the runtime as
`mcp-test-runner` (kebab-case, NOT `test-runner-mcp`).

### Error 2: upstream advertises ONE tool with a framework enum, NOT six tools

Live `tools/list` reply against `npx -y @iflow-mcp/mcp-test-runner@0.2.1`:

```json
{
  "result": {
    "tools": [
      {
        "name": "run_tests",
        "description": "Run tests and capture output",
        "inputSchema": {
          "properties": {
            "framework": {
              "enum": ["bats", "pytest", "flutter", "jest", "go", "rust", "generic"]
            },
            "command": { "type": "string" },
            "workingDir": { "type": "string" },
            ...
          },
          "required": ["command", "workingDir", "framework"]
        }
      }
    ]
  }
}
```

`serverInfo.name = "test-runner"`, `serverInfo.version = "0.1.0"` (NOT
the npm package version 0.2.1).

**Total advertised tools: 1.** **NOT 6 as DEC-0024 prescribed.**

The framework enum (bats/pytest/flutter/jest/go/rust/generic — 7 values,
one of which is `rust` not `cargo`) is the discriminator that DEC-0024
mistook for a 6-tool surface. Inspection of the upstream `src/index.ts`
at every commit in repo history (back to commit `463bd5d6` 2025-01-18
"init") confirms the server has ALWAYS exposed exactly one `run_tests`
tool with this framework-enum dispatcher pattern — there is no
historical commit/tag where the 6-tool surface ever existed.

## Impact on WO-0081

The work-order's hard acceptance criteria #4 explicitly require:

> assert on `health.tools.len() >= 6` (per DEC-0024 §Decision point 3)

Tier-1 (`npx -y @iflow-mcp/mcp-test-runner@0.2.1`) cannot satisfy this:
the live tool count is 1, not ≥ 6.

scope_in #5 directs the executor to "pivot down to the latest preceding
ref exposing the canonical surface" if live capture returns < 6 tools —
but **no such preceding ref exists**. The 6-tool surface was a
planner research error, not a regression in a recent upstream release.

scope_in #2 grants Tier-2 fallback authority (vendor a ~200-LOC TS
wrapper at `plugin/test-runner-mcp/` that re-exposes 6 tools), but this
path:

1. Adds net-new build infrastructure (TypeScript build pipeline, npm
   install for the integration test, dist/ artifact management) that no
   prior plugin manifest WO carries.
2. Explodes the WO's stated ~80-LOC commit budget to ~200+ LOC of
   wrapper TS plus test-time `npm install && npm run build` orchestration.
3. Re-implements multi-language test-running CLI dispatch in-house — a
   significant scope addition that the master plan §4.8 line 405 spec
   describes as "expose unified test execution via MCP", which the
   `@iflow-mcp/mcp-test-runner` mirror (or an in-house reimpl)
   functionally satisfies via a single `run_tests` dispatcher.
4. The 6-tool prescription itself was a misreading — the master plan
   does not require any specific tool count; "unified test execution"
   is functional, not structural.

## Decision options

### Option A — Land Tier-1 with iflow-mcp mirror, amend ACs to match reality

Land `plugins/testing/test-runner/plugin.toml` pointing at
`@iflow-mcp/mcp-test-runner@0.2.1` (the only published surface), pin the
test on `run_tests` (the canonical advertised tool), and **amend the
WO-0081 acceptance criterion #4 to assert on framework-enum coverage
rather than tool count**:

- Drop `assert health.tools.len() >= 6`.
- Replace with: assert `run_tests` is advertised AND the tool's
  `framework` enum exposes ≥ 6 framework values from the canonical set
  {bats, pytest, flutter, jest, go, rust, cargo, generic, vitest, ...}.

**Cost**: low (continues the WO's anticipated ~80-LOC commit shape).
**Functional coverage**: equivalent to DEC-0024's intent (multi-language
unified test execution per F07 master-plan spec).
**Honest about the upstream surface**: yes (documents the iflow-mcp
mirror, the dispatcher pattern, and the framework enum verbatim).

### Option B — Pivot to Tier-2 (vendored TS wrapper)

Implement the ~200-LOC TS wrapper at `plugin/test-runner-mcp/` per
WO-0081 scope_in #2 verbatim, exposing 6 distinct tools
(run_cargo_tests / run_pytest / run_vitest / run_go_tests / run_bats /
run_jest). The wrapper shells out to the respective test-runner CLIs.

**Cost**: high (net-new TS package, build pipeline, dist artifact mgmt,
test-time npm install orchestration). Adds ~400-600 LOC across
package.json + tsconfig.json + src/index.ts + tests + RFR.
**Functional coverage**: equivalent to Option A but routes through
in-house code instead of the iflow-mcp mirror.
**Risk**: introduces a new failure surface (build break, npm install
flakes, dist drift) that no other UCIL plugin manifest carries.

### Option C — Re-defer F07 (supersede DEC-0024)

Acknowledge that the upstream-availability premise of DEC-0024 was
materially wrong (no canonical `test-runner-mcp` on npm; only a
third-party mirror with a 1-tool dispatcher). Restore the DEC-0021
deferral posture, with revisit-trigger updated to require either:
1. The upstream privsim/mcp-test-runner author publishes to npm under
   their own name, OR
2. UCIL chooses to invest in Tier-2 vendoring as a scoped deliverable.

**Cost**: low for this WO (no new code), but Phase 3 reverts to 20/45
pending a fresh F07 strategy.

## Recommendation

**Option A** preserves DEC-0024's spirit (revive F07, ship a working
multi-language test-runner manifest) at the cost only of correcting
the 6-tool ACs to match the real upstream surface. The functional spec
(F07 master-plan §4.8 line 405: "unified test execution covers Rust,
Python, TypeScript via MCP") is satisfied by the iflow-mcp mirror's
`run_tests` dispatcher tool with framework={rust, pytest, jest}.

Option B preserves the structural prescription of DEC-0024 (6 tools)
at meaningful infrastructure cost — better deferred to a separate WO
that scopes Tier-2 vendoring as a first-class deliverable rather than a
fallback.

Option C is the safest if there's any doubt about the iflow-mcp mirror's
maintenance posture (it has only 2 versions, both released in early
2025; the upstream privsim repo's last meaningful commit is from
2025-03-30, the merge in 2025-11-09 is just a README badge).

## References

- DEC-0024-revive-test-runner-mcp-via-npm.md (the decision being amended)
- DEC-0021-defer-test-runner-mcp-plugin-no-canonical-upstream.md (the
  decision DEC-0024 superseded; was correct on the bare-name 404 but
  wrong on Phase 7 deferral)
- WO-0081 work-order ucil-build/work-orders/0081-test-runner-mcp-plugin-manifest.json
- Live capture transcript: /tmp/wo-0081-capture.out (initialize +
  tools/list reply against `npx -y @iflow-mcp/mcp-test-runner@0.2.1`)
- npm registry probes:
  - `npm view test-runner-mcp` → 404
  - `npm view @iflow-mcp/mcp-test-runner version` → 0.2.1
  - `npm view mcp-test-runner` → 404
  - `npm view @modelcontextprotocol/server-test-runner` → 404
- GitHub upstream: https://github.com/privsim/mcp-test-runner
  - latest commit: 83c84ed053f534774f7de935aeaa7698a5e5f9dc (2025-11-09)
  - latest substantive commit: 1240ea24 (2025-03-30 "after_next")
  - tags: empty
- master plan §4.8 line 405 (test-runner-mcp P0 — multi-language)
- master plan §13 (pinned-immutable-ref policy — frozen semver tag
  `0.2.1` and SHA `d6ccbd99...` are both immutable; either works as a
  pin)
