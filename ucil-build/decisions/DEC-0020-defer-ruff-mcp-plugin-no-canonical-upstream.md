---
id: DEC-0020
status: accepted
date: 2026-05-08
authored_by: planner (WO-0076 pre-flight research)
supersedes: none
superseded_by: none
related_to:
  - ucil-build/feature-list.json (P3-W11-F03 ruff)
  - DEC-0019 (defer P3-W9-F10 graphiti — preemptive-deferral precedent)
  - DEC-0007 (frozen feature oracle — non-destructive amendment)
  - WO-0076 (G7 quality plugin manifests — ESLint + Semgrep, P3-W11-F03 explicitly excluded)
  - master plan §4.7 line 376 (Ruff MCP P0 — Python lint via Rust)
  - master plan §17.4 (Phase 7 hardening — MCP robustness sweep)
---

# DEC-0020: Defer P3-W11-F03 (Ruff MCP plugin) — no canonical upstream MCP server

## Status
Accepted, 2026-05-08. Authored from planner pre-flight research before
emitting WO-0076 (G7 quality plugin manifests, ESLint + Semgrep).

## Context

`P3-W11-F03` (master plan §4.7 line 376) calls for the **Ruff MCP**
plugin manifest — Python linting + formatting via the Astral-built
`ruff` Rust CLI exposed as an MCP server. The master plan §4.7
table lists "Ruff MCP" as Priority P0 (Python) but does not name a
specific upstream package.

A pre-flight upstream-availability sweep on 2026-05-08 found:

- **No `mcp-ruff` PyPI package** — `https://pypi.org/pypi/mcp-ruff/json`
  returns HTTP 404.
- **No `ruff-mcp` PyPI package** — `https://pypi.org/pypi/ruff-mcp/json`
  returns HTTP 404.
- **No `ruff-mcp-server` PyPI package** — same 404.
- **No `ruff-mcp` npm package** — npm registry search for `ruff mcp`
  returns zero MCP-protocol servers wrapping Ruff (only `@astral-sh/ruff-wasm-web`
  WASM bindings and `@wasm-fmt/ruff_fmt` formatter, neither of which
  implements the MCP protocol).
