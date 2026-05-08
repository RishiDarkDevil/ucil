# Root Cause Analysis: WO-0071 (graphiti + codegraphcontext plugin manifests)

**Analyst session**: rca-2026-05-08-wo-0071
**Work-order**: WO-0071
**Features**: P3-W9-F08 (codegraphcontext), P3-W9-F10 (graphiti)
**Attempts before RCA**: 1 (each feature)
**Branch**: `feat/WO-0071-graphiti-and-codegraphcontext-plugin-manifests` @ `d795bc0`
**Merge base**: `fee489608088bd3dae7002b26c149b2e38d6d396`

## Failure pattern

The verifier rejected with classification `blocked-by-executor-escalation`. The
feat branch carries a **single commit** that adds **only** an executor-written
escalation file:

```
$ git log main..HEAD --oneline
d795bc0 chore(escalation): WO-0071 — graphiti MCP server has no canonical pypi pin

$ git diff main...HEAD --stat
ucil-build/escalations/20260508T0210Z-wo-0071-graphiti-mcp-handshake-and-infra-blocker.md | 82 ++++++++++++++++++++++
```

No source code, no manifests, no install scripts, no integration tests, no
verify-script implementations landed. The executor halted under the
`.claude/agents/executor.md` escalation contract after six documented attempts
to land F10 (graphiti) failed to satisfy the WO-0069 plugin-manifest
health-check template's three preconditions:

1. **Boots without external infrastructure** (FalkorDB / Neo4j / live API key
   during `tools/list`).
2. **Respects the MCP `notifications/initialized` no-response rule**.
3. **Is published to pypi/npm under a canonical name with a console script**
   (so it can be launched via `npx -y <pkg>@<ver>` or `uvx <pkg>@<ver>`).

This is not a stub/skip/laziness violation. The critic at
`ucil-build/critic-reports/WO-0071.md:1` classified the diff as `BLOCKED` (not
`REJECTED`) for the same reason; the verifier's REJECT routes the work back to
the planner per the executor's recovery-options proposal.

## Root cause (hypothesis, 95% confidence)

**The WO-0071 work-order bundles two features whose upstream landscapes are
asymmetric: F08 codegraphcontext is implementable as-prescribed, but F10
graphiti has no pypi-published canonical MCP server that satisfies the WO-0069
template's three preconditions simultaneously.** The bundling assumption is a
planner-template defect, not an executor capability defect.

### Evidence — F08 is implementable

The executor's research session verified end-to-end:
`uvx --with falkordblite codegraphcontext@0.4.7 mcp start` responds cleanly to
the full `initialize` → `notifications/initialized` → `tools/list` handshake
and advertises **25 tools**, including the master-plan §4.4 line 326
"blast-radius analysis" representative `analyze_code_relationships`. Cited at
`ucil-build/escalations/20260508T0210Z-wo-0071-graphiti-mcp-handshake-and-infra-blocker.md:17`
and corroborated by the critic at `ucil-build/critic-reports/WO-0071.md:54-60`.

### Evidence — F10 is unimplementable under WO-0069 template

The four pypi candidates and one upstream-from-source target each fail at
least one of the three preconditions:

| Package | Latest | Failing precondition |
|---|---|---|
| `graphiti-core@0.29.0` (upstream Zep) | 0.29.0 | (3) ships no console script — has no `mcp_server` entry point |
| `graphiti-mcp-varming@1.0.6` | 1.0.6 | (1) requires live FalkorDB on `localhost:6379` during `initialize`; CLI rejects `kuzu` (only `neo4j`/`falkordb`) |
| `graphiti-memory@0.2.0` | 0.2.0 | (2) emits unsolicited JSON-RPC error envelope for `notifications/initialized` (id=1, code=-32601 "Method not found") |
| `iflow-mcp-graphiti-core@0.24.1` | 0.24.1 | (3) `ModuleNotFoundError: No module named 'config'` — broken package |
| `getzep/graphiti` from-source | n/a | (3) not pypi-published; runs via `uv run main.py` from a checkout — `uvx --from git+…` is forbidden by WO scope_in #1 |

### Evidence — the protocol code matches the executor's claim

The closest viable F10 candidate (`graphiti-memory@0.2.0`) breaks the
single-line-read assumption baked into the production helpers. Verified live
in the worktree:

* `crates/ucil-daemon/src/plugin_manager.rs:1143-1175`
  (`run_protocol_prefix`) — sends `initialize`, reads **exactly one** line as
  the response, sends `notifications/initialized`, **does not read** any
  follow-up frame (the spec says notifications must not be replied to).
* `crates/ucil-daemon/src/plugin_manager.rs:1185-1212` (`send_tools_list`) —
  sends `tools/list`, reads **exactly one** line, calls
  `parse_tools_list_response(&tools_response)`.
* `crates/ucil-daemon/src/plugin_manager.rs:1652-1685`
  (`parse_tools_list_response`) — if the parsed JSON has an `error` field it
  returns `PluginError::ProtocolError("plugin returned JSON-RPC error: …")`.

