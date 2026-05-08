---
slug: wo-0071-stale-dispatch-dec-0019-supersedes
created_at: 2026-05-08T03:24:00Z
created_by: executor (WO-0071 retry 3 dispatch — refused at session-start)
blocks_loop: true
severity: critical
requires_planner_action: true
related_to:
  - WO-0071 (cancelled per DEC-0019)
  - DEC-0019 (defer-graphiti-plugin-to-phase-7) — main:7290ebf
  - ucil-build/rejections/WO-0071.md (retry 2 REJECT)
  - ucil-build/verification-reports/root-cause-WO-0071.md (RCA — `Who: planner` + `Do NOT route to executor`)
  - ucil-build/feature-list.json (P3-W9-F08 attempts=2; P3-W9-F10 attempts=2)
resolved: false
---

# Escalation: WO-0071 retry-3 dispatch is stale — DEC-0019 supersedes the dispatch

## TL;DR

The dispatch prompt for this executor session asks me to "apply the
RCF's recommended remediation" against WO-0071. **DEC-0019 (committed
on main as `7290ebf`, dated 2026-05-08) supersedes the RCF and CANCELS
WO-0071** — the planner is supposed to emit WO-0071-bis (F08-only)
before the next executor dispatch. The replacement WO has NOT been
emitted; only `0071-graphiti-and-codegraphcontext-plugin-manifests.json`
sits in `ucil-build/work-orders/`. Acting on this stale dispatch would
either:

1. Repeat the retry-2 policy violation that caused
   `forbidden-paths-violation+scope-creep+rcf-routing-bypass` rejection
   (since the only path to land F10 still requires `plugin_manager.rs`
   edits + an ADR — both out of `forbidden_paths`/`scope_in`); OR
2. Land an F08-only no-op interpretation under the existing WO scope
   (which still includes F10 in `feature_ids`/`acceptance_criteria` —
   verifier would reject F10 portion ⇒ `attempts=3` ⇒ 3-strikes halt).

The RCF (`ucil-build/verification-reports/root-cause-WO-0071.md:140-142,
390-393`) is explicit: **"Do NOT route this RCA back to the executor"**
and **"Do NOT spawn a verifier on this feat branch again"**. The
retry-3 dispatch crosses the first directive again. This escalation
halts the loop so the planner can act per DEC-0019.

## Evidence

### 1. DEC-0019 cancels WO-0071 and instructs the planner to emit WO-0071-bis

`ucil-build/decisions/DEC-0019-defer-graphiti-plugin-to-phase-7.md`
lines 88-96 (committed on `main` at `7290ebf`):

```
4. **WO-0071 disposition**: Cancel WO-0071. Planner emits WO-0071-bis
   with `feature_ids: ["P3-W9-F08"]` only (codegraphcontext alone),
   reusing the clean F08 work already on the worktree branch
   (commits `1d52a3f`, `39850a1`, `b23bdfe`, `e5f10bb`,
   `12a705d`).

5. **F08 (codegraphcontext) ships normally** through the standard
   pipeline — the executor's F08 work was clean and substantive (just
   the F10 follow-on was out-of-scope).
```

DEC-0019 also instructs (lines 80-86):

```
3. **Implementation**: Set `P3-W9-F10.blocked_reason` to a stable
   sentinel: `"deferred-to-phase-7-per-DEC-0019-graphiti-needs-tolerant-handshake"`.
   The verifier's flip-feature.sh allows `blocked_reason` mutation
   (the field is in the six-field whitelist per `ucil-build/CLAUDE.md`
   §"Immutability of feature-list.json"). The feature stays
   `passes: false` but the blocked_reason marks it as
   admin-deferred-to-Phase-7, not work-pending.
```

### 2. Replacement WO has NOT been emitted

```
$ ls ucil-build/work-orders/ | grep -E '0071|0072|0073'
0071-graphiti-and-codegraphcontext-plugin-manifests.json
```

No `0071-bis-...json`, no `0072-...json`, no `0073-...json` exists.
The planner has not yet acted on DEC-0019.

### 3. F10's blocked_reason is still null

```
$ jq '.features[] | select(.id == "P3-W9-F10") | {id, blocked_reason, attempts}' ucil-build/feature-list.json
{
  "id": "P3-W9-F10",
  "blocked_reason": null,
  "attempts": 2
}
```

DEC-0019 step 3 mandates a sentinel. The verifier has not yet applied
it (and the verifier is the only sanctioned mutator of that field per
the six-field whitelist).

### 4. WO-0071 still lists F10 in `feature_ids` and `acceptance_criteria`

```
$ jq '.feature_ids' ucil-build/work-orders/0071-graphiti-and-codegraphcontext-plugin-manifests.json
[ "P3-W9-F08", "P3-W9-F10" ]
```

