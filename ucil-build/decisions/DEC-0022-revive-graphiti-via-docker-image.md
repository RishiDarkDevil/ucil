---
id: DEC-0022
status: accepted
date: 2026-05-08
authored_by: monitor session (with explicit user authorization 2026-05-08T11:30Z — "Can you fix this? And ensure that these are not deferred and properly handled end to end without slacking off.")
supersedes: DEC-0019
superseded_by: none
related_to:
  - ucil-build/feature-list.json (P3-W9-F10 graphiti)
  - DEC-0019 (defer graphiti — original deferral, now superseded)
  - WO-0069 (codebase-memory + mem0 plugin manifests — template)
  - master plan §17.2 (graphiti P0 plugin)
  - research-report 2026-05-08T11:30Z monitor-session
---

# DEC-0022: Revive P3-W9-F10 (graphiti plugin manifest) — canonical source is non-PyPI

## Status
Accepted, 2026-05-08. Authored from monitor session with explicit user
authorization. **Supersedes DEC-0019.**

## Context

DEC-0019 deferred P3-W9-F10 (graphiti) to Phase 7 hardening on the basis
that no canonical upstream MCP server existed. **That conclusion was
incorrect** — the planner's pre-flight search only checked PyPI/npm,
not GitHub subdirectories or Docker Hub.

A 2026-05-08T11:30Z research sweep found:

- **Official Zep MCP server EXISTS**: `getzep/graphiti/mcp_server/`
  subdirectory of the main `getzep/graphiti` repo. Not packaged as
  standalone PyPI distribution because it requires a long-running
  graph DB (FalkorDB or Neo4j 5.26+).
- **Prebuilt Docker image**: `zepai/knowledge-graph-mcp` on Docker Hub.
- **Community fork**: `klaviyo/graphiti_mcp` (tracks upstream).
- **Official docs**: https://www.getzep.com/product/knowledge-graph-mcp/

The deferral was a **distribution-channel mismatch**, not a capability gap.

## Decision

1. **Reverse DEC-0019.** P3-W9-F10 is no longer deferred to Phase 7.
2. **Re-emit a fresh WO** for P3-W9-F10 (graphiti) targeting:
   - Install via prebuilt Docker image: `zepai/knowledge-graph-mcp`
   - Compose-style integration (FalkorDB or Neo4j sidecar)
   - Manifest fields:
     - `transport: stdio` (via `docker run -i ...`) OR `transport: docker`
     - `install_command: docker pull zepai/knowledge-graph-mcp`
     - `start_command: docker run -i --rm --network ucil-graphiti zepai/knowledge-graph-mcp`
     - `health_check: tools/list MCP request returns ≥1 tool`
3. **Acceptance criteria** mirror WO-0069's mem0 manifest pattern:
   - `scripts/verify/P3-W9-F10.sh` exits 0 (smoke check via `docker run`)
   - `cargo test -p ucil-daemon g6_plugin_manifests::test_graphiti` green
   - Stub-scan + coverage gate pass per standard verifier rules
4. **NO source code is required for the daemon side** — graphiti is
   orchestrated as an external MCP server, same pattern as Serena
   (P0) or codegraphcontext (WO-0072).
5. **Forbidden_paths** must include the Phase-7 sentinel from DEC-0019
   that already lives in F10's `blocked_reason` — verifier should
   clear it on flip.

## Rationale

- **Capability is real**: Zep ships the official MCP server, just not
  on PyPI. Their preferred install path is docker-compose because
  graphiti requires a graph DB sidecar.
- **Risk is bounded**: F10 has `attempts=2` from the WO-0071 BLOCKED
  retries. The next attempt with this clear install path should
  succeed cleanly. If it doesn't, we hit the 3-strikes escalation
  trigger (NOT a hard halt — escalation lets RCA inspect).
- **Phase 3 ship is improved**: P3=18/45 with this back in scope vs
  17/45 with the deferral.
- **Pattern matches existing P0 docker-image plugins**: Serena
  (master plan §0.1) is also a docker-pulled image, not a PyPI install.

## Consequences

- Planner emits next WO for P3-W9-F10 with the docker-image template
  above.
- The WO-0071 cancelled-and-archived state stays archived; this is
  a fresh WO (e.g., WO-0079 or later), not a WO-0071-bis.
- DEC-0019's "carry-forward sentinel" in F10's `blocked_reason` is
  obsolete; verifier clears on PASS.
- No carry-forward to Phase 7 anymore.

## Revisit trigger

If 3 consecutive WO attempts with the docker-image install path
fail (e.g., docker daemon flakiness, image registry outage, FalkorDB
sidecar issues), revisit and consider:
- Substituting Neo4j sidecar (also documented upstream)
- Vendoring a thin in-process wrapper around `graphiti-core` PyPI lib
  (~150 lines, exposes 5–7 tools — avoids docker dependency)

## References

- DEC-0019 (superseded): defer-graphiti-plugin-to-phase-7.md
- Research report: monitor session 2026-05-08T11:30Z, general-purpose
  agent run aa7da1993afa43fb1
- Upstream: https://github.com/getzep/graphiti/tree/main/mcp_server
- Docker image: https://hub.docker.com/r/zepai/knowledge-graph-mcp
- Community fork: https://github.com/klaviyo/graphiti_mcp
- Master plan §17.2 (graphiti P0)