Wire trace against `graphiti-memory@0.2.0`:
1. Helper writes `initialize` → server writes initialize response (line A).
2. Helper writes `notifications/initialized` → server writes BOGUS error frame
   (line B) — non-spec.
3. Helper writes `tools/list` → server writes tools/list response (line C).
4. Helper reads line A → OK.
5. Helper reads next line **B** (bogus error frame) when expecting C.
6. `parse_tools_list_response` sees `error` field → returns `ProtocolError`.

The executor's diagnosis is mechanically correct.

### Evidence — the spec does not require pypi packaging

`ucil-master-plan-v2.1-final.md:313` describes Graphiti as "Python SDK + MCP
server" (P1 priority), and §14.1:1676 lists `plugins/knowledge/graphiti/` in
the plugins directory layout. **Neither location prescribes pypi/npm
packaging** — the WO-0069 template's "pinned npm/pypi tag" requirement is a
planner-side template decision derived from the codebase-memory + mem0 prior
art, not a master-plan invariant. The spec is silent on packaging mode, so
deferring F10 to a follow-up WO with a different packaging strategy (Option C
docker-compose, Option E wait-for-upstream, or a sanctioned `uvx --from
git+…@<sha>` ADR exemption) does not contradict the master plan.

### Evidence — the bundling rationale was symmetry, not necessity

WO-0071 scope_in #19 cites: *"Two coordinated features bundled per the
WO-0067/0068/0069/0070 precedent — both are external plugin manifests with
shared scope_in shape, both depend on P2-W6-F01 alone."* The bundling is a
soft template-coherence convenience; both features sit at independent
`dependencies: ["P2-W6-F01"]` in `feature-list.json:P3-W9-F08:dependencies`
and `feature-list.json:P3-W9-F10:dependencies`. **Splitting the bundle does
not break any dependency edge in the feature graph.**

## Hypothesis tree (alternatives, ranked)

* **H1 (95%)** — *Asymmetric upstream-readiness; planner bundling defect.*
  Already explained above. Highest evidence; falsifiable only by finding a
  fifth pypi package the executor didn't try (search-space is small; the
  executor enumerated five and the upstream getzep/graphiti). Cheap to
  re-falsify: `pip search graphiti-mcp` / `pypi.org/search/?q=graphiti-mcp`.
* **H2 (3%)** — *Executor missed a configuration flag (`--no-init-response`
  or equivalent) that would coax `graphiti-memory@0.2.0` into spec
  compliance.* Falsifiable by reading the
  `github.com/alankyshum/graphiti-memory` source for `notifications/initialized`
  handling. The executor enumerated the package's behaviour by direct
  observation; the protocol code at
  `crates/ucil-daemon/src/plugin_manager.rs:1143-1175` confirms the helper
  cannot consume an extra frame regardless of upstream config.
* **H3 (2%)** — *Wait for upstream Zep to publish a canonical pypi MCP
  server.* Not actionable on the WO loop's timescale; this is Option E.

## Remediation

**Who**: planner

**What**: Re-scope WO-0071 by splitting it. Concretely, emit two replacement
work-orders:

