---
work_order: WO-0049
feature: P2-W7-F05
branch: feat/WO-0049-find-references-and-g1-source-production-wiring
ref: f197ad1389c0e2ef67b67e5364b45fc25970bcfd
attempts_before_rca: 1
rca_session: rca-WO-0049-2026-05-05
remediation_owner: executor
---

# Root Cause Analysis: WO-0049 (find_references MCP tool + 4 G1Source production-shape impls — `P2-W7-F05`)

**Analyst session**: `rca-WO-0049-2026-05-05`
**Feature**: `P2-W7-F05`
**Branch**: `feat/WO-0049-find-references-and-g1-source-production-wiring`
**Branch HEAD**: `f197ad1389c0e2ef67b67e5364b45fc25970bcfd`
**Attempts before RCA**: 1 (one verifier rejection at `ucil-build/rejections/WO-0049.md`, preceded by a critic `BLOCKED` verdict at `ucil-build/critic-reports/WO-0049.md`)

## Failure pattern

**The work-order was not executed.** The branch contains a single refactor-only commit (`f197ad1`) closing scope_in steps 7 (per-source `tracing::instrument` on `run_g1_source`) and 8 (`.expect(...)` cleanup at `executor.rs:1057`). The headline body of work — scope_in steps 1–6 + 9–12 — never landed:

- `crates/ucil-daemon/src/g1_sources.rs` (NEW file with 4 `G1Source` impls + `G1SourceFactory`) does not exist on the branch (`git ls-tree feat/WO-0049-… -- crates/ucil-daemon/src/g1_sources.rs` empty).
- `McpServer.g1_sources` field, `with_g1_sources` builder, `find_references` route, and `handle_find_references` method on `crates/ucil-daemon/src/server.rs` are absent (`git diff --name-only main...HEAD` shows only `crates/ucil-daemon/src/executor.rs`).
- The 3 frozen module-root acceptance tests (`test_find_references_tool`, `test_find_references_tool_unknown_symbol`, `test_find_references_tool_missing_name_param`) are missing — `grep -nE '^pub async fn test_find_references_tool' crates/ucil-daemon/src/server.rs` returns empty.
- `crates/ucil-daemon/src/lib.rs` re-exports for the 5 new public symbols are missing.
- `scripts/verify/P2-W7-F05.sh` does not exist.
- `ucil-build/work-orders/0049-ready-for-review.md` does not exist — the executor never declared the WO complete.

The **shape of the failure is "executor session ended early"**, not "tests fail" or "compile fails". Cargo silently ran the absent test selector and returned 0 (`0 passed; 0 failed; 1 filtered out`) — exactly the masked-failure pattern the verifier's reality-check exists to catch (`ucil-build/rejections/WO-0049.md:43`).

## Root-cause hypothesis tree

### H1 (90% confidence, primary) — Executor session terminated after the easy refactors, never tackled the body of work

The two refactors that landed (steps 7 and 8 of scope_in) are the **smallest** items in the WO: a single attribute macro on a private helper plus an 11-line replacement of `.map(|r| r.expect(...))` with `flatten() + debug_assert_eq!`. Both touch the same private function neighborhood in `executor.rs` (lines 985–1080), needed no new types, no new modules, no new dependencies. They are exactly the "warm-up" edits an executor would land first.

The remaining 14 scope_in items (~740 LOC across 4 files including a brand-new module) form the actual feature. The executor stopped between the warm-up and the feature.

**Evidence**:
- Single commit (`f197ad1`) is +25 / −4 lines (`ucil-build/critic-reports/WO-0049.md:30-31`).
- Working tree is clean (`git status --porcelain` empty in `/home/rishidarkdevil/Desktop/ucil-wt/WO-0049`) — orderly exit, NOT a crash mid-edit.
- Branch is up-to-date with `origin` (`git rev-parse HEAD == git rev-parse @{u} == f197ad1`) — the executor pushed and ended cleanly.
- No `0049-ready-for-review.md` marker — the executor never claimed the WO was done.
- The `f197ad1` commit message lists `Feature: P2-W7-F05`, `Work-order: WO-0049`, `Phase: 2` trailers correctly, and the body cites WO-0047 lessons line 296 + line 308 (the deferral-closure rationale for steps 7 and 8). The executor *understood* the WO; it just stopped after 2 of 16 steps.
- No second executor session has resumed since 2026-05-05T01:03:38Z (the verifier rejection time). The autonomous loop has been writing periodic `monitor-bypass-p2-N-features-remaining` advisories (`ucil-build/escalations/20260505T0125Z…` through `…T0520Z…`) but has not re-spawned an executor for this branch.

