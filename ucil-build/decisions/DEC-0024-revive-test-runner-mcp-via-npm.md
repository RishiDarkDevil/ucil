---
id: DEC-0024
status: accepted
date: 2026-05-08
authored_by: monitor session (with explicit user authorization 2026-05-08T11:30Z — "Can you fix this? And ensure that these are not deferred and properly handled end to end without slacking off.")
supersedes: DEC-0021
superseded_by: none
related_to:
  - ucil-build/feature-list.json (P3-W11-F07 test-runner-mcp)
  - DEC-0021 (defer test-runner-mcp — original deferral, now superseded)
  - WO-0077 (G8 mcp-pytest-runner manifest — adjacent template)
  - master plan §4.8 (test-runner-mcp P0 — multi-language)
---

# DEC-0024: Revive P3-W11-F07 (test-runner-mcp plugin manifest) — exists on npm, not PyPI

## Status
Accepted, 2026-05-08. Authored from monitor session with explicit user
authorization. **Supersedes DEC-0021.**

## Context

DEC-0021 deferred P3-W11-F07 (test-runner-mcp) to Phase 7 hardening
on the basis that no upstream existed. **That conclusion was a
registry-mismatch error** — the planner only checked PyPI and GitHub
for `test-runner-mcp` and found 404, but **did NOT check npm**, where
the package is published.

A 2026-05-08T11:30Z research sweep found:

- **npm package EXISTS**: `test-runner-mcp` (privsim/mcp-test-runner),
  released 2025-01-18, MIT license, ~16 GitHub stars, 13 commits.
- **Mirror published**: `@iflow-mcp/mcp-test-runner` on npm (re-publication).
- **Coverage**: Bats, Pytest, Flutter, Jest, Go, Cargo, plus a generic
  command shim — exactly the multi-language unified runner the master
  plan calls for.
- **Source repo**: https://github.com/privsim/mcp-test-runner

The deferral was a **registry-channel error** (planner searched PyPI
+ GitHub-repo-name but skipped npm registry).

## Decision

1. **Reverse DEC-0021.** P3-W11-F07 is no longer deferred to Phase 7.
2. **Re-emit a fresh WO** for P3-W11-F07 (test-runner-mcp) targeting:
   - Install via npm: `npx -y test-runner-mcp@<sha-pinned>` — pin to
     a specific commit SHA from `privsim/mcp-test-runner` for
     reproducibility (low star count means lighter maintenance, so
     pin defensively).
   - Manifest pattern matches WO-0077 (mcp-pytest-runner) but
     covers all languages instead of just pytest.
3. **Acceptance criteria** mirror WO-0077:
   - `scripts/verify/P3-W11-F07.sh` exits 0 (smoke: spawn server,
     `tools/list` MCP request returns ≥6 tools — bats/pytest/flutter/
     jest/go/cargo)
   - `cargo test -p ucil-daemon g8_plugin_manifests::test_test_runner` green
   - Stub-scan + coverage gate per verifier rules
4. **Pin SHA** rather than tag for stability (16 stars + 13 commits
   means light maintenance — protect against silent upstream changes).

## Rationale

- **Package exists today**: npm has it; planner just searched the
  wrong registry.
- **Master-plan §4.8 intent is preserved**: unified multi-language
  test execution exposed as MCP, exactly as specified.
- **Risk is bounded**: low maintenance score is mitigated by SHA-pinning
  + Tier-2 fallback (vendor 200-line TS wrapper using
  `@modelcontextprotocol/sdk` if upstream rots).
- **Phase 3 ship is improved**: P3=20/45 (assuming F10+F03+F07 all
  revived via DEC-0022/0023/0024) vs 17/45 with all three deferrals.

## Consequences

- Planner emits next WO for P3-W11-F07 with SHA-pinned npx install.
- If upstream silently breaks, fallback path is documented (Tier 2:
  in-house TS wrapper, ~200 LOC).

## Revisit trigger

If `privsim/mcp-test-runner` is archived or has zero commits for
12 months, switch to Tier 2 (vendored TS wrapper) and supersede
this ADR.

## References

- DEC-0021 (superseded): defer-test-runner-mcp-plugin-no-canonical-upstream.md
- Research report: monitor session 2026-05-08T11:30Z
- npm: https://www.npmjs.com/package/test-runner-mcp
- Source: https://github.com/privsim/mcp-test-runner
- Master plan §4.8 (test-runner-mcp P0)