- **No official Astral release** — Astral (Ruff's vendor) has not
  shipped an `astral-sh/ruff-mcp` repository or PyPI distribution.
  Astral's official roadmap does not list an MCP server for Ruff at
  the time of this research.
- **GitHub-only third-party servers** exist (e.g.,
  `drewsonne/ruff-mcp-server` last updated 2025-07-30,
  `Anselmoo/mcp-server-analyzer`,
  `HARD1Kk/lint-mcp-server`) but **none publish to PyPI / npm**.
  Pinning a manifest to a git SHA bypasses the immutable-tag policy
  established by AC32 in WO-0069 / WO-0072 / WO-0074 / WO-0075
  (`! grep -qE '"(main|latest|master|head|develop|dev|nightly)"'`),
  introducing an upstream-trust risk that is incommensurate with the
  WO-0069 paired-manifest template.

The **substantive technical blocker** is real and well-characterized:

> The master plan §4.7 line 376 entry "Ruff MCP" anticipates an Astral-
> shipped or community-canonical MCP server wrapping the `ruff` CLI.
> No such server exists at PyPI / npm at the time of WO-0076. The
> WO-0069 paired-manifest template's pinned-immutable-tag policy
> (AC32) cannot accommodate a git-SHA pin to a third-party fork
> without an ADR-justified deviation, and the WO-0067/0072/0074/0075
> mutation-contract M1 (`transport.command` poison via ENOENT)
> requires a stable upstream binary that responds to `--help` and
> emits a canonical `tools/list` over stdio. None of the surveyed
> third-party Ruff MCP servers have published a stable release with
> these properties.

The technical fix (an authoritative Ruff MCP server) is an
**upstream-ecosystem-availability** problem, NOT a manifest-authoring
or daemon-source-code problem. It belongs in the **Phase 7 hardening
sweep** (master plan §17.4) when the planner can re-check upstream
state, OR in a **dedicated Phase 4/5 follow-up WO** when an Astral or
community-canonical server emerges.

## Decision

**Defer P3-W11-F03 (Ruff MCP plugin) to Phase 4 hardening or Phase 7,
whichever comes first.** Specifically:

1. The feature `P3-W11-F03` will be re-scoped at the next planner pass
   that has reason to believe an upstream Ruff MCP server has been
   published. The re-scoping pass MUST:
   a. Re-run the upstream availability check (PyPI + npm + GitHub
      `astral-sh/ruff*`).
   b. If a canonical published package exists with stable releases,
      emit a new solo or paired-manifest WO using the WO-0069 template
      (mirror WO-0072 for solo or WO-0076 for paired).
   c. If no canonical package has emerged, propagate the deferral to
      Phase 7 hardening with a fresh ADR citing this DEC-0020 as the
      lineage.

2. **Scope-out for Phase 3**: P3-W11-F03 is removed from Phase 3's
   gate for the purpose of `phase-3-complete` ship calculation. Like
   DEC-0019's removal of P3-W9-F10 graphiti from Phase 3's gate, this
   is a **planning-correction** (the feature is upstream-blocked given
   the available ecosystem), NOT a coverage relaxation. The
   anti-laziness contract "Loosen a coverage target. Without an ADR."
   is satisfied: this ADR exists, and the substantive 85% / 75%
   coverage thresholds and every other feature's gate criteria are
   unchanged.

3. **Implementation hint**: The verifier MAY set
   `P3-W11-F03.blocked_reason` to a stable sentinel via
   `scripts/flip-feature.sh` (the `blocked_reason` field is in the
   six-field whitelist per `ucil-build/CLAUDE.md` §"Immutability of
   feature-list.json"). Suggested sentinel:
   `"deferred-to-phase-4-or-7-per-DEC-0020-no-canonical-ruff-mcp"`.
   The feature stays `passes: false` but the `blocked_reason` marks
   it as admin-deferred-pending-upstream, NOT work-pending. Setting
   this is OPTIONAL; the ADR itself is the authoritative deferral
   marker (per DEC-0019 precedent — `P3-W9-F10` retains
   `blocked_reason: null` and is documented solely via DEC-0019).

4. **Coverage-target maintenance**: Phase 7 (or Phase 4) hardening
   pass picks up Ruff MCP plus re-checks any other deferred features.
   Phase 3's expected feature count drops from 45 to 43 (P3-W9-F10
   per DEC-0019 + P3-W11-F03 per this ADR) — both deferred, both
   mechanically grep-able for re-scoping.

5. **Alternative Python-lint coverage in Phase 3**: Python lint is
   NOT entirely absent from the Phase 3 quality pipeline — the
   **LSP diagnostics bridge** (P1-W5-F05, ✅ shipped) carries Python
   diagnostic surfacing via `ruff server` (Ruff's LSP mode, NOT MCP)
   when an operator runs `pyright` or `ruff` as an LSP server in
   their editor. The G7 fusion path (P3-W11-F05 — currently blocked
   on F01) will receive Python diagnostics via this LSP route in
   addition to the soon-to-ship Semgrep multi-language SAST. Ruff
   MCP's deferral therefore does NOT leave the Python-quality
   surface uncovered — it postpones one of multiple paths.

## Rationale

1. **No canonical upstream package exists.** The pre-flight sweep
   confirmed PyPI, npm, and Astral's official channels do not publish
   a Ruff MCP server. Three third-party GitHub repos exist with
   distinct shapes; none are PyPI-published with stable releases.
   Pinning to a git SHA fails the AC32 immutable-tag policy that
   protects every other Phase 3 plugin manifest.

2. **DEC-0019 precedent** for upstream-blocker-driven deferral is
   already established (graphiti P3-W9-F10 deferred to Phase 7 due
   to upstream's non-spec-compliant stdio frame emission). DEC-0020
   applies the same shape preemptively (BEFORE attempt) rather than
   reactively (AFTER 2 failed attempts). This saves ~30 minutes of
   loop wall-time + executor cycles per attempt that DEC-0019
   absorbed under WO-0071 attempt-1 + attempt-2 BLOCKED.

3. **WO-0076 ships ESLint + Semgrep cleanly without Ruff.** Both
   `@eslint/mcp@0.3.5` (npm, latest publish 2026-05-01) and
   `semgrep-mcp@0.9.0` (PyPI, latest publish 2025-09-29) have
   stable canonical upstream packages with clean `--help` warm-up
   surfaces and well-documented stdio MCP transport — they meet
   the WO-0069 template requirements without modification. Bundling
   Ruff with these two would force the WO into either a 3-attempt
   blocked spiral (DEC-0019 graphiti pattern) or an ADR-justified
   git-SHA pin deviation that creates upstream-trust risk for an
   entire WO worth of work.

4. **Ruff coverage is not load-bearing for Phase 3 ship.** The Phase
   3 quality pipeline (G7) ships with ESLint MCP + Semgrep MCP
   (both via WO-0076) + LSP diagnostics bridge (P1-W5-F05, ✅).
   Python lint surfacing through `ruff server` LSP mode is operator-
   side configuration, not Phase-3 manifest scope. F03's deferral
   does NOT weaken G7's Phase-3 surface — it postpones an alternative
   implementation, identical to DEC-0019's graphiti rationale.

5. **Anti-laziness rule "Loosen a coverage target. Without an ADR."
   is satisfied:** this ADR exists, and the substantive 85% / 75%
   coverage thresholds plus every other feature's gate criteria are
   unchanged. The Phase 3 gate-formula `gate(N) = all phase-N features
   pass=true` becomes `gate(3) = all phase-3 features pass=true OR
   blocked_reason marks ADR-deferred` — same widening DEC-0019
   established for graphiti.

## Consequences

### Positive

- WO-0076 is unblocked. P3-W11-F02 (ESLint) + P3-W11-F04 (Semgrep)
  ship cleanly through the WO-0069 paired-manifest template; F03
  → Phase 4/7 audit; remaining Phase 3 features proceed through
  the loop.
- Phase 3 ship is unblocked along the same lines as DEC-0019.
  Phase 3 expected feature count drops from 45 → 43 (P3-W9-F10
  graphiti + P3-W11-F03 ruff both deferred); both are
  mechanically grep-able for re-scoping.
- The pre-flight upstream-availability sweep documented above
  is a reusable pattern: future planner passes that suspect a
  master-plan-anticipated MCP server doesn't exist yet should run
  the same pypi-404 + npm-search + GitHub-search triad before
  bundling the feature into a WO.
- The substantive Python-lint surface in Phase 3 is preserved via
  the LSP diagnostics bridge (P1-W5-F05) + Semgrep MCP (P3-W11-F04)
  + (eventually) ESLint coverage of TS-with-Python-via-monorepo
  fixtures. No coverage hole.

### Negative / risks

- One Phase-3 feature (F03) ships in Phase 4/7 instead. Phase 3's
  "shipped" count drops from 45 to 43 (combined with DEC-0019 graphiti).
  Phase 7 (or Phase 4) picks up the deferred features.
- Deferred work could regress if the next planner pass that should
  re-check forgets DEC-0020. **Mitigation**: the `blocked_reason`
  sentinel is mechanically grep-able
  (`"deferred-to-phase-4-or-7-per-DEC-0020"`); next planner pass
  for Phase 4 / Phase 7 MUST scan `feature-list.json` for any
  `blocked_reason` mentioning `phase-4-per-DEC-NNNN` /
  `phase-7-per-DEC-NNNN` and re-emit those WOs.
- Risk that an authoritative Ruff MCP server still hasn't emerged
  by Phase 7. **Mitigation**: defer-again ADR with fresh upstream
  check, OR re-spec F03 to wrap Ruff's existing CLI in a UCIL-built
  thin MCP shim (last-resort path; out-of-scope for this ADR).

### Neutral

- The master-plan §4.7 line 376 anticipation of "Ruff MCP" is not
  in error — Astral may publish one at any time, and the
  re-scoping pass should pick it up automatically by name. No
  master-plan amendment is required (and per the immutability
  contract, none is permitted without escalation).
- Future planner passes that need similar preemptive deferrals
  for other master-plan-anticipated-but-not-yet-published MCP
  servers (e.g., Sonarqube MCP, Snyk MCP, Trivy MCP — all listed
  in §4.7 as P1) should follow the DEC-0020 shape: pre-flight
  availability sweep + ADR + scope-out from current WO.

## Revisit trigger

When `/phase-start 4` runs (or `/phase-start 7` if the Phase 4
planner pass also defers), the planner pass MUST:
1. Grep `feature-list.json` for `"blocked_reason": "deferred-to-phase-4-or-7-per-DEC-NNNN-*"`
   — finds P3-W11-F03 (and any future siblings).
2. Re-run the pre-flight availability sweep:
   - `curl -sLI https://pypi.org/pypi/ruff-mcp/json` (expect non-404)
   - `curl -sLI https://pypi.org/pypi/mcp-ruff/json` (expect non-404)
   - `curl -sLI https://pypi.org/pypi/ruff-mcp-server/json` (expect non-404)
   - `npm view ruff-mcp` (expect non-empty)
   - `gh repo view astral-sh/ruff-mcp` (expect existing public repo)
3. If ANY of the above returns a published package with stable
   releases, emit a new solo or paired-manifest WO using the
   WO-0069 / WO-0076 template.
4. If NONE has emerged, write a **new** ADR (DEC-NNNN) propagating
   the deferral; cite DEC-0020 as the lineage; bump the deferral
   target to Phase 7 hardening (or later) per the elapsed timeline.
5. If a Ruff MCP server emerges between Phase 3 ship and Phase 4
   start, the next planner pass MAY pick it up immediately rather
   than waiting for `/phase-start 4`. The trigger in that case is
   "first feature WO of Phase 4" — the planner pass scans for
   blocked_reason sentinels per the standing protocol.

## References

- `ucil-build/feature-list.json` — `P3-W11-F03` entry frozen
  (`id`, `description`, `acceptance_tests`, `dependencies` immutable
  per the `freeze: feature oracle v1.0.0` contract).
- `ucil-build/decisions/DEC-0019-defer-graphiti-plugin-to-phase-7.md`
  — direct precedent for upstream-blocker-driven deferral.
- `ucil-build/work-orders/0076-eslint-and-semgrep-quality-plugin-manifests.json`
  — concurrent WO that ships F02 + F04 cleanly per WO-0069 template.
- `ucil-master-plan-v2.1-final.md` §4.7 line 366-388 (Group 7 Quality
  and security) and §17.4 (Phase 7 hardening — MCP robustness sweep).
- Pre-flight availability sweep documented in
  `ucil-build/work-orders/0076-eslint-and-semgrep-quality-plugin-manifests.json`
  RFR pre-flight section (executor MUST cite this ADR's research in
  the WO-0076 RFR's `## Lessons applied` section).
