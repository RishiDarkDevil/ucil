---
id: DEC-0023
status: accepted
date: 2026-05-08
authored_by: monitor session (with explicit user authorization 2026-05-08T11:30Z — "Can you fix this? And ensure that these are not deferred and properly handled end to end without slacking off.")
supersedes: DEC-0020
superseded_by: none
related_to:
  - ucil-build/feature-list.json (P3-W11-F03 ruff)
  - DEC-0020 (defer ruff — original deferral, now superseded)
  - WO-0076 (G7 quality plugin manifests — ESLint + Semgrep template)
  - master plan §4.7 (Ruff MCP P0 — Python lint via Rust)
---

# DEC-0023: Revive P3-W11-F03 (Ruff plugin manifest) — community wrapper exists on PyPI

## Status
Accepted, 2026-05-08. Authored from monitor session with explicit user
authorization. **Supersedes DEC-0020.**

## Context

DEC-0020 deferred P3-W11-F03 (Ruff) to Phase 7 hardening on the basis
that no Ruff MCP server existed on PyPI/npm/GitHub. **That conclusion
was a search-narrowness error** — the planner queried `pypi/mcp-ruff`,
`pypi/ruff-mcp`, `pypi/ruff-mcp-server` (404 on all) but did NOT
search PyPI broadly for any Ruff-wrapping MCP server.

A 2026-05-08T11:30Z research sweep found:

- **Community wrapper on PyPI**: `mcp-server-analyzer` (Anselmoo).
  Combines Ruff + Vulture (dead-code) into one MCP server.
- **Source community wrapper**: `drewsonne/ruff-mcp-server` (3 commits,
  not on PyPI — install via `pip install -e .` from source). Three tools:
  `ruff_check`, `ruff_format`, `ruff_fix`.
- **Astral discussion**: `astral-sh/ruff#19639` — Anselmoo posted for
  feedback; Astral did not endorse-or-reject (they prefer LSP layer).

The deferral was an **incomplete-search error**.

## Decision

1. **Reverse DEC-0020.** P3-W11-F03 is no longer deferred to Phase 7.
2. **Re-emit a fresh WO** for P3-W11-F03 (Ruff) with two-tier path:
   - **Tier 1 (default)**: install `mcp-server-analyzer` from PyPI via
     `uvx mcp-server-analyzer` per WO-0076 ESLint pattern.
   - **Tier 2 (if Tier 1 fails)**: vendor a ~50-line FastMCP wrapper
     in `plugin/ruff-mcp/` that shells out to `ruff check
     --output-format=json` and `ruff format --diff`. Smallest MCP
     server template using `mcp` Python SDK is ~30–50 lines.
3. **Acceptance criteria** mirror WO-0076's ESLint pattern:
   - `scripts/verify/P3-W11-F03.sh` exits 0 (smoke check)
   - `cargo test -p ucil-daemon g7_plugin_manifests::test_ruff` green
   - Stub-scan + coverage gate per verifier rules
4. **Tier 2 fallback authority**: executor may proceed to Tier 2 in
   the same WO if Tier 1 smoke fails. This avoids an extra WO cycle.

## Rationale

- **Capability exists**: PyPI has a working wrapper today.
- **Astral's non-shipment is intentional, not blocking**: they own
  the LSP layer and view MCP as duplicative for diagnostics. We don't
  need them to ship — third-party wrapper suffices.
- **Tier 2 is trivial**: 50-line wrapper around `ruff check
  --output-format=json` is well-precedented (drewsonne's source repo
  is the proof-of-existence).
- **Phase 3 ship is improved**: P3=19/45 (assuming F10 also revived
  via DEC-0022) vs 17/45 with both deferrals.

## Consequences

- Planner emits next WO for P3-W11-F03 with two-tier install path.
- The Tier-2 fallback may add `plugin/ruff-mcp/` directory under
  forbidden_paths exemptions.

## Revisit trigger

If `mcp-server-analyzer` is unmaintained for 6 months (no commits)
or removed from PyPI, switch to Tier 2 (vendored wrapper) as default.

## References

- DEC-0020 (superseded): defer-ruff-mcp-plugin-no-canonical-upstream.md
- Research report: monitor session 2026-05-08T11:30Z
- PyPI: https://pypi.org/project/mcp-server-analyzer/
- Source wrapper: https://github.com/drewsonne/ruff-mcp-server
- Astral discussion: https://github.com/astral-sh/ruff/discussions/19639
- Master plan §4.7 (Ruff MCP P0)
