# Effectiveness Report — Phase 1

Run at: 2026-04-18T22:25:29Z
Commit: e8d7c2f885b7f910993916d0f5ae707533e663d6
Evaluator: effectiveness-evaluator (fresh session, `claude-opus-4-7`)

## Summary

| metric | value |
|---|---|
| Scenarios discovered for phase 1 | 1 |
| Scenarios run | 0 |
| Scenarios skipped (tool_not_ready) | 1 |
| Scenarios PASS | 0 |
| Scenarios WIN | 0 |
| Scenarios FAIL | 0 |

**Gate verdict: PASS (vacuous, 7th consecutive)** — one scenario is
tagged for phase 1 (`nav-rust-symbol`), and it auto-skips because one
of its `requires_tools` (`find_references`) is still a Phase-1 stub at
this commit. The gate contract (see §"Gate contract") permits this as
a vacuous pass. The §"Advisory" section documents what would make the
pass *substantive* rather than vacuous.

## Progress since the previous report (`effectiveness-phase-1.md` @ `cfe3344`)

HEAD advanced `855cdfa` → `e8d7c2f` (3 commits). All three are
verification/integration **log refreshes** + an escalation resolution
— zero source delta in `crates/`, `adapters/`, `ml/`, `plugin*/`,
`scripts/`, `.claude/settings.json`, `tests/scenarios/`, or
`tests/fixtures/`:

```
git diff cfe3344..e8d7c2f -- crates/ adapters/ ml/ plugin/ plugins/ \
                             scripts/ .claude/settings.json | wc -l
  → 0
git diff cfe3344..e8d7c2f -- tests/scenarios/ tests/fixtures/ | wc -l
  → 0
```

| sha | subject | category |
|---|---|---|
| `cfe3344` | test(effectiveness): phase-1 gate refresh at 855cdfa — verdict PASS (vacuous, 6th) | prior evaluator output |
| `e17ff2f` | chore(escalation): resolve 20260419-0235-monitor-phase1-gate-still-red — Bucket A | escalation admin |
| `04d5130` | chore(verification-reports): harness gate log refresh | log refresh |
| `e8d7c2f` | test(integration): phase-1 integration pass — verdict FAIL (diagnostics-bridge) | integration log refresh |

No advisory item from the prior report has shipped in this window:

| # | Advisory (from `cfe3344` report) | Status at `e8d7c2f` |
|---|---|---|
| 1 | Attach a KG from the stdio entry point so `find_definition` becomes responsive over stdio | ✅ remains landed at `f11ebfd` (WO-0041) — re-confirmed in Probe 3 below |
| 2 | Register UCIL under `mcpServers.ucil` in `.claude/settings.json` | ⏳ still open (Probe 4) |
| 3 | Author a phase-1-only scenario exercising only Phase-1 tools | ⏳ still open (Discovery §) |

This evaluator pass is therefore expected to mirror the prior pass.
Probes are re-run anyway to keep the report a proof, not a recall.

## Scenario discovery

Scanned `tests/scenarios/*.yaml`; retained any scenario whose
`phases:` list contains `1`:

| Scenario file | phases | fixture | requires_tools |
|---|---|---|---|
| `nav-rust-symbol.yaml` | `[1,2,3,4,5,6,7,8]` | `rust-project` | `find_definition`, `find_references` |
| `refactor-rename-python.yaml` | `[2,3,4,5,6,7,8]` | `python-project` | phase 2+, not applicable |
| `add-feature-ts.yaml` | `[3,4,5,6,7,8]` | `typescript-project` | phase 3+, not applicable |
| `arch-query.yaml` | `[3,4,5,6,7,8]` | `mixed-project` | phase 3+, not applicable |

Only `nav-rust-symbol` is eligible for Phase 1.

## Tool-availability probe (per `.claude/agents/effectiveness-evaluator.md`)

### Probe 1 — `ucil-mcp` binary

```
command -v ucil-mcp                → MISSING
test -x ./target/debug/ucil-mcp    → MISSING
test -x ./target/release/ucil-mcp  → MISSING
test -x ./target/debug/ucil-daemon → exists
```

No standalone `ucil-mcp` binary. Per WO-0040 the equivalent entry
point is `ucil-daemon mcp --stdio`, which the evaluator contract
§"Tool-availability checks" explicitly permits.

### Probe 2 — stdio handshake via `ucil-daemon mcp --stdio --repo <fixture>`

Spawned from CWD `tests/fixtures/rust-project`, `--repo .`.
Server stderr (single startup log line):

```
INFO ucil_daemon: ucil-daemon mcp --stdio bootstrap complete
repo=. discovered=7 ingested=7
```

Ingest count matches the fixture's 7 .rs files (6 under `src/`, 1
under `tests/`). KG populated pre-request.