Acceptance criteria #2, #5 (graphiti test addition), #6 (g3 test extension
to 3 tests), #7 (graphiti cargo-test selector), #10 (P3-W9-F10.sh),
#11 (M1 graphiti mutation), #15 (g3 test must contain 3 async fns),
#22 (both feature ids flippable). All require landing F10 work, which
DEC-0019 has just deferred to Phase 7.

The work-order JSON has NOT been updated to drop F10 — that's the
planner's job and is part of the WO-0071-bis emission.

### 5. Two prior RCF directives forbid this dispatch

`ucil-build/verification-reports/root-cause-WO-0071.md:140-142`:

> * **Do NOT route this RCA back to the executor** — the executor
>   cannot unblock F10 with the WO-0071 scope as currently written;
>   the next action belongs to the planner.

`ucil-build/verification-reports/root-cause-WO-0071.md:390-393`:

> * **Do NOT route this RCA back to the executor.** This is a
>   re-statement of the prior RCA's directive; the dispatch step from
>   RCA → next-action is the routing-defect Layer B. The next outer-loop
>   iteration must be a planner session, not an executor session.

The retry-3 dispatch crossing this directive is the same Layer B
defect the RCA already flagged for retry-2.

### 6. 3-strikes halt is one rejection away

```
$ jq '.features[] | select(.id == "P3-W9-F08" or .id == "P3-W9-F10") | {id, attempts}' ucil-build/feature-list.json
{ "id": "P3-W9-F08", "attempts": 2 }
{ "id": "P3-W9-F10", "attempts": 2 }
```

`.claude/CLAUDE.md` anti-laziness contract + escalation triggers and
`ucil-build/CLAUDE.md` triage section: `attempts >= 3` halts the loop
unconditionally. Any executor session against WO-0071 that produces
work the verifier can REJECT (it can — see Blocker analysis below)
trips this halt with the underlying issue still unaddressed.

## Why no in-scope path exists

I considered three possible interpretations of the dispatch:

### Interpretation A — implement per RCF (Sub-option B)

The RCF's "Sub-option B" requires:
- Editing `crates/ucil-daemon/src/plugin_manager.rs::send_tools_list`
  (out of `scope_in`, was retry-2 Blocker #2).
- Writing a new ADR under `ucil-build/decisions/**` (in
  `forbidden_paths`, was retry-2 Blocker #1).

Verbatim repeat of retry-2 → verbatim repeat of the rejection.

### Interpretation B — implement per DEC-0019 (cancel-and-emit-bis)

DEC-0019 explicitly assigns this to the **planner**, not the executor.
The executor cannot legitimately:
- Cancel an in-flight WO (`scripts/spawn-planner.sh` / planner-only).
- Emit a new WO-0071-bis JSON (planner is the only sanctioned mutator
  of `ucil-build/work-orders/`).
- Modify F10's `blocked_reason` in `feature-list.json` (verifier-only,
  six-field whitelist).

If I tried to do any of these, the pre-commit hook would (correctly)
block them as out-of-role mutations.

### Interpretation C — F08-only land under existing WO-0071 scope

Cherry-pick the existing F08 commits (`1d52a3f`, `39850a1`,
`b23bdfe`, plus F08-portion of `e5f10bb`) onto a clean `feat/WO-0071-…`
branch and call it done. **Problem**: WO-0071 still lists `P3-W9-F10`
in `feature_ids` and ACs #5–#11 still require landing F10 work. A
verifier session would either:
- Run all ACs and REJECT for missing F10 (→ `attempts=3` halt); OR
- Refuse to run because `feature_ids` includes a feature DEC-0019 has
  deferred (no current verifier-side handling for this case).

The RCF specifically considered and ruled out this interpretation
(`root-cause-WO-0071.md:185-195` H2): *"A unilateral F08-only retry-2
by the executor would itself be a (more subtle) scope-creep. The
correct cut is at the planner layer, not the executor's
interpretation of the WO."*

### Conclusion

There is **no executor-implementable path** to satisfy WO-0071 as
written without a planner-side WO emission. The dispatch is stale —
the planner action DEC-0019 mandates has not happened yet.

## What needs to happen (planner action required)

Per DEC-0019 §"Decision" steps 1-5 and §"Revisit trigger":

### Action 1 (planner) — emit WO-0071-bis

Write `ucil-build/work-orders/0071-bis-codegraphcontext-only.json` (or
`0072-…`, planner picks the id; DEC-0019 line 88 names it
"WO-0071-bis"). Concrete shape:

- `feature_ids: ["P3-W9-F08"]` only.
- `branch: feat/WO-0071-bis-codegraphcontext-only` (new branch off
  main; the existing `feat/WO-0071-…` branch is NOT reused — it has
  the rejected `9953a0f` commit on it).
- `forbidden_paths`: same as WO-0071 (no widening).
- `scope_in`: items #2, #3, #4 (F08-applicable subset), #6 (F08
  install script), #8 (g4_plugin_manifests.rs new file), #10 (F08.sh
  verify script), #13 (M2 mutation contract), plus the F08-applicable
  subset of #14 (M3 on either test).
