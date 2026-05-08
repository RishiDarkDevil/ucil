---
id: DEC-0021
status: accepted
date: 2026-05-08
authored_by: planner (WO-0077 pre-flight research)
supersedes: none
superseded_by: none
related_to:
  - ucil-build/feature-list.json (P3-W11-F07 test-runner-mcp)
  - DEC-0020 (defer P3-W11-F03 ruff — direct preemptive-deferral precedent)
  - DEC-0019 (defer P3-W9-F10 graphiti — original upstream-blocker pattern)
  - DEC-0007 (frozen feature oracle — non-destructive amendment)
  - WO-0077 (G8 testing solo-manifest WO — mcp-pytest-runner only; F07 explicitly excluded)
  - master plan §4.8 line 401 (test-runner-mcp P0 — unified test execution)
  - master plan §17.4 (Phase 7 hardening — MCP robustness sweep)
---

# DEC-0021: Defer P3-W11-F07 (test-runner-mcp plugin) — no canonical upstream MCP server

## Status
Accepted, 2026-05-08. Authored from planner pre-flight research before
emitting WO-0077 (G8 testing solo-manifest, mcp-pytest-runner only).

## Context

`P3-W11-F07` (master plan §4.8 line 401) calls for the **test-runner-mcp**
plugin manifest — unified test execution covering Pytest, Jest, Go, Rust,
Bats, Flutter via the Model Context Protocol. The master plan §4.8 table
lists "test-runner-mcp" as Priority P0 (multi-language) but does not name
a specific upstream package or vendor.

A pre-flight upstream-availability sweep on 2026-05-08 found:

- **No `test-runner-mcp` PyPI package** —
  `https://pypi.org/pypi/test-runner-mcp/json` returns HTTP 404.
- **No `test-runner-mcp` npm package** —
  `https://registry.npmjs.org/test-runner-mcp` returns
  `{"error":"Not found"}`.
- **No GitHub canonical repo** — `gh search repos test-runner-mcp` /
  `https://api.github.com/search/repositories?q=test-runner-mcp` returns
  one unrelated repo (`alkoleft/mcp-onec-test-runner` — a 1С/Russian
  language YaXUnit test runner for the 1С:Predprijatie platform, NOT a
  multi-language polyglot test runner). No vendor (Anthropic, official
  MCP servers, or community-canonical) has shipped a `test-runner-mcp`
  matching the master-plan's intended scope.
- **No matching official `@modelcontextprotocol/servers` entry** — the
  official `@modelcontextprotocol` GitHub org publishes Git, GitHub,
  Filesystem, Slack, etc. MCP servers but does NOT ship a
  multi-language test-runner.

Pinning a manifest to a git SHA bypasses the immutable-tag policy
established by AC32 in WO-0069 / WO-0072 / WO-0074 / WO-0075 / WO-0076
(`! grep -qE '"(main|latest|master|head|develop|dev|nightly)"'`),
introducing an upstream-trust risk that is incommensurate with the
WO-0069 paired-manifest template — same blocker DEC-0020 documented for
Ruff MCP.

The **substantive technical blocker** is real and well-characterized:

> The master plan §4.8 line 401 entry "test-runner-mcp" anticipates a
> community-canonical or vendor-shipped MCP server wrapping multiple
> language test runners (pytest, jest, cargo nextest, vitest, go test,
> bats, flutter test) under one stdio MCP surface. No such server exists
> at PyPI / npm / GitHub at the time of WO-0077. The WO-0069 paired-
> manifest template's pinned-immutable-tag policy (AC32) cannot
> accommodate a git-SHA pin to a hypothetical third-party fork without
> an ADR-justified deviation, and the WO-0067/0072/0074/0075/0076
> mutation-contract M1 (`transport.command` poison via ENOENT) requires
> a stable upstream binary that responds to `--help` and emits a
> canonical `tools/list` over stdio. No surveyed candidate has published
> a stable release with these properties.

The technical fix (an authoritative test-runner-mcp server) is an
**upstream-ecosystem-availability** problem, NOT a manifest-authoring
or daemon-source-code problem. It belongs in the **Phase 7 hardening
sweep** (master plan §17.4) when the planner can re-check upstream
state, OR in a **dedicated Phase 4/5 follow-up WO** when an Anthropic
or community-canonical server emerges.