**Falsification test (cheap)**: re-spawn the executor on `feat/WO-0049-…-wiring`, confirm that no environmental obstacle (compile error from existing main, KG API surprise, dep missing) prevents progress on scope_in step 1. If the executor lands `crates/ucil-daemon/src/g1_sources.rs` with a `KgG1Source` shell that compiles, H1 is confirmed and the remediation is just "keep going". If the executor immediately blocks on a missing prerequisite, escalate to H2.

### H2 (8% confidence) — Spec ambiguity in scope_in step 1(a) (`KnowledgeGraph::find_references_by_name` not implemented)

scope_in step 1(a) directs the executor to:

> "use `KnowledgeGraph::find_references_by_name` if present, else open a `Connection` and SELECT from the `references` table joined to `symbols` to project rows with `(file_path, start_line, end_line, usage_type)`."

Verified by inspecting `crates/ucil-core/src/knowledge_graph.rs`:

- `KnowledgeGraph::find_references_by_name` does **not** exist. The public KG surface around references is `list_relations_by_source` (line 1046) and `list_relations_by_target` (line 1086) — generic relation queries keyed by entity rowid, not name.
- There is **no** `references` table. The schema has a single `relations` table; references are encoded as rows where `relations.kind` is one of `'references'`, `'calls'`, etc. (the test fixture at `knowledge_graph.rs:2444` seeds a `make_relation(caller_b, "references")` row to demonstrate this).

scope_in step 1(a) frames this as "if present, else fallback" — the spec is not actually wrong, but the fallback path it describes ("SELECT from the `references` table") names a non-existent table. The executor would have to read between the lines: the real fallback is **"resolve_symbol(name) → entity_id → list_relations_by_target(entity_id) → filter by relation.kind"**, mirroring the existing `find_definition` handler's caller-list logic (`server.rs:601-627` calls `read_find_definition` which uses `list_relations_by_target` filtered by `kind = 'calls'`).

This ambiguity is **not** load-bearing for H1: the executor stopped before reaching the KG-query inner loop. But if H1 is wrong (the executor did try and got stuck), this is the most likely sticking point. Treat it as a documentation patch the executor must make in the `KgG1Source` rustdoc rather than an ADR-level spec defect.

### H3 (2% confidence) — Compile-failure cascade from the WO-0048 fusion API not matching `g1_sources.rs` expectations

WO-0048 froze `fuse_g1`'s signature as `pub fn fuse_g1(outcome: &G1Outcome) -> G1FusedOutcome`. scope_in step 1(a)–(b) for `g1_sources.rs` describes emitting `G1FusionEntry { location: G1FusedLocation { … }, fields: { … } }` records — but `G1FusionEntry` is a separate type from `G1FusedEntry` (verified by grep at `executor.rs:1217` and `executor.rs:1257`). The executor would need to construct `G1FusionEntry` (the pre-fusion entry shape) for the `payload` field of `G1ToolOutput`.

This is just normal API consumption, not a defect. Disconfirmed as a cause of the early stop because the executor never started on `g1_sources.rs`.

## Root cause (best guess, 90% confidence)

**The executor's session ended after landing scope_in steps 7 and 8 — the smallest, safest refactors at the bottom of the scope_in list — without continuing on to the headline body of work.** The branch state (clean tree, pushed, no ready-for-review marker, single refactor commit, correct conventional-commit trailers) is consistent with a turn-budget exhaustion or an external session-termination event, not a crash, compile failure, or test failure. The autonomous loop has not re-spawned an executor on this branch since the verifier rejection.

The two refactors that DID land (`f197ad1`) are **correct** and form proper foundations — they should NOT be reverted (echoed by the critic at `ucil-build/critic-reports/WO-0049.md:134` and the verifier at `ucil-build/rejections/WO-0049.md:135`).

## Remediation

**Who**: executor
**What**: resume on `feat/WO-0049-find-references-and-g1-source-production-wiring` and land scope_in steps 1–6 + 9–12 in fresh commits. Do not revert `f197ad1`.
**Acceptance**: same as the work-order — AC03 (`cargo test -p ucil-daemon server::test_find_references_tool` exits 0) plus AC04–AC11, AC25, AC27, AC34 plus all regressions AC12–AC22 unbroken.