- Cherry-pick map: per RCF
  `root-cause-WO-0071.md:230-256` — `1d52a3f`, `39850a1`, `b23bdfe`,
  and the F08-applicable 2 lines of `e5f10bb`.
- Estimated complexity: small (≈ 4 commits; cherry-pick + verify).

### Action 2 (verifier) — apply F10's blocked_reason sentinel

Once WO-0071-bis is in flight, the verifier should also flip:

```
P3-W9-F10.blocked_reason = "deferred-to-phase-7-per-DEC-0019-graphiti-needs-tolerant-handshake"
```

via `flip-feature.sh` (six-field whitelist allows it). DEC-0019 line
80-86 mandates this sentinel for the Phase 7 planner pass to grep on.

### Action 3 (triage / docs-writer) — re-author DEC-0019 provenance

DEC-0019's `authored_by:` field reads "monitor session (with explicit
user authorization 2026-05-08T03:15Z)". The RCF `root-cause-WO-0071.md:294-301`
flagged a similar concern for the prior (now-superseded) DEC-0019
attempt. This is informational; the current DEC-0019's authorization
is documented inline and is fine.

### Action 4 (orchestrator) — fix dispatch-layer Layer B defect

Out-of-scope for this escalation, but flagged twice now in two RCF
docs and one rejection: the dispatch step between rejection/RCF and
retry should respect the RCF's `Who:` directive and refuse to
re-dispatch the same agent type the RCF excluded. Per
`root-cause-WO-0071.md:402-409`, this is a Bucket B
harness-improvement candidate (suggested slug:
`dispatch-layer-rca-routing-respect-needed`). I am NOT writing that
escalation here — this single escalation would fall back to its
sibling fix anyway once a new dispatch arrives.

## What I did NOT do

I deliberately did not:

1. Touch the worktree at `/home/rishidarkdevil/Desktop/ucil-wt/WO-0071`
   (HEAD `12a705d`, working tree clean per `git status`). The retry-2
   commits are forward-only and pushed; the planner's WO-0071-bis can
   cherry-pick from them.
2. Modify `feature-list.json` (verifier-only, six-field whitelist).
3. Modify or cancel `0071-graphiti-and-codegraphcontext-plugin-manifests.json`
   (planner-only mutation).
4. Write a new ADR (planner-only mutation; also `forbidden_paths`
   under WO-0071's contract — this is exactly what retry-2 did and
   was rejected for).
5. Edit `crates/ucil-daemon/src/plugin_manager.rs` (out-of-scope; was
   retry-2 Blocker #2).
6. Cherry-pick anything onto a new branch (this is WO-0071-bis
   territory; no WO-0071-bis JSON exists).

The escalation file (this file) is the only artifact I'm landing.

## Suggested resolution path

1. **Halt the loop** (this escalation has `blocks_loop: true,
   resolved: false`).
2. **Spawn the planner** (`/phase-start 3` or `/replan` is sufficient
   — the phase-3 CLAUDE.md and DEC-0019 both point at the right
   action).
3. **Planner emits WO-0071-bis** per Action 1 above.
4. **Outer loop dispatches a fresh executor session** against
   WO-0071-bis (NOT WO-0071). The new session will use a fresh
   feat branch off main and cherry-pick `1d52a3f`, `39850a1`,
   `b23bdfe`, plus the F08-applicable portion of `e5f10bb` from the
   existing `feat/WO-0071-…` branch.
5. **Verifier flips P3-W9-F08:passes=true** AND
   **P3-W9-F10:blocked_reason** to the DEC-0019 sentinel.
6. **Phase 3 ship-time** — `gate-check.sh 3` should treat
   P3-W9-F10 as deferred (per DEC-0019 §"Decision" step 2:
   "P3-W9-F10 is removed from Phase 3's gate"). If gate-check.sh
   doesn't auto-honour blocked_reason sentinels, the standing
   protocol carry from DEC-0017 / DEC-0018 covers manual
   verifier-confirmation per the precedent set there.

## Counter-status post-escalation

P3-W9-F08 = 2 (unchanged — no verifier rejection appended for this
escalation). P3-W9-F10 = 2 (unchanged). The 3-strikes halt threshold
is NOT yet tripped; this escalation prevents tripping it
unnecessarily.