**Note on Phase 3 G8 scope coverage**: The Phase 3 G8 (Testing) group
is NOT entirely without canonical upstream coverage even with F07
deferred. P3-W11-F08 (mcp-pytest-runner) ships in WO-0077 against
the published `mcp-pytest-runner@0.2.1` PyPI package (latest publish
2025-12+, summary "MCP server providing opinionated pytest execution
interface for AI agents") which provides Python test discovery +
execution + selective re-run by node ID. The deferred F07
(test-runner-mcp) is the **multi-language unifier** — its absence
means JS/TS test execution via Vitest, Rust via cargo nextest, Go via
`go test`, etc., are NOT covered by an MCP plugin manifest in Phase 3.
**Mitigation** for the multi-language coverage gap: Phase 4/5/7
re-emission picks it up when upstream lands; alternatively, a future
planner pass MAY emit individual-language test-runner manifests
(e.g., `mcp-vitest-runner` if such PyPI/npm packages exist) under the
G8 group's `testing.*` namespace per the WO-0077 G8 template.

## Decision

**Defer P3-W11-F07 (test-runner-mcp plugin) to Phase 4 hardening or
Phase 7, whichever comes first.** Specifically:

1. The feature `P3-W11-F07` will be re-scoped at the next planner pass
   that has reason to believe an upstream test-runner-mcp server has
   been published. The re-scoping pass MUST:
   a. Re-run the upstream availability check (PyPI + npm + GitHub
      `modelcontextprotocol/test-runner-mcp` /
      `anthropic/test-runner-mcp` / community-canonical).
   b. If a canonical published package exists with stable releases,
      emit a new solo or paired-manifest WO using the WO-0069 template
      (mirror WO-0077 for solo or WO-0076 for paired).
   c. If no canonical package has emerged, propagate the deferral to
      Phase 7 hardening with a fresh ADR citing this DEC-0021 (and
      DEC-0020 / DEC-0019) as the lineage chain.

2. **Scope-out for Phase 3**: P3-W11-F07 is removed from Phase 3's
   gate for the purpose of `phase-3-complete` ship calculation. Like
   DEC-0019's removal of P3-W9-F10 graphiti and DEC-0020's removal of
   P3-W11-F03 ruff from Phase 3's gate, this is a **planning-correction**
   (the feature is upstream-blocked given the available ecosystem),
   NOT a coverage relaxation. The anti-laziness contract "Loosen a
   coverage target. Without an ADR." is satisfied: this ADR exists,
   and the substantive 85% / 75% coverage thresholds and every other
   feature's gate criteria are unchanged.

3. **Implementation hint**: The verifier MAY set
   `P3-W11-F07.blocked_reason` to a stable sentinel via
   `scripts/flip-feature.sh` (the `blocked_reason` field is in the
   six-field whitelist per `ucil-build/CLAUDE.md` §"Immutability of
   feature-list.json"). Suggested sentinel:
   `"deferred-to-phase-4-or-7-per-DEC-0021-no-canonical-test-runner-mcp"`.
   The feature stays `passes: false` but the `blocked_reason` marks
   it as admin-deferred-pending-upstream, NOT work-pending. Setting
   this is OPTIONAL; the ADR itself is the authoritative deferral
   marker (per DEC-0019/0020 precedent — `P3-W9-F10` and
   `P3-W11-F03` retain `blocked_reason: null` and are documented
   solely via their respective ADRs).

4. **Coverage-target maintenance**: Phase 7 (or Phase 4) hardening
   pass picks up test-runner-mcp plus re-checks any other deferred
   features. Phase 3's expected feature count drops from 45 to 42
   (P3-W9-F10 graphiti per DEC-0019 + P3-W11-F03 ruff per DEC-0020 +
   P3-W11-F07 test-runner-mcp per this ADR) — three deferred, all
   mechanically grep-able for re-scoping.

5. **Alternative test-execution coverage in Phase 3**: Test execution
   is NOT entirely absent from the Phase 3 testing pipeline — the
   **mcp-pytest-runner** (P3-W11-F08, ships in WO-0077) provides
   Python test discovery + execution + selective re-run by pytest
   node ID. The G8 fusion path (P3-W11-F09 — currently dep-blocked
   on F07 + F08, will become dep-blocked-on-F08-only after this ADR
   takes effect since F07 is deferred-by-ADR) will receive Python
   test results via this MCP route. The multi-language unification
   (Vitest, cargo nextest, Go, etc.) is the deferred portion. F09's
   dependency declaration includes `P3-W11-F07` as one of its deps;
   the planner re-scoping pass for F09 SHOULD treat F07's deferral
   as "satisfied via DEC-0021 admin marker" by precedent of how F03
   deferral (DEC-0020) interacts with F05's dependency on F03 — the
   group fusion runtime ships with whatever sources are available
   and the multi-language sources land via Phase 4/5/7 re-emission.

## Rationale

1. **No canonical upstream package exists.** The pre-flight sweep
   confirmed PyPI, npm, and GitHub do not publish a test-runner-mcp
   server matching the master-plan's intended multi-language scope.
   Pinning to a git SHA fails the AC32 immutable-tag policy that
   protects every other Phase 3 plugin manifest.

2. **DEC-0019 + DEC-0020 precedent** for upstream-blocker-driven
   deferral is already established (graphiti P3-W9-F10 deferred to
   Phase 7 due to upstream's non-spec-compliant stdio frame emission;
   ruff P3-W11-F03 deferred preemptively to Phase 4/7 due to no
   canonical Astral or community-canonical Ruff MCP server). DEC-0021
   applies the same shape preemptively (BEFORE attempt) rather than
   reactively (AFTER 2 failed attempts). This saves ~30 minutes of
   loop wall-time + executor cycles per attempt that DEC-0019
   absorbed under WO-0071 attempt-1 + attempt-2 BLOCKED.

3. **WO-0077 ships mcp-pytest-runner cleanly without test-runner-mcp.**
   `mcp-pytest-runner@0.2.1` (PyPI; latest publish 2025-12+, semver
   stable line) has a stable canonical upstream package with a clean
   `--help` warm-up surface and well-documented stdio MCP transport
   per its PyPI README — meets the WO-0069 template requirements
   without modification. Bundling test-runner-mcp with it would force
   the WO into either a 3-attempt blocked spiral (DEC-0019 graphiti
   pattern) or an ADR-justified git-SHA pin deviation that creates
   upstream-trust risk for an entire WO worth of work.

4. **test-runner-mcp coverage is not Phase 3 gate-blocking.** The
   Phase 3 testing pipeline (G8) ships with mcp-pytest-runner (via
   WO-0077) covering Python — the master plan's most heavily-tested
   language for the v0.1.0 fixture suite. JS/TS/Rust/Go test execution
   is operator-side via direct cargo/npm/go invocations (the LSP
   diagnostics bridge — P1-W5-F05, ✅ — surfaces compile errors and
   inline diagnostics for these languages already). F07's deferral
   does NOT weaken G8's Phase-3 surface — it postpones the
   multi-language unifier whose individual language paths are
   covered (Python via F08; others via existing toolchains).

5. **Anti-laziness rule "Loosen a coverage target. Without an ADR."
   is satisfied:** this ADR exists, and the substantive 85% / 75%
   coverage thresholds plus every other feature's gate criteria are
   unchanged. The Phase 3 gate-formula
   `gate(N) = all phase-N features pass=true` becomes
   `gate(3) = all phase-3 features pass=true OR blocked_reason marks
   ADR-deferred` — same widening DEC-0019 + DEC-0020 established for
   graphiti + ruff.

## Consequences

### Positive

- WO-0077 is unblocked. P3-W11-F08 (mcp-pytest-runner) ships cleanly
  through the WO-0069 / WO-0072 solo-manifest template; F07 → Phase
  4/7 audit; remaining Phase 3 features proceed through the loop.
- Phase 3 ship is unblocked along the same lines as DEC-0019 + DEC-0020.
  Phase 3 expected feature count drops from 45 → 42 (P3-W9-F10
  graphiti + P3-W11-F03 ruff + P3-W11-F07 test-runner-mcp all
  deferred); all three are mechanically grep-able for re-scoping.
- The pre-flight upstream-availability sweep documented above is now
  applied THREE TIMES (DEC-0019 reactive, DEC-0020 preemptive, DEC-0021
  preemptive) — the pattern is mature and reusable. Future planner
  passes that suspect a master-plan-anticipated MCP server doesn't
  exist yet should run the pypi-404 + npm-search + GitHub-search triad
  before bundling the feature into a WO.
- The substantive test-execution surface in Phase 3 is preserved via
  mcp-pytest-runner (F08, WO-0077) covering Python. Multi-language
  test-runner unification is deferred — operator-side direct
  invocation continues to work for non-Python tests (cargo nextest,
  vitest, go test, etc.).

### Negative / risks

- One Phase-3 feature (F07) ships in Phase 4/7 instead. Phase 3's
  "shipped" count drops from 45 to 42 (combined with DEC-0019 +
  DEC-0020). Phase 7 (or Phase 4) picks up the deferred features.
- Deferred work could regress if the next planner pass that should
  re-check forgets DEC-0021. **Mitigation**: the `blocked_reason`
  sentinel is mechanically grep-able
  (`"deferred-to-phase-4-or-7-per-DEC-0021"`); next planner pass
  for Phase 4 / Phase 7 MUST scan `feature-list.json` for any
  `blocked_reason` mentioning `phase-4-per-DEC-NNNN` /
  `phase-7-per-DEC-NNNN` and re-emit those WOs. The DEC-0019 +
  DEC-0020 + DEC-0021 chain establishes the convention.
- Risk that an authoritative test-runner-mcp server still hasn't
  emerged by Phase 7. **Mitigation**: defer-again ADR with fresh
  upstream check, OR re-spec F07 to (a) wrap individual-language
  test-runners under the G8 testing.* namespace (mcp-vitest-runner
  if it exists, mcp-cargo-nextest-runner if it exists, etc.) as
  separate manifests — the multi-language unifier surface degrades
  to per-language manifests with shared G8 fusion logic.
- F09 (G8 test discovery — convention-based + import-based + KG
  tested_by relations) lists F07 as a dependency. With F07 deferred,
  F09's dep graph reduces to F08 + P3-W9-F03 (cross-group executor,
  ✅). The next planner pass that bundles F09 SHOULD treat F07's
  deferral as "admin-satisfied via DEC-0021" — same mitigation
  pattern as F05 / DEC-0020 (Ruff). Pure planner-side bookkeeping;
  no UCIL source impact.

### Neutral

- The master-plan §4.8 line 401 anticipation of "test-runner-mcp" is
  not in error — Anthropic, the MCP servers org, or a community
  vendor may publish one at any time, and the re-scoping pass should
  pick it up automatically by name. No master-plan amendment is
  required (and per the immutability contract, none is permitted
  without escalation).
- Future planner passes that need similar preemptive deferrals for
  other master-plan-anticipated-but-not-yet-published MCP servers
  (e.g., the master plan §4.7 P1 list — Sonarqube MCP, Snyk MCP,
  Trivy MCP, RuboCop MCP — and any future §4.8 entries) should
  follow the DEC-0019/0020/0021 shape: pre-flight availability sweep
  + ADR + scope-out from current WO.

## Revisit trigger

When `/phase-start 4` runs (or `/phase-start 7` if the Phase 4
planner pass also defers), the planner pass MUST:
1. Grep `feature-list.json` for
   `"blocked_reason": "deferred-to-phase-4-or-7-per-DEC-NNNN-*"`
   — finds P3-W11-F07 (and any future siblings).
2. Re-run the pre-flight availability sweep:
   - `curl -sLI https://pypi.org/pypi/test-runner-mcp/json`
     (expect non-404)
   - `curl -sLI https://registry.npmjs.org/test-runner-mcp`
     (expect non-404)
   - `gh search repos test-runner-mcp --owner anthropic --owner modelcontextprotocol`
     (expect at least one stable-released repo)
   - `gh repo view modelcontextprotocol/test-runner-mcp`
     (expect existing public repo)
3. If ANY of the above returns a published package with stable
   releases, emit a new solo or paired-manifest WO using the
   WO-0069 / WO-0072 / WO-0077 template.
4. If NONE has emerged, write a **new** ADR (DEC-NNNN) propagating
   the deferral; cite DEC-0021 (and DEC-0019 / DEC-0020) as the
   lineage; bump the deferral target to Phase 7 hardening (or later)
   per the elapsed timeline.
5. If a test-runner-mcp server emerges between Phase 3 ship and
   Phase 4 start, the next planner pass MAY pick it up immediately
   rather than waiting for `/phase-start 4`. The trigger in that case
   is "first feature WO of Phase 4" — the planner pass scans for
   blocked_reason sentinels per the standing protocol.
6. The re-scoping WO SHOULD also re-evaluate the F09 (G8 test
   discovery) dependency on F07; if F07 is finally available at
   that time, F09 can consume it; if F07 is still deferred, F09's
   dep graph reduces to F08-only + P3-W9-F03 (per DEC-0021 §Decision
   point 5 admin-marker pattern).

## References

- `ucil-build/feature-list.json` — `P3-W11-F07` entry frozen
  (`id`, `description`, `acceptance_tests`, `dependencies` immutable
  per the `freeze: feature oracle v1.0.0` contract).
- `ucil-build/decisions/DEC-0019-defer-graphiti-plugin-to-phase-7.md`
  — direct precedent for upstream-blocker-driven deferral (reactive).
- `ucil-build/decisions/DEC-0020-defer-ruff-mcp-plugin-no-canonical-upstream.md`
  — direct precedent for preemptive upstream-availability deferral.
- `ucil-build/work-orders/0077-mcp-pytest-runner-plugin-manifest.json`
  — concurrent WO that ships F08 cleanly per WO-0072/0076 template.
- `ucil-master-plan-v2.1-final.md` §4.8 line 397-410 (Group 8 Testing)
  and §17.4 (Phase 7 hardening — MCP robustness sweep).
- Pre-flight availability sweep documented in
  `ucil-build/work-orders/0077-mcp-pytest-runner-plugin-manifest.json`
  RFR pre-flight section (executor MUST cite this ADR's research in
  the WO-0077 RFR's `## Lessons applied` section).
