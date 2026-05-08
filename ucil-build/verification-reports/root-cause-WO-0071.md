# Root Cause Analysis: WO-0071 (graphiti + codegraphcontext plugin manifests) — retry 2 rejection

**Analyst session**: rca-2026-05-08-wo-0071-r2
**Work-order**: WO-0071
**Features**: P3-W9-F08 (codegraphcontext), P3-W9-F10 (graphiti)
**Attempts before this RCA**: P3-W9-F08 = 2, P3-W9-F10 = 2
**Branch**: `feat/WO-0071-graphiti-and-codegraphcontext-plugin-manifests` @ `12a705d25e3d51ea7a2304c4e5bfd17c122aa143`
**Merge base**: `fee489608088bd3dae7002b26c149b2e38d6d396`
**Supersedes**: prior RCA section "Root cause (hypothesis, 95% confidence)" at this same path (re-uses the diagnosis; adds a new top-level diagnosis for the retry-2 cycle).

> **TL;DR — same remediation as the prior RCA, with the dispatch-layer
> defect now confirmed**: the planner must split WO-0071 into WO-0072
> (F08-only) and WO-0073 (F10 + DEC-0019 + harness extension). The
> retry-2 cycle was a *dispatch-routing* failure, not an executor-
> capability failure: the orchestrator re-dispatched the executor on the
> still-open WO-0071 scope despite the prior RCA's explicit `Who: planner`
> + `Do NOT route this RCA back to the executor` directives
> (`root-cause-WO-0071.md:142-244`). The executor honoured the dispatch
> prompt and applied the deferred WO's "Sub-option B" content in-place,
> which crossed `forbidden_paths` and `scope_in`. Counter status: one
> more rejection on either feature trips the 3-strikes halt
> (`.claude/CLAUDE.md:69`).

## Failure pattern