### Probe 3 — `tools/call` responsiveness

Transcript at `/tmp/ucil-eval-probes-e8d7c2f/probe-out.jsonl`.

```
id=0 → initialize
id=1 → tools/list
id=2 → tools/call find_definition {"name":"simplify","reason":"probe"}
id=3 → tools/call find_references  {"name":"simplify","reason":"probe","repo":"."}
```

Response summaries:

| id | tool | `_meta.not_yet_implemented` | notable fields | verdict |
|---|---|---|---|---|
| 1 | `tools/list`        | n/a | 22 tools registered (canonical set) | operational |
| 2 | `find_definition`   | **absent** | `source: tree-sitter+kg`, `file_path: ./src/transform.rs`, `start_line: 78`, `signature: pub fn simplify(expr: &Expr) -> Expr`, `qualified_name: ./src/transform.rs::simplify@78:5`, `doc_comment` populated, `found: true` | **operational** |
| 3 | `find_references`   | **true**   | `content[0].text: "tool \`find_references\` is registered but its handler is not yet implemented (Phase 1 stub)"` | **STUB — not ready** |

State of both tools is **bit-identical to the `855cdfa` / `cfe3344`
probes**. As expected from the empty source-delta above.

Canonical 22-tool roster returned by `tools/list`:

```
understand_code, find_definition, find_references, search_code,
find_similar, get_context_for_edit, get_conventions, get_architecture,
trace_dependencies, blast_radius, explain_history, remember,
review_changes, check_quality, run_tests, security_scan, lint_code,
type_check, refactor, generate_docs, query_database, check_runtime
```

### Probe 4 — host-level MCP registration

```
jq '.mcpServers | keys' .claude/settings.json
  → ["context7", "filesystem", "github", "memory",
     "sequential-thinking", "serena"]

jq '.mcpServers.ucil // "ABSENT"' .claude/settings.json
  → "ABSENT"
```

No `ucil` entry under `mcpServers` in `.claude/settings.json`.
Advisory item #2 still open. This evaluator probes the binary
directly, so the missing host registration does not block this gate;
it would block a child-`claude -p`-driven scenario (see Advisory §).

### Probe 5 — in-process feature status

Per `ucil-build/feature-list.json` at `e8d7c2f`:

| Tool | Feature ID | phase | passes | last_verified_by |
|---|---|---|---|---|
| `find_definition` | `P1-W4-F05` | 1 | ✅ true | `verifier-9422e28c-64e9-4bc0-a26d-cea7533de34b` |
| `find_references` | `P2-W7-F05` | 2 | ❌ false | null (Phase 2 feature) |

`find_references` is a Phase-2 feature; no implementation body in
the KG-routed allow-list. Even with a KG attached, a call falls
through to the stub (Probe 3 id=3 confirms).

### Conclusion

- `find_definition` — **operational over stdio** when the server is
  spawned with `--repo <PATH>`. Unchanged since `f11ebfd`.
- `find_references` — **stub**. Required by this scenario. Blocks
  the run.

The scenario's task (*"list every place it is CALLED FROM (file:line
each)"*) cannot be answered without a working `find_references` — the
"every place it is CALLED FROM" bullet is `find_references`'s core
contract. Per evaluator contract §"Tool-availability checks" —
*"If any is missing, `skipped_tool_not_ready`"* — this scenario is
`skipped_tool_not_ready`.

## Per-scenario verdict

### `nav-rust-symbol`

- **Status: `skipped_tool_not_ready`**
- **Reason:** `find_references` (`P2-W7-F05`) still returns
  `_meta.not_yet_implemented: true`; no real handler. `find_definition`
  is operational post-WO-0041 but by itself cannot answer the caller
  list.
- **Fixture sanity (side-info, not gate-relevant):**
  `grep -riE "retry|backoff|exponential|http" tests/fixtures/rust-project --include='*.rs'`
  returns **0 matches**. The fixture is a small arithmetic-expression
  evaluator (7 .rs files: `parser.rs`, `transform.rs`, `util.rs`,
  `eval_ctx.rs`, `main.rs`, `lib.rs`, `tests/integration_test.rs`)
  with no HTTP machinery. The scenario's canonical answer at this
  fixture is therefore "no such function exists" — which raises the
  bar on `find_references`'s precision: the agent must prove absence
  across caller graphs, not just hallucinate a plausible list.