**Concrete commit plan** (mirrors the critic's recommendation block at `ucil-build/critic-reports/WO-0049.md:127-132`, expanded with file:line landing-spots and one new "watch out for" line per step):

1. **Commit 2 of ≥6 — Add `crates/ucil-daemon/src/g1_sources.rs` skeleton + `G1SourceFactory` + `KgG1Source`** (~120 LOC).
   - Lands the new file with `mod`-level `//!` doc citing master-plan §5.1 lines 416-446.
   - `pub struct G1SourceFactory` holds `Arc<Mutex<KnowledgeGraph>>` + `Arc<dyn SerenaHoverClient + Send + Sync>` and exposes `pub fn new(kg, hover_client) -> Self` + `pub fn build(&self) -> Vec<Box<dyn G1Source + Send + Sync + 'static>>`.
   - `pub struct KgG1Source { kg: Arc<Mutex<KnowledgeGraph>> }`. `impl crate::executor::G1Source` (UCIL-owned trait per DEC-0008 §4 — local impls are NOT critical-dep mocks). `kind() -> G1ToolKind::TreeSitter`.
   - `execute(&query) -> G1ToolOutput` body: lock KG (return `Errored` on poison — no `.unwrap()`), call `kg.resolve_symbol(&query.symbol)?` to get the entity_id, then `kg.list_relations_by_target(entity_id)?` to enumerate inbound relations. Filter rows where `relation.kind ∈ {"calls", "references", "imports", "type-of"}`. For each, look up `kg.get_entity_by_id(relation.source_id)?` to project `(file_path, start_line, end_line)` plus map the relation kind to a `usage_type` string: `"calls" → "call"`, `"references" → "call"` (default bucket), `"imports" → "import"`, `"type-of" → "type-annotation"`. Emit a `G1FusionEntry` per row with `fields = { "usage_type": …, "signature": …, "source": "tree-sitter+kg" }`. Pack as `serde_json::to_value(Vec<G1FusionEntry>)` into `G1ToolOutput::payload`, set `status = G1ToolStatus::Available`.
   - **Watch out for**: scope_in step 1(a) says "use `KnowledgeGraph::find_references_by_name` if present, else SELECT from the `references` table". **Both names are wrong** — H2 above documents the real KG surface. Use `resolve_symbol` + `list_relations_by_target` per the `find_definition` handler precedent at `server.rs:619` → `read_find_definition` → `list_relations_by_target`. Cite this divergence in a rustdoc comment on `KgG1Source::execute` — do **not** silently change the spec, but DO make it work against the real API. (If the divergence feels like an ADR-required spec defect, STOP and write `ucil-build/decisions/proposed-DEC-XXXX-find-references-by-name-naming.md`. The reading here is that scope_in step 1(a) frames the API name as conditional ("if present, else …") so the conditional fallback IS in-spec — write the rustdoc and proceed.)

2. **Commit 3 — `SerenaG1Source`** (~45 LOC).
   - `pub struct SerenaG1Source { hover_client: Arc<dyn SerenaHoverClient + Send + Sync> }`. `impl G1Source`. `kind() -> G1ToolKind::Serena`.
   - `execute(&query)` calls `self.hover_client.hover(&query.symbol, &query.file_path, query.line, query.column).await` (the existing trait at `executor.rs:645` — verified). On `Ok(HoverDoc { signature, doc, source })`, emit ONE `G1FusionEntry` keyed at `(query.file_path, query.line, query.line)` with `fields = { "usage_type": "definition", "signature": …, "hover_doc": …, "source": "serena" }`. On `Err(HoverFetchError::*)`, return `G1ToolOutput { kind: Serena, status: Errored | TimedOut, error: Some(…) }`. The `definition` bucket is documented as a 4th key in the rustdoc per scope_in step 1(b).
   - **Watch out for**: `SerenaHoverClient::hover` is async (`async fn` per `executor.rs:645`). Make sure `G1Source::execute` is the async-trait variant (or `Box::pin` async return) — verify against `executor.rs:965` (`pub trait G1Source`) before wiring. If `G1Source` uses `#[async_trait]`, mirror it; if it returns `Pin<Box<dyn Future<…>>>`, mirror that. Do NOT change the trait shape.

3. **Commit 4 — `AstGrepG1Source` + `DiagnosticsG1Source`** (~30 LOC).
   - Both are production-shape stubs returning `G1ToolStatus::Unavailable`. Constructor `new()` + `Default` impl. Rustdoc cites the deferral with forward-WO trigger language: "real wiring lands in a follow-up WO that touches `PluginManager` channel surface" / "lands in a follow-up WO that adds `ucil-lsp-diagnostics` workspace dep".
   - `execute(&_query)` always returns `G1ToolOutput { kind: AstGrep / Diagnostics, status: G1ToolStatus::Unavailable, elapsed_ms: 0, payload: Value::Null, error: Some("…") }`. NO new dependency on `ucil-lsp-diagnostics`. NO `Cargo.toml` mod.
   - **Watch out for**: AC26 forbids `Cargo.toml` mods (`forbidden_paths` includes `crates/ucil-daemon/Cargo.toml`). All deps for the stubs are already in-tree (they only use `serde_json::Value::Null` + `G1ToolOutput`).

4. **Commit 5 — Wire `mod g1_sources` + 5 re-exports + lib.rs preamble** (~10 LOC, in `crates/ucil-daemon/src/lib.rs`).
   - Add `mod g1_sources;` to the module-list block in alphabetical position.
   - Add a single `pub use g1_sources::{AstGrepG1Source, DiagnosticsG1Source, G1SourceFactory, KgG1Source, SerenaG1Source};` line (alphabetical-within-block).
   - Append the WO-0049 sentence to the WO-0048 preamble paragraph per scope_in step 4 (the literal text is in the WO).
   - **Watch out for**: AC27 mandates ALL 5 symbols on a single `pub use g1_sources::{…}` line — verifier greps with `grep -nE 'KgG1Source|SerenaG1Source|AstGrepG1Source|DiagnosticsG1Source|G1SourceFactory' crates/ucil-daemon/src/lib.rs` expecting all 5. AC02 has a `clippy::doc_markdown` pre-flight: every uppercase identifier in the new doc paragraph MUST be backticked.

5. **Commit 6 — Extend `McpServer` + add `find_references` route** (~80 LOC, in `crates/ucil-daemon/src/server.rs`).
   - Add `pub g1_sources: Option<Arc<G1SourceFactory>>` to the `McpServer` struct at `server.rs:334-348`.
   - Add `pub fn with_g1_sources(mut self, factory: Arc<G1SourceFactory>) -> Self { self.g1_sources = Some(factory); self }` builder.
   - Update existing `McpServer::new()` and `McpServer::with_knowledge_graph(kg)` constructors to default `g1_sources: None`. Both must stay byte-identical in BEHAVIOR (only the new field added).
   - Insert the route in `handle_tools_call` (currently `server.rs:514-571`) BETWEEN the `search_code` and `understand_code` clauses: `if name == "find_references" && self.g1_sources.is_some() { return Self::handle_find_references(id, params, self.g1_sources.as_ref().unwrap().clone()).await; }` — note: `handle_tools_call` is currently sync (`fn`), so the executor may need to make it async OR dispatch the find_references branch via `tokio::task::block_in_place` + `Handle::current().block_on(…)`. Examine `handle_tools_call`'s existing async strategy first: is it already async-ready (some paths await), or does the daemon's caller (`handle_line`?) own the runtime? If `handle_tools_call` is sync today, you may need to either (a) make it async (preferred — minimal intrusion) or (b) use `tokio::runtime::Handle::current().block_on(…)`. The existing `handle_find_definition` is sync because `read_find_definition` is sync (uses blocking mutex). `handle_find_references` is async because it must await `execute_g1`.
   - **Watch out for**: AC14 requires `server::test_all_22_tools_registered` to stay green. The catalog at `ucil_tools()` (server.rs:223-310) already lists `find_references` (frozen WO-0010 surface); do NOT touch it. The route addition is in `handle_tools_call`, NOT in the catalog.

6. **Commit 7 — `handle_find_references` method on `McpServer`** (~80 LOC, in `crates/ucil-daemon/src/server.rs`).
   - `async fn handle_find_references(id: &Value, params: &Value, factory: Arc<G1SourceFactory>) -> Value` (note: `async` because it awaits `execute_g1`).
   - Parse `params.arguments.name` (REQUIRED string) and `arguments.file_path` (OPTIONAL string). Mirror the existing `handle_find_definition` arg-extraction error shape at `server.rs:610-616` (JSON-RPC -32602 + message naming `name`).
   - Construct `G1Query { symbol, file_path: PathBuf::from(file_path.unwrap_or_default()), line: 1, column: 1 }`.
   - Build sources via `factory.build()`. Wrap in `Vec<Box<dyn G1Source + Send + Sync + 'static>>`.
   - Call `let raw = execute_g1(q, sources, G1_MASTER_DEADLINE).await; let fused = fuse_g1(&raw);`.
   - Group `fused.entries` by `entry.fields["usage_type"]` (default `"call"`) into `BTreeMap<String, Vec<Value>>` for deterministic key ordering. For each fused entry, push `{ "file_path": location.file_path, "start_line": location.start_line, "end_line": location.end_line, "signature": fields.signature.unwrap_or(null), "contributing_sources": serialize_kind_list(entry.contributing_sources) }`.
   - Emit envelope `{ jsonrpc: "2.0", id, result: { _meta: { tool: "find_references", source: "g1-fused", found: <bool>, total_references: <count>, references_by_usage: <BTreeMap>, master_timed_out: raw.master_timed_out (or whatever the field is named on G1Outcome — verify) }, content: [{ type: "text", text: "<human summary>" }], isError: false } }`.
   - **Watch out for**: AC09 asserts `result._meta.total_references == 4` (counts FUSED entries, not pre-fusion source entries). Do NOT count `raw.results.iter().sum(...)` — count `fused.entries.len()`. AC08 asserts the `(util.rs, 10, 10)` entry has `signature == "fn foo() -> i32"` (Serena's value, fused into TreeSitter's row by `fuse_g1`). Do NOT route TreeSitter's raw output — go through `fuse_g1` so the Serena signature wins per the WO-0048 source-authority precedence.

7. **Commit 8 — Three module-root acceptance tests** (~300 LOC, in `crates/ucil-daemon/src/server.rs`).
   - All three tests live at MODULE ROOT of `server.rs` (NOT inside `mod tests {}`) per DEC-0007. They are `pub async fn test_find_references_tool()`, `pub async fn test_find_references_tool_unknown_symbol()`, `pub async fn test_find_references_tool_missing_name_param()` annotated `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`. (Note: existing module-root `find_definition` tests at server.rs:2148 use `#[test]` because `handle_find_definition` is sync — the find_references variants are async, hence `#[tokio::test]`.)
   - Build a `TestG1SourceFactory` returning 4 local `TestG1Source` impls per scope_in step 9(a) — these are NOT critical-dep mocks per DEC-0008 §4 (G1Source is UCIL-owned). Each TestG1Source returns a `G1ToolOutput { status: Available, payload: serde_json::to_value(Vec<G1FusionEntry>) }`.
   - The 5 sub-assertions per scope_in step 9 (envelope, meta fields, bucketing, fused signature, total_references) are documented in AC05–AC09 with the exact expected values. Use `assert_eq!` with operator-readable panic messages quoting the actual JSON content.
   - The unknown-symbol test factory returns 4 sources that all yield empty `Vec<G1FusionEntry>` payloads → `_meta.found == false`, no JSON-RPC error envelope.
   - The missing-name-param test omits `arguments.name` → JSON-RPC `error.code == -32602` and `error.message` mentions `name`.
   - **Watch out for**: DEC-0005 module-coherence permits up to ~200 LOC for a coherent test+helper commit (precedent: WO-0046 252-LOC framing). One commit for all 3 tests + the `TestG1SourceFactory` helper is acceptable per DEC-0005. AC25 allow-list validation requires each `git diff --name-only main...HEAD` path to be in the closed set: `crates/ucil-daemon/src/{server,executor,lib,g1_sources}.rs`, `scripts/verify/P2-W7-F05.sh`, `ucil-build/work-orders/0049-ready-for-review.md`. Do NOT add new tests files in `crates/ucil-daemon/tests/**` — `forbidden_paths` denies it.

8. **Commit 9 — `scripts/verify/P2-W7-F05.sh`** (~30 LOC).
   - Per scope_in step 11. Runs `cargo test -p ucil-daemon server::test_find_references_tool -- --nocapture` plus the 2 negative-path tests. Tees output through the WO-0042 alternation regex `grep -Eq 'test result: ok\. .* 0 failed|[0-9]+ tests? passed'`. Emits `[FAIL] P2-W7-F05: <reason>` on non-zero exit.
   - `chmod +x scripts/verify/P2-W7-F05.sh`.

9. **Commit 10 — `ucil-build/work-orders/0049-ready-for-review.md`** (~5 LOC).
   - The hand-off marker. Only after all 3 tests + clippy + build are green locally. Cite the commit shas and confirm AC03–AC11, AC25, AC27, AC34 are met locally.

**Risk**: H2 (the `find_references_by_name` / `references` table naming defect in scope_in step 1(a)) is the only place where the executor might genuinely STOP — if so, the executor must STOP and write a `ucil-build/decisions/proposed-DEC-XXXX-find-references-by-name-naming.md` rather than silently invent a new method. The recommended in-rustdoc divergence note (commit 2 watch-out) keeps the spec / implementation seam visible without an ADR.

## If the H1 hypothesis is wrong

If the next executor session ALSO terminates after only a partial commit (e.g., commits 2–4 land but 5+ don't), the root cause is environmental, not work-order-shape:

- **Falsification step 1**: check `~/.claude/projects/-home-rishidarkdevil-Desktop-ucil/sessions/` for the executor session's exit reason — turn-budget? token-budget? OOM? user-interrupt?
- **Falsification step 2**: confirm the autonomous loop is actually re-spawning executor sessions on this branch. The `monitor-bypass-p2-N-features-remaining` advisories indicate the loop is alive but spending turns on monitor housekeeping rather than on this WO. Inspect `scripts/run-phase.sh` orchestration for whether the loop sees `ucil-build/work-orders/0049-find-references-and-g1-source-production-wiring.json` with `feature_ids: ["P2-W7-F05"]` and `passes: false` and routes a fresh executor against it.
- **Falsification step 3**: if the loop re-spawns and terminates again, escalate as a Bucket E "executor cannot complete a 16-step / ~740-LOC WO in one session" → planner SHOULD split the WO into 3 sub-WOs (g1_sources.rs / server.rs handler / acceptance tests) per the same precedent that produced WO-0047 → WO-0048 → WO-0049 (the G1 fan-out / fuse / find_references trilogy). This is remediation category (b) — planner re-scope — and would require a fresh ADR documenting the WO-size ceiling discovered.

## Cross-references

- Verifier rejection: `ucil-build/rejections/WO-0049.md` (2026-05-05T01:03:38Z, session `vrf-162f0721-…`)
- Critic verdict: `ucil-build/critic-reports/WO-0049.md` (verdict BLOCKED, session `crt-WO-0049-2026-05-05-74ac9bb7`)
- Work-order: `ucil-build/work-orders/0049-find-references-and-g1-source-production-wiring.json`
- Feature: `ucil-build/feature-list.json` entry `P2-W7-F05` (`attempts: 1`, `passes: false`)
- Branch HEAD: `f197ad1389c0e2ef67b67e5364b45fc25970bcfd` (clean, pushed to `origin/feat/WO-0049-find-references-and-g1-source-production-wiring`)
- Foundational refactors that landed and must be preserved:
  - `crates/ucil-daemon/src/executor.rs:985-996` — `#[tracing::instrument(name = "ucil.group.structural.source", …)]` on `run_g1_source` (closes WO-0047 lessons line 296).
  - `crates/ucil-daemon/src/executor.rs:1063-1080` — `flatten() + debug_assert_eq!` replacement of the `.expect(...)` (closes WO-0047 lessons line 308).
- KG surface to consume in `KgG1Source::execute`:
  - `crates/ucil-core/src/knowledge_graph.rs:1221` — `resolve_symbol(name)` (entry point).
  - `crates/ucil-core/src/knowledge_graph.rs:1086-1104` — `list_relations_by_target(entity_id)` (the actual "find references" path, filtered by `relation.kind`).
  - `crates/ucil-core/src/knowledge_graph.rs:882` — `get_entity_by_id(id)` (resolve `relation.source_id` → `(file_path, start_line, end_line)`).
- Existing precedent for the handler shape:
  - `crates/ucil-daemon/src/server.rs:601-627` — `handle_find_definition` (arg parsing, JSON-RPC -32602 on missing name, envelope shape).
  - `crates/ucil-daemon/src/server.rs:2126-2317` — `test_find_definition_tool` + 2 negative paths (template for the 3 module-root tests).
  - `crates/ucil-daemon/src/executor.rs:753-810` — `enrich_find_definition` (template for `Arc<dyn SerenaHoverClient>` consumer pattern).
- Frozen public API surface to consume (NOT modify):
  - `crates/ucil-daemon/src/executor.rs:1111` — `pub async fn execute_g1` (WO-0047 final).
  - `crates/ucil-daemon/src/executor.rs:1369` — `pub fn fuse_g1` (WO-0048 final).
  - `crates/ucil-daemon/src/executor.rs:811`, `:820` — `G1_MASTER_DEADLINE`, `G1_PER_SOURCE_DEADLINE` consts.
  - `crates/ucil-daemon/src/executor.rs:965` — `pub trait G1Source` (the seam — UCIL-owned per DEC-0008).