1. **WO-0072 (or next available id) — "land-codegraphcontext-plugin-manifest"**
   * Single feature: `P3-W9-F08` only.
   * Verbatim copy of WO-0071's F08 portion: scope_in #1, #3, #4, #6, #8,
     #10, #13, #14 (M2 mutation only), #15, #16, #17, #18, #20, #21, #22,
     #24, #25, #26, #27, #28, #30, #31 (codegraphcontext-only md5 snapshots).
   * Drop F10-specific scope_in items (#2, #5, #7, #11, #12 M1-mutation,
     #14 M3 dual-target carve-out collapses to F08-only, #22 API-key gate,
     #23 eventual-consistency disjunction).
   * Pin: `codegraphcontext@0.4.7` via `uvx --with falkordblite
     codegraphcontext@0.4.7 mcp start`. The `--with falkordblite` is
     load-bearing — bare `uvx codegraphcontext@0.4.7` fails because
     FalkorDB-Lite is not pre-bundled (see escalation:33 attempt #1).
   * The expected representative tool name for the M3 mutation contract is
     `analyze_code_relationships` (or `find_code` / `add_code_to_graph` —
     executor selects from the verified 25-tool advertised set).
   * Estimated complexity: small (~half of WO-0071's footprint), 4-5 commits.
2. **WO-NNNN (deferred — landed as a tracked follow-up after the planner
   chooses one of three sub-options)** for `P3-W9-F10` graphiti:
   * **Sub-option B** (recommended): land an ADR + harness extension that
     adds a tolerant `health_check_with_timeout` variant which loops
     `read_line` until it sees an `id == 2` frame (or until a budget
     elapses), tolerating spurious notification-error frames. Implementation
     site: `crates/ucil-daemon/src/plugin_manager.rs:1185` (replace
     `send_tools_list`'s single `read_line` with a bounded
     drain-until-id-match loop). Estimated 80-120 LOC + new ADR
     (`DEC-NNNN-tolerant-mcp-handshake`) + a unit test against the existing
     in-tree mock-mcp test plugin extended with a noisy-notification mode.
     Then land F10 against `graphiti-memory@0.2.0`.
   * **Sub-option C**: land an ADR + `plugins/knowledge/graphiti/docker-compose.yaml`
     (FalkorDB sidecar, parallel to the Serena docker-fixture pattern).
     Significantly larger scope (~200-300 LOC); requires CI docker-in-docker
     access. Pin: `graphiti-mcp-varming@1.0.6` against the spun-up FalkorDB.
   * **Sub-option E** (defer): mark `P3-W9-F10` as `blocked_reason = "awaiting
     upstream pypi publication of canonical Graphiti MCP server"` and revisit
     when `getzep/graphiti` ships `graphiti-mcp@<version>` to pypi with a
     console script.

   The planner picks one of B/C/E and emits the appropriate WO; B is the
   recommended path because the protocol-tolerance change is reusable for
   future plugins that emit out-of-spec frames.

**Acceptance** (for the F08-only replacement WO):
* All scope_in items relevant to F08 met (manifests, install script, new test
  file `tests/g4_plugin_manifests.rs`, verify script `scripts/verify/P3-W9-F08.sh`).
* `cargo test -p ucil-daemon --test g4_plugin_manifests
  g4_plugin_manifests::codegraphcontext_manifest_health_check` passes.
* M2 + M3 mutation contract proven (M1 collapses out — no graphiti manifest
  to mutate in this WO).
* `bash scripts/verify/P3-W9-F08.sh` exits 0.
* Verifier flips `P3-W9-F08:passes` to `true` (P3-W9-F10 stays `false` with
  `blocked_reason` populated).

**Risk**:
* **Decoupling F08 from F10 in the plugin-manifest landing series breaks no
  dependency edge** — verified by reading
  `feature-list.json:P3-W9-F08:dependencies` (`["P2-W6-F01"]`) and
  `feature-list.json:P3-W9-F10:dependencies` (`["P2-W6-F01"]`). Future
  features that consume F08 (the master plan's downstream P3-W9-F09 G4
  parallel architecture executor) do not need F10. So the split is graph-safe.
* **The harness-extension Option B requires an ADR** because it weakens MCP
  spec strictness (`crates/ucil-daemon/src/plugin_manager.rs`'s current
  approach assumes spec-compliant servers). The ADR should document which
  upstream MCP servers in the wild emit out-of-spec notification responses
  (graphiti-memory; possibly others — the planner should survey the next 4-6
  G3/G4 plugins before locking the harness-tolerance behaviour). Suggested
  ADR slug: `DEC-NNNN-tolerant-mcp-handshake-for-out-of-spec-plugins`.
* **The docker-compose Option C requires verifier-host docker-in-docker
  capacity**. The harness already brings up `docker/serena-compose.yaml`
  (per master-plan §11), so adding a graphiti-bound FalkorDB compose is
  precedented but adds ~30-60s to the integration-test budget on cold cache.

## If hypothesis is wrong

If H2 is true (executor missed a configuration flag), the cheapest
falsification is:

1. Read `github.com/alankyshum/graphiti-memory@v0.2.0`'s `mcp_server.py`
   handler for `notifications/initialized`. Search for any
   `--strict-notifications` or `--disable-notification-responses` flag.
2. If found, pass it via the manifest's `transport.args` and re-test.
3. If absent, H1 stands and the planner-split remediation is correct.

If H3 is the resolution path (wait for upstream), the planner should:
1. File `P3-W9-F10` as a `blocked_reason="awaiting upstream pypi publication"`
   feature in `feature-list.json` (verifier-only field via `flip-feature.sh`).
2. Track the upstream `getzep/graphiti` PR for `mcp_server` pypi publication.
3. Revisit when the upstream ships a canonical pypi package.

## Outer-loop routing

* **Open escalation**:
  `ucil-build/escalations/20260508T0210Z-wo-0071-graphiti-mcp-handshake-and-infra-blocker.md`
  remains `resolved: false` until the planner emits the replacement WO. On
  emission, triage may auto-resolve with a `## Resolution` note pointing at
  the replacement WO id (Bucket A pattern).
* **Do NOT route this RCA back to the executor** — the executor cannot
  unblock F10 with the WO-0071 scope as currently written; the next action
  belongs to the planner.
* **Do NOT spawn a verifier on this branch as-is** — the F10 portion is
  by-construction unmeetable.
* **Increment counters only**: `attempts` for both features now stand at
  `1` per the verifier's flip; the next replacement WO starts a fresh
  attempt counter for F08 alone.

## State preservation

I have not modified any source files, ADRs, the master plan,
`feature-list.json`, or `flip-feature.sh`. The worktree at
`/home/rishidarkdevil/Desktop/ucil-wt/WO-0071` remains at `d795bc0` with the
single escalation commit as the verifier observed it. No `git stash`/`pop`
experiments were applied during this RCA — the escalation file's evidence
combined with the production protocol code at
`crates/ucil-daemon/src/plugin_manager.rs:1143-1175,1185-1212,1652-1685`
provided sufficient cross-confirmation without a stash-and-time experiment.