- **Required work to unblock at Phase 1:**
  - Either pull `P2-W7-F05` (`find_references`) forward with an ADR
    + planner approval per `CLAUDE.md` rules — likely premature given
    W5/W6 scope — **or** author a new phase-1-only scenario whose
    `requires_tools` is a subset of `{find_definition, search_code,
    get_conventions, understand_code}` (all KG-routable at `e8d7c2f`).
  - Register UCIL under `mcpServers.ucil` in `.claude/settings.json`
    if/when a scenario is added that drives UCIL from inside a spawned
    `claude -p` child (this evaluator's probe shortcut works without
    that, but a child-session-driven scenario would need it).
- **Fixture:** `tests/fixtures/rust-project/` — present; **not
  copied** to tempdirs because no run was attempted.
  `/tmp/ucil-eval-<scenario>` not created (confirmed absent at start
  and end of run via `find /tmp -maxdepth 1 -name 'ucil-eval-*'` →
  only probe dir `/tmp/ucil-eval-probes-e8d7c2f/` present, which
  holds this report's probe artifacts, not scenario run state).
- **Acceptance checks:** not run (no UCIL output to check; running
  baseline alone would violate contract §"Hard rules" — *"If you omit
  the baseline, fail the run as baseline-missing"* — and the
  companion UCIL run is unrunnable).
- **Judge session:** not spawned (no outputs to judge).

## Gate contract

Per `scripts/verify/effectiveness-gate.sh` and the evaluator contract
in `.claude/agents/effectiveness-evaluator.md`:

> Exits 0 iff:
>   - At least one scenario tagged for this phase exists
>   - Every non-skipped scenario returns a PASS or WIN verdict

Applied here:

- 1 scenario tagged for phase 1 ✅
- 1 scenario skipped (`skipped_tool_not_ready`) — permissible per
  contract
- 0 non-skipped scenarios → "every non-skipped PASS/WIN" vacuously
  true ✅

**Gate verdict: PASS.**

## Advisory (non-gating)

This is the **seventh consecutive vacuous PASS**
(`316109e` → `8d8fc0c` → `5edc200` → `97932e0` → `f11ebfd` →
`855cdfa` → `e8d7c2f`). The HEAD movement in this window
(`855cdfa` → `e8d7c2f`, 3 commits) is log-refresh-only plus one
benign Bucket-A escalation resolution — no source change touched the
MCP tool surface, no scenario was added, no host registration
changed. Advisory items #2 and #3 remain open with no progress.

Residual path to a **substantive** phase-1 effectiveness pass
(unchanged from prior reports):

1. **Add a phase-1-only scenario** (#3 above). A scenario shaped like
   "given a symbol name, emit the fully-qualified definition file:line
   + the signature + a conventions summary + a structured search for
   sibling usages" would let UCIL answer with `find_definition` +
   `search_code` (+ optionally `get_conventions` / `understand_code`
   if they end up Phase-1 KG-routable) and let the baseline answer
   with `grep + Read`. That produces a real UCIL-vs-baseline score
   delta rather than a skip. The existing `nav-rust-symbol` stays
   phase-2+ because the scenario's task explicitly requires caller
   enumeration.

2. **Register UCIL in host settings** (#2 above). Only strictly
   required for scenarios that drive UCIL from inside a spawned
   child `claude -p` session; this evaluator's own probe uses the
   binary directly and works today.

The evaluator does not block the gate on items 1 or 2 — they are
carried as planner input. Recommend planner pick item 1 up before
phase-1 ships, so the eventual phase-1 ship has at least one
substantive effectiveness datapoint instead of seven vacuous passes.
The scenario-authoring task is short (≈40 lines of new YAML + one
acceptance-check script) and is not blocked by any open escalation.

## Environment notes (for reproducibility)

- Repo root: `/home/rishidarkdevil/Desktop/ucil`
- Branch: `main`
- HEAD: `e8d7c2f885b7f910993916d0f5ae707533e663d6`
- Evaluator binary spawn: `./target/debug/ucil-daemon mcp --stdio
  --repo .` from CWD `tests/fixtures/rust-project` (no rebuild
  forced; binary inherits from the WO-0041 build at `f11ebfd`, which
  remains on disk and unchanged).
- Probe artifacts (transient):
  - `/tmp/ucil-eval-probes-e8d7c2f/probe-msgs.jsonl` — 4-message
    input JSONL (initialize + tools/list + find_definition +
    find_references).
  - `/tmp/ucil-eval-probes-e8d7c2f/probe-out.jsonl` — transcript.
  - `/tmp/ucil-eval-probes-e8d7c2f/probe-err.log` — server-side
    tracing (single `bootstrap complete` line).
- `/tmp/ucil-eval-<scenario-id>` tempdirs were **not** created
  (no scenario was runnable).
- No judge sessions spawned.
- No fixture files modified (contract §"Hard rules").
- No source files modified (contract §"Hard rules").
- No scenario files modified (contract §"Hard rules").

## Exit code

`0` — gate passes per contract.
