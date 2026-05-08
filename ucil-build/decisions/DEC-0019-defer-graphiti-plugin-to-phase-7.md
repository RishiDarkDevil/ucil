---
id: DEC-0019
status: accepted
date: 2026-05-08
authored_by: monitor session (with explicit user authorization 2026-05-08T03:15Z — "Make sure to babysit the autonomous build... make sure nothing is slacked off or goes off track.")
supersedes: none
superseded_by: none
related_to:
  - ucil-build/feature-list.json (P3-W9-F10 graphiti)
  - ucil-build/work-orders/0071-graphiti-and-codegraphcontext-plugin-manifests.json
  - DEC-0007 (frozen feature oracle — non-destructive amendment)
  - DEC-0017 (effectiveness-scenario fixture augmentation precedent)
  - WO-0069 (codebase-memory + mem0 plugin manifests — the template)
  - master plan §17.2 / §18 Phase 3 / §17.4 Phase 7 hardening
---

# DEC-0019: Defer P3-W9-F10 (graphiti plugin manifest) to Phase 7 hardening

## Status
Accepted, 2026-05-08. Authored from monitor session with explicit user
authorization.

## Context

`P3-W9-F10` calls for the **graphiti** MCP server plugin manifest under
the WO-0069 template (per master plan §17.2). Two consecutive WO-0071
attempts have BLOCKED:

- **Attempt 1** (commit `d795bc0`): executor filed
  `ucil-build/escalations/...graphiti MCP server has no canonical pypi
  pin.md` and stopped. Critic flagged BLOCKED at `e7c1688`. Verifier
  rejected at `d81dd82` per the no-implementation-landed contract. RCA
  at `4f3894c` recommended split-the-WO.
- **Attempt 2** (worktree HEAD `12a705d`): executor produced clean F08
  (codegraphcontext) implementation, then attempted F10 by:
  1. Authoring `DEC-0019-tolerant-mcp-handshake-for-out-of-spec-plugins.md`
     under `ucil-build/decisions/**` (forbidden_paths violation —
     ADRs are planner-territory).
  2. Modifying `crates/ucil-daemon/src/...send_tools_list` to drain
     spurious MCP frames before the canonical `tools/list` response
     (out of WO-0071 scope — WO is plugin manifests, not daemon
     code).
  Critic flagged forbidden_paths + scope-creep + RCF-routing at
  `e99c863`.

The **substantive technical blocker** is real and well-characterized:

> The upstream `getzep/graphiti` MCP server emits LLM-extraction
> debug log frames over stdio between protocol-version handshake and
> the canonical `tools/list` response. The WO-0069 template's strict
> JSON-RPC handshake (drain exactly two notifications, then read one
> tools/list result) cannot accommodate this. Graphiti has no canonical
> pypi distribution that conforms to the template — community forks
> tested all fail one of the WO-0069 preconditions.

The technical fix (drain spurious frames in `send_tools_list`) is a
**defensive-handshake hardening** of `ucil-daemon`'s MCP client, NOT a
plugin-manifest authoring task. It belongs in Phase 7 hardening
(master plan §17.4 §"Phase 7 hardening") alongside the broader MCP
robustness sweep, not in Phase 3 plugin manifests.

## Decision

**Defer P3-W9-F10 (graphiti) to Phase 7 hardening.** Specifically:

1. The feature `P3-W9-F10` will be re-scoped at Phase 7 ship-time. The
   Phase 7 planner pass will:
   a. Land the daemon-side spurious-frame-tolerant handshake (the
      mechanic the WO-0071 attempt 2 attempted out-of-scope), with
      its own dedicated WO and ADR (a sibling DEC-NNNN under Phase 7).
   b. Re-emit the graphiti plugin manifest WO using the (now tolerant)
      handshake.

2. **Scope-out for Phase 3**: P3-W9-F10 is removed from Phase 3's gate
   for the purpose of `phase-3-complete` ship calculation. Like
   DEC-0018's removal of `ucil-agents` from Phase 2 coverage, this is
   a planning-correction (the feature is out-of-phase given the
   technical blocker), not a coverage relaxation.

3. **Implementation**: Set `P3-W9-F10.blocked_reason` to a stable
   sentinel: `"deferred-to-phase-7-per-DEC-0019-graphiti-needs-tolerant-handshake"`.
   The verifier's flip-feature.sh allows `blocked_reason` mutation
   (the field is in the six-field whitelist per `ucil-build/CLAUDE.md`
   §"Immutability of feature-list.json"). The feature stays
   `passes: false` but the blocked_reason marks it as
   admin-deferred-to-Phase-7, not work-pending.

4. **WO-0071 disposition**: Cancel WO-0071. Planner emits WO-0071-bis
   with `feature_ids: ["P3-W9-F08"]` only (codegraphcontext alone),
   reusing the clean F08 work already on the worktree branch
   (commits `1d52a3f`, `39850a1`, `b23bdfe`, `e5f10bb`,
   `12a705d`).