Two consecutive verifier rejections on WO-0071, with rejection #1 and
rejection #2 having **fundamentally different defect classes** but the
**same underlying cause** (planner-template defect in WO-0071's bundle).

```
$ git log fee4896..12a705d --oneline
12a705d docs(rfr): WO-0071 retry 2 RFR + escalation resolution
e5f10bb fix(verify/P3-W9-F08): copy fixture to tmpdir + use correct query_type
9953a0f feat(daemon): drain spurious MCP frames in send_tools_list (DEC-0019)
b23bdfe test(verify): replace P3-W9-F08 stub with codegraphcontext acceptance
39850a1 test(daemon): add g4 plugin manifest health-check suite (codegraphcontext)
1d52a3f feat(plugins/architecture): land codegraphcontext MCP manifest at v0.4.7
d795bc0 chore(escalation): WO-0071 — graphiti MCP server has no canonical pypi pin
```

* **Retry 1 / first rejection** (`ucil-build/rejections/WO-0071.md`
  prior copy, classification `blocked-by-executor-escalation`,
  superseded on disk): feat carried only `d795bc0`, an
  escalation-only commit. No source landed. Verifier rejected for
  no-implementation. **Cause**: F10 graphiti has no pypi-published MCP
  server that satisfies the WO-0069 template's three preconditions
  (boots without external infra, respects `notifications/initialized`
  no-response rule, ships canonical console script). Diagnosed in the
  prior RCA `root-cause-WO-0071.md:60-71`.
* **Retry 2 / second rejection** (current
  `ucil-build/rejections/WO-0071.md` at retry=2, classification
  `forbidden-paths-violation+scope-creep+rcf-routing-bypass`): feat
  added 6 substantive commits (`1d52a3f` through `12a705d`).
  Implementation is **technically clean** per the critic
  (`ucil-build/critic-reports/WO-0071.md:36-41,144-170`) — both
  manifests land, real `uvx`-driven subprocess + JSON-RPC handshake
  exercised end-to-end, M1/M2/M3 mutation contract applied + restored,
  RFR-claimed line coverage 89.15% on `ucil-daemon`, all anti-laziness
  scans (stubs, mocked critical deps, skipped tests, weak assertions,
  hallucinated paths) pass. **Three policy-level Blockers** dispose of
  the work upstream of per-AC evaluation:
  1. `forbidden_paths` violation: commit `9953a0f` adds 159 LOC under
     `ucil-build/decisions/DEC-0019-tolerant-mcp-handshake-for-out-of-spec-plugins.md`
     (status `accepted`); WO-0071 line 121 lists `ucil-build/decisions/**`
     in `forbidden_paths`. Per `ucil-build/CLAUDE.md` "Decisions (ADRs)"
     section, the mutator is **planner**, not executor. Per the root
     `CLAUDE.md` Oracle hierarchy section, conflicts STOP and emit
     `ucil-build/decisions/proposed-*.md` (a `proposed-` prefix); the
     executor wrote a final `DEC-0019-…md` instead.
  2. Out-of-scope source modification: commit `9953a0f` rewrites
     `crates/ucil-daemon/src/plugin_manager.rs::send_tools_list`
     (+70/−12 LOC, `MAX_DRAIN_FRAMES = 4` bounded read-loop). None of
     WO-0071's `scope_in` items #1–#31 authorize editing
     `plugin_manager.rs`; #21 explicitly inverts the direction with
     *"these features add data files (TOML manifests) + integration
     tests + helper scripts, NOT new orchestration code"*
     (`ucil-build/work-orders/0071-graphiti-and-codegraphcontext-plugin-manifests.json:34`).
  3. RCF top-level routing bypass: prior RCA at line 142
     specifies **`Who: planner`** with the remediation **"Re-scope
     WO-0071 by splitting it. Concretely, emit two replacement
     work-orders"**. The retry-2 session lifted the deferred WO's
     "Sub-option B" content (DEC-0019 + tolerant handshake harness +
     graphiti manifest) and applied it inside the still-open WO-0071
     scope, without a planner-issued replacement WO. The RFR
     (`ucil-build/work-orders/0071-graphiti-and-codegraphcontext-plugin-manifests-ready-for-review.md:213-216`)
     and the escalation tail
     (`ucil-build/escalations/20260508T0210Z-wo-0071-graphiti-mcp-handshake-and-infra-blocker.md`
     resolution-note, on the feat branch) both explicitly acknowledge
     this deviation.

The two rejection cycles **share the underlying cause** (planner-side
WO-0071 bundle is unmeetable as written) but **differ at the surface
defect** (no-implementation vs. policy-boundary violation). Both
surfaces are foreseen in the prior RCA's remediation —
`root-cause-WO-0071.md:144-187`.

## Root cause (hypothesis, 98% confidence) — TWO STACKED LAYERS

### Layer A (carried from prior RCA): planner-template bundling defect

**The WO-0071 work-order bundles two features whose upstream landscapes
are asymmetric.** F08 codegraphcontext is implementable as-prescribed;
F10 graphiti has no pypi-published MCP server that satisfies the
WO-0069 template's three preconditions simultaneously. Verified in
detail at `root-cause-WO-0071.md:50-99` (prior section); evidence
re-confirmed by the retry-2 commits — F08 landed cleanly under the
template (`1d52a3f`, `39850a1`, `b23bdfe`), F10 only landed because
the executor introduced a tolerant-handshake harness extension
(`9953a0f`) that the prior RCA explicitly assigned to **planner**, not
executor.

### Layer B (new in this RCA): orchestrator-dispatch routing defect

**The prior RCA's `Who: planner` + `Do NOT route to executor`
directives were ignored at the dispatch step.** The retry-2 session
was opened with the prompt "apply RCF remediation" against the
existing WO-0071 scope. Evidence (executor's own admission, retry-2
RFR at
`ucil-build/work-orders/0071-graphiti-and-codegraphcontext-plugin-manifests-ready-for-review.md:213-216`):

> The RCF's top-level remediation routed the work to the planner for
> WO-splitting. This retry-2 session deviated from that routing: the
> orchestrator dispatched the executor directly with 'apply RCF
> remediation' rather than routing to the planner.

And mirrored in the escalation tail (feat-branch view at
`12a705d:ucil-build/escalations/20260508T0210Z-wo-0071-graphiti-mcp-handshake-and-infra-blocker.md`,
the `## Resolution (2026-05-08, executor retry 2)` block):

> The orchestrator routed the rejected WO-0071 to a retry-2 executor
> session with the directive "Apply the RCF's recommended remediation"
> referencing `ucil-build/verification-reports/root-cause-WO-0071.md`.
> The RCF recommended Sub-option B … which the retry-2 executor
> implemented in-place under the existing WO-0071 scope rather than
> waiting for a planner-side split.

The prior RCA at `root-cause-WO-0071.md:235-249` ("Outer-loop
routing") expressly wrote:

> * **Do NOT route this RCA back to the executor** — the executor
>   cannot unblock F10 with the WO-0071 scope as currently written;
>   the next action belongs to the planner.
> * **Do NOT spawn a verifier on this branch as-is** — the F10 portion
>   is by-construction unmeetable.
> * **Increment counters only** …

That instruction was followed by the verifier (correctly REJECTed,
correctly incremented `attempts` 0→1) but was **not** honoured at the
dispatch layer between rejection #1 and retry #2.

### How the layers combine

The retry-2 executor faced an impossible choice:

* If the executor honoured the RCA's `Do NOT route to executor` rule
  and refused the dispatch, it would short-circuit the dispatch with
  another escalation — but a second escalation-only commit on a
  branch already at attempts=1 would be a near-certain second
  rejection, walking into a 3-strikes halt with the underlying issue
  unaddressed.
* If the executor took the dispatch prompt at face value and applied
  the RCF's "Sub-option B" content under the existing WO scope, it
  would by-construction violate `forbidden_paths` and `scope_in` (the
  Sub-option B content is documented in `root-cause-WO-0071.md:166-178`
  and explicitly requires editing `plugin_manager.rs` and writing
  DEC-NNNN — both of which are out of WO-0071's scope_in/forbidden_paths).
  The executor took this branch, in good faith, and the RFR transparently
  acknowledges the deviation
  (`ready-for-review.md:217-220`).

The retry-2 cycle therefore is not a sign that the executor agent is
broken; it is a sign that **the dispatch step between rejection and
retry is missing a "this RCA mandates a planner-split, not an executor-
retry" branch**. The next planner-emitted WO is the correct routing.

## Hypothesis tree (alternatives, ranked)

* **H1 (98%)** — *Layer A + Layer B as above; planner must split + the
  dispatch layer must respect RCA's `Who: planner`.* Falsifiable only
  by finding a within-WO-0071-scope route to make F10 land — which the
  prior RCA already enumerated and ruled out at line 60-71. The
  retry-2 cycle is itself a falsification attempt that confirmed the
  ruling: F10 cannot land without `plugin_manager.rs` edits + a new
  ADR, both of which are out of WO-0071's scope.
* **H2 (1%)** — *The retry-2 executor over-extended; a more
  conservative retry would have landed F08 alone and re-escalated F10.*
  Falsifiable by reading WO-0071's scope_in. Even an F08-only retry-2
  is **at the boundary** of the WO scope: scope_in #19 explicitly
  cites the "two coordinated features bundled" rationale, AC #23
  requires both feature ids to be flippable to `passes=true`, and the
  cargo-test selectors in scope_in #20 + acceptance_criteria #11 cover
  both g3 (graphiti) and g4 (codegraphcontext) suites. A unilateral
  F08-only retry-2 by the executor would itself be a (more subtle)
  scope-creep. The correct cut is at the planner layer, not the
  executor's interpretation of the WO.
* **H3 (1%)** — *The dispatch instruction was correct; the WO-0071
  scope_in was already wide enough; the prior RCA was wrong about
  `Who: planner`.* Falsifiable by the WO-0071 JSON: line 121 sets
  `forbidden_paths: ["ucil-build/decisions/**", …]` and lines 13-44
  enumerate `scope_in` items, none of which mention
  `crates/ucil-daemon/src/plugin_manager.rs`. The text is dispositive:
  the WO does not authorize the work the retry-2 cycle landed.

## Remediation

**Who**: planner

**What**: Same as the prior RCA — emit two replacement work-orders.
The prior RCA at `root-cause-WO-0071.md:144-187` specifies the split
in detail; this RCA refines two operational details (commit cherry-pick
map + escalation re-open) and is otherwise an endorsement.

**Replacement WOs (verbatim from prior RCA, with cherry-pick refinements)**:

1. **WO-0072 — "land-codegraphcontext-plugin-manifest" (F08-only)**.
   * Single feature: `P3-W9-F08`.
   * Verbatim copy of WO-0071's F08-applicable scope_in items per
     `root-cause-WO-0071.md:147-162`.
   * Pin: `codegraphcontext@0.4.7` via `uvx --with falkordblite
     codegraphcontext@0.4.7 mcp start`. The `--with falkordblite` is
     load-bearing (cited in escalation:33 attempt #1 evidence).
   * Representative tool name for M3 mutation: `analyze_code_relationships`
     (or `find_code` / `add_code_to_graph` — executor selects from the
     verified 25-tool advertised set).
   * **`forbidden_paths`** stays as in WO-0071 — the F08 work is
     manifest + new test file + verify script + install script + a
     small refinement to a new test file. No `plugin_manager.rs`
     edit is needed for F08; codegraphcontext respects the spec.
   * **Cherry-pick map** (executor sources from the existing feat
     branch `feat/WO-0071-graphiti-and-codegraphcontext-plugin-manifests`):
     * `1d52a3f711763d407d8d82ddc126701dd687341c` — F08 manifest
       (122 LOC; `plugins/architecture/codegraphcontext/plugin.toml`
       added). Cherry-pick clean.
     * `39850a1…` — F08 g4 test suite (new file
       `crates/ucil-daemon/tests/g4_plugin_manifests.rs`,
       single test `codegraphcontext_manifest_health_check`).
       Cherry-pick clean.
     * `b23bdfe…` — F08 verify-script replacement of TODO-stub
       (`scripts/verify/P3-W9-F08.sh`). Cherry-pick clean.
     * **`e5f10bb`** is a kitchen-sink commit (5 files / 503 LOC —
       see critic-report Warning #1 at
       `ucil-build/critic-reports/WO-0071.md:124-137`). Of those 5
       files, only **2 lines** (the `cleanup_smoke` trap + the
       `query_type`/`find_callers` rename) belong to F08 — the
       other 3 files (graphiti manifest, graphiti install script,
       g3 test extension) and the F10 verify-script body belong to
       F10. **Recommendation**: cherry-pick `e5f10bb` with `--no-commit`,
       then `git restore --staged --worktree -- crates/ucil-daemon/tests/g3_plugin_manifests.rs plugins/knowledge/graphiti/plugin.toml scripts/devtools/install-graphiti-mcp.sh scripts/verify/P3-W9-F10.sh`,
       then re-commit only the F08-applicable diff to
       `scripts/verify/P3-W9-F08.sh`. Alternative: a fresh
       `git diff main..12a705d -- scripts/verify/P3-W9-F08.sh
       crates/ucil-daemon/tests/g4_plugin_manifests.rs
       plugins/architecture/codegraphcontext/plugin.toml
       scripts/devtools/install-codegraphcontext-mcp.sh | git apply`
       on a fresh branch from main.
   * Estimated complexity: small (4-5 commits; ~half of WO-0071's
     footprint).

2. **WO-0073 — "tolerant-mcp-handshake-and-graphiti-manifest" (F10 +
   DEC-0019 + harness extension)**.
   * Single feature: `P3-W9-F10`.
   * **`scope_in` EXPLICITLY authorizes**:
     `crates/ucil-daemon/src/plugin_manager.rs` (the
     `send_tools_list` drain rewrite — Sub-option B implementation
     site at `root-cause-WO-0071.md:166-174`); writing the new
     `DEC-0019` ADR; the F10 manifest + install script + verify
     script + g3-test extension.
   * **`forbidden_paths`** REMOVES `ucil-build/decisions/**` (the
     ADR write is in-scope by construction); RETAINS
     `ucil-build/feature-list.json`, `ucil-master-plan-v2.1-final.md`,
     `tests/fixtures/**`, `scripts/gate/**`, `scripts/flip-feature.sh`,
     `ucil-build/post-mortems/**`,
     `crates/ucil-daemon/tests/plugin_manifests.rs`.
   * Pin: `graphiti-memory@0.2.0` (the WO-0069-template-incompatible
     candidate with a bogus `notifications/initialized` echo; the
     drain handles the spurious frame). M1 mutation contract on
     `transport.command` poison still applies; M3 on the canonical
     graphiti tool name (e.g., `search_memory_facts`,
     `add_memory_observation`, or whichever the live `tools/list`
     surface advertises).
   * **Cherry-pick map**:
     * `9953a0ff702b97f5b1cc15b9efc8628e1172a0e8` — `plugin_manager.rs`
       send_tools_list drain rewrite (+70/−12 LOC) AND DEC-0019 ADR
       (159 LOC). Cherry-pick clean — both files are in WO-0073's
       scope_in.
     * `e5f10bb` (split-pick): the F10-applicable portion (graphiti
       manifest + graphiti install script + g3 test extension + F10
       verify script). Same `cherry-pick --no-commit` + `git restore`
       pattern as WO-0072 but mirrored — keep the F10 files, drop
       the F08 verify-script tail.
     * The retry-2 RFR (`ucil-build/work-orders/0071-…ready-for-review.md`)
       can be re-purposed in the WO-0073 RFR with a citation to the
       cherry-pick history.
   * **ADR refinement**: the planner should review DEC-0019's
     `authored_by:` field
     (`12a705d:ucil-build/decisions/DEC-0019-…md:5` — currently reads
     `executor (WO-0071 retry 2; per RCF Sub-option B at
     ucil-build/verification-reports/root-cause-WO-0071.md:166-178)`)
     and re-author to `planner (WO-0073)` so the ADR provenance
     matches the WO-contract of the cherry-picked commit. The body
     can stay unchanged.
   * Estimated complexity: medium (5-7 commits with the
     `plugin_manager.rs` rewrite + ADR + manifest + tests + verify).

**Acceptance** (carried from prior RCA, both WOs):

* WO-0072: `cargo test -p ucil-daemon --test g4_plugin_manifests
  g4_plugin_manifests::codegraphcontext_manifest_health_check`
  passes; M2 + M3 mutation contract proven; `bash scripts/verify/P3-W9-F08.sh`
  exits 0; verifier flips `P3-W9-F08:passes` to `true`.
* WO-0073: `cargo test -p ucil-daemon --test g3_plugin_manifests
  g3_plugin_manifests::graphiti_manifest_health_check` passes;
  M1 + M3 mutation contract proven; `bash scripts/verify/P3-W9-F10.sh`
  exits 0; verifier flips `P3-W9-F10:passes` to `true`. The new
  `plugin_manager.rs::send_tools_list` retains byte-identical
  behaviour for spec-compliant plugins (codebase-memory, mem0,
  codegraphcontext, ast-grep, probe, scip) — this should be a load-
  bearing assertion in the WO-0073 acceptance set, exercised by the
  existing tests in `crates/ucil-daemon/tests/plugin_manifests.rs`
  and `tests/g3_plugin_manifests.rs` (codebase-memory + mem0 tests).

**Risk** (carried + refined):

* **Decoupling F08 from F10 breaks no dependency edge** — verified at
  `feature-list.json:P3-W9-F08:dependencies` = `["P2-W6-F01"]` and
  `feature-list.json:P3-W9-F10:dependencies` = `["P2-W6-F01"]`. The
  master plan's downstream P3-W9-F09 (G4 parallel architecture
  executor) consumes F08 only.
* **The retry-2 commits are forward-only and pushed**. Both replacement
  WOs cherry-pick from `feat/WO-0071-…` rather than re-implementing.
  Nothing is wasted.
* **The escalation file
  `ucil-build/escalations/20260508T0210Z-wo-0071-graphiti-mcp-handshake-and-infra-blocker.md`
  on the feat branch carries `resolved: true`** as the last line of
  its appended `## Resolution (2026-05-08, executor retry 2)` block.
  That resolution-note describes precisely the policy-violation path
  this rejection rejects — so the resolution is **vacated** by the
  retry-2 verifier rejection. Triage's correct move on the planner-
  emission of WO-0072 + WO-0073 is to re-author the resolution-note
  to point at the replacement WO ids (Bucket A pattern per
  `ucil-build/CLAUDE.md` "Triage" section). The note's `resolved: true`
  status is acceptable to retain only if the resolution-text accurately
  cites the replacement WOs as the unblock path; otherwise re-open.
* **3-strikes halt is one rejection away.** Both features are at
  `attempts: 2` per `feature-list.json:P3-W9-F08.attempts` and
  `feature-list.json:P3-W9-F10.attempts`. The `.claude/CLAUDE.md`
  Anti-laziness contract section + `ucil-build/CLAUDE.md` Triage
  section both halt unconditionally on `attempts >= 3`. The planner
  split MUST happen as the next outer-loop iteration; another
  executor session against WO-0071 will trip the halt.

## If the hypothesis is wrong

If H2 is correct (retry-2 over-extended; an F08-only retry would have
landed):

1. The planner could emit a **single** replacement WO-0072 with F08
   alone and `forbidden_paths` widened only to remove the bundled
   feature_id constraint. This is functionally equivalent to the
   recommended split because F10 still requires a separate WO with
   `plugin_manager.rs` in `scope_in`.
2. Even under H2, `attempts` should be reset (or the new WO should
   carry a fresh attempts counter; the verifier interprets attempts
   as feature-scoped, not WO-scoped, per `flip-feature.sh` semantics).
   This is unchanged from H1.

If H3 is correct (the prior RCA was wrong about `Who: planner`; the
retry-2 work was actually in scope):

1. The cheapest falsification is reading WO-0071 line 121
   `forbidden_paths` array: the substring
   `"ucil-build/decisions/**"` appears verbatim. The retry-2 commit
   `9953a0f` adds a 159-LOC file under `ucil-build/decisions/`. The
   diff is dispositive against H3; the verifier's classification
   `forbidden-paths-violation` is mechanical.

## Outer-loop routing

* **Open escalation**:
  `ucil-build/escalations/20260508T0210Z-wo-0071-graphiti-mcp-handshake-and-infra-blocker.md`
  is `resolved: true` on the feat branch (`12a705d`) and **not present
  on `main`**. The merge of feat→main is gated on verifier accept,
  which has not occurred. Effectively the escalation is `resolved:
  false` from `main`'s perspective. On planner emission of WO-0072 +
  WO-0073, triage may auto-resolve on `main` (Bucket A) with a one-
  line note pointing at the replacement WO ids; OR the planner can
  cherry-pick the escalation file (without the `resolved: true` tail)
  onto main and re-author the resolution-text to cite WO-0072/WO-0073
  before re-marking resolved.
* **Do NOT route this RCA back to the executor.** This is a
  re-statement of the prior RCA's directive; the dispatch step from
  RCA → next-action is the routing-defect Layer B. The next outer-loop
  iteration must be a planner session, not an executor session.
* **Do NOT spawn a verifier on this feat branch again.** The F08
  portion is meetable but blocked behind the F10 + DEC-0019 + scope-
  creep portion that the verifier has already rejected. Splitting the
  WO is the only forward path.
* **Counter status (post this rejection):** P3-W9-F08 = 2,
  P3-W9-F10 = 2. The next executor session against either feature
  must run under a *new* WO id (WO-0072 or WO-0073); attempting
  WO-0071 again increments to 3 and halts the loop unconditionally.
* **Dispatch-layer remediation (Layer B fix)**: out of scope for this
  RCA to implement (it would touch `scripts/run-phase.sh` /
  orchestration, which sits outside an RCA's read-mostly remit). A
  follow-up Bucket B harness-improvement candidate: the dispatch
  branch between rejection and retry should parse the prior RCA's
  `Who:` directive and refuse to re-dispatch the same agent type
  the RCA explicitly excluded. Suggested escalation slug:
  `dispatch-layer-rca-routing-respect-needed`.

## State preservation

I have not modified any source files, ADRs, the master plan,
`feature-list.json`, `flip-feature.sh`, or any in-repo file outside
`ucil-build/verification-reports/root-cause-WO-0071.md` (this file).
The worktree at `/home/rishidarkdevil/Desktop/ucil-wt/WO-0071` remains
at `12a705d` with a clean working tree (`git status` confirmed).
No `git stash`/`pop` experiments were applied — the diff at
`fee4896..12a705d` combined with the rejection report
(`ucil-build/rejections/WO-0071.md`), critic report
(`ucil-build/critic-reports/WO-0071.md`), executor RFR
(`ucil-build/work-orders/0071-…ready-for-review.md`), and the
escalation tail at the feat branch provided sufficient cross-
confirmation without code-mutation experiments.

## Summary table — claims, evidence, and citation

| Claim | Citation |
|---|---|
| Retry-2 rejection classification is `forbidden-paths-violation+scope-creep+rcf-routing-bypass` | `ucil-build/rejections/WO-0071.md:9` |
| WO-0071 lists `ucil-build/decisions/**` in `forbidden_paths` | `ucil-build/work-orders/0071-graphiti-and-codegraphcontext-plugin-manifests.json:114-124` (line 121) |
| Commit `9953a0f` adds 159 LOC under `ucil-build/decisions/DEC-0019-…md` (status `accepted`) and rewrites `plugin_manager.rs::send_tools_list` (+70/−12 LOC) | `git show --stat 9953a0f` (verified live); critic-report at `ucil-build/critic-reports/WO-0071.md:55-58, 77-92` |
| The prior RCA's recommendation is `Who: planner` + `Do NOT route to executor` | `ucil-build/verification-reports/root-cause-WO-0071.md:142-244` |
| The retry-2 RFR explicitly acknowledges the `forbidden_paths` and routing deviations | `ucil-build/work-orders/0071-graphiti-and-codegraphcontext-plugin-manifests-ready-for-review.md:213-216,217-220` |
| Both features are at `attempts: 2` | `jq '.features[] \| select(.id == "P3-W9-F08" or .id == "P3-W9-F10")' ucil-build/feature-list.json` |
| `attempts >= 3` halts the loop unconditionally | `.claude/CLAUDE.md` (anti-laziness contract / escalation triggers); `ucil-build/CLAUDE.md` (Triage section) |
| Retry-2 work is technically clean (anti-laziness scans pass; M1/M2/M3 exercised; coverage 89.15%) | `ucil-build/critic-reports/WO-0071.md:144-170`; `ucil-build/work-orders/0071-…ready-for-review.md` (RFR table) |
| F08 is implementable as-prescribed; F10 is unimplementable under the WO-0069 template | `ucil-build/verification-reports/root-cause-WO-0071.md:50-99` (prior section, evidence table) |
| Cherry-pick from feat is the path; nothing wasted | `git log fee4896..12a705d --oneline` shows 7 commits, all forward-only and pushed |