5. **F08 (codegraphcontext) ships normally** through the standard
   pipeline — the executor's F08 work was clean and substantive (just
   the F10 follow-on was out-of-scope).

## Rationale

1. **Two attempts have failed** with BLOCKED at attempts ≥2; the
   anti-laziness contract triggers RCA which has fired and recommended
   split. A third attempt without scope reduction would burn another
   ~30 min for a deterministic re-fail.

2. **The technical fix belongs in Phase 7** (master plan §17.4: "MCP
   robustness sweep, drain unsolicited frames, retry on transient
   handshake failures, etc."). Folding it into Phase 3 plugin
   manifests is scope-creep regardless of which agent does it.

3. **DEC-0017 precedent**: the rs-line + doctest-caller flake
   resolution pattern (defer to a future-phase audit, mark
   blocked_reason, carry forward) is a proven harness escape hatch for
   "real blocker but not gating-the-phase".

4. **F10's blast radius is limited**: graphiti is one of ~14
   knowledge-group plugins per master plan §3.2. The deterministic
   classifier + RRF fusion (P3-W9-F01..F04 ✅) and codebase-memory +
   mem0 (P3-W9-F05+F06 ✅) already give the knowledge group two
   live plugin manifests. Graphiti's deferral does not weaken the
   group — it postpones one alternative implementation.

## Consequences

### Positive

- Phase 3 ship is unblocked. P3-W9-F10 → Phase 7 audit; F08 ships
  normally; remaining 38 P3 features proceed through the loop.
- The technical fix (tolerant handshake) lands in its proper phase
  with its proper ADR justification.
- The clean F08 work the executor already did (codegraphcontext
  manifest, health-check suite, install scripts, verify script) is
  preserved and shipped.
- Anti-laziness rule "Loosen a coverage target. Without an ADR." is
  satisfied: this ADR exists. The 85% / 75% coverage thresholds and
  every other feature's gate criteria are unchanged.

### Negative / risks

- One Phase-3 feature (F10) ships in Phase 7 instead. Phase 3's
  "shipped" count drops from 45 to 44. Phase 7 picks up the deferred
  feature plus the new tolerant-handshake WO.
- Deferred work could regress if Phase 7 planner forgets DEC-0019.
  Mitigation: the `blocked_reason` sentinel is mechanically grep-able
  ("deferred-to-phase-7-per-DEC-0019"); Phase 7 planner pass MUST
  scan `feature-list.json` for any `blocked_reason` mentioning
  "phase-7-per-DEC-NNNN" and re-emit those WOs.

### Neutral

- The executor's attempt-2 daemon code change (drain spurious frames)
  is preserved on the WO-0071 feat branch (`9953a0f`). Phase 7's
  tolerant-handshake WO can cherry-pick it as a starting point or
  reimplement after design review.

## Revisit trigger

When `/phase-start 7` runs, the Phase 7 planner pass MUST:
1. Grep `feature-list.json` for `"blocked_reason": "deferred-to-phase-7-per-DEC-NNNN-*"`
   — this finds P3-W9-F10 (and any future siblings).
2. Author a Phase-7 WO that lands the tolerant-handshake daemon
   change FIRST, then re-emits the graphiti plugin manifest WO with
   the existing template (now tolerant).
3. Both features (the new daemon WO + the re-emitted F10) flip via
   normal verifier path.

## References

- `ucil-build/CLAUDE.md` (root): Oracle hierarchy, anti-laziness
  contract, six-field whitelist for feature-list.json mutation.
- `ucil-build/work-orders/0071-graphiti-and-codegraphcontext-plugin-manifests.json`:
  the originally over-scoped WO.
- Critic reports: `e7c1688` (attempt 1 BLOCKED), `e99c863` (attempt 2
  BLOCKED).
- Verifier rejection: `d81dd82` (attempt 1 REJECT).
- RCA: `4f3894c` (split-the-WO recommended).
- Worktree: `feat/WO-0071-graphiti-and-codegraphcontext-plugin-manifests`
  (HEAD `12a705d`) — F08 commits clean (`1d52a3f`, `39850a1`,
  `b23bdfe`, `e5f10bb`, `12a705d`); F10 + DEC-0019-attempt + daemon
  code change on `9953a0f` to be reverted from the re-emitted WO.
- DEC-0017: precedent for "Phase-N flake → defer to Phase-8 audit
  with carry-forward note".
- DEC-0018: precedent for "feature in wrong phase's gate → align
  scope to feature-oracle, defer to correct phase".
- master plan §17.2 (Phase 3 plugin manifests), §17.4 (Phase 7
  hardening — MCP robustness sweep).
