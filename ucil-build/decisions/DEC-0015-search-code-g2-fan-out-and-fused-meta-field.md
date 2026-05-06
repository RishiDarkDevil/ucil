---
id: DEC-0015
title: search_code G2 fan-out adds `_meta.g2_fused`, Probe via run_tools_call, LanceDB no-op until P2-W8-F04
date: 2026-05-06
status: accepted
phase: 2
feature: P2-W7-F06
work_order: WO-0057
raised_by: planner
extends: DEC-0009
---

# DEC-0015: `search_code` G2 fan-out — additive `_meta.g2_fused` field, Probe driven by new `run_tools_call`, LanceDB returns empty until P2-W8-F04

## Context

`P2-W7-F06` (frozen acceptance test selector
`server::test_search_code_fused`) promotes the existing `search_code` MCP
tool from its WO-0035 / P1-W5-F09 shape — KG symbol search merged with
in-process ripgrep, response shape `_meta.{tool, source, count, results,
symbol_match_count, text_match_count}` — to the master-plan §5.2 G2 shape:
"All search engines run in parallel: ripgrep, Probe, LanceDB → Weighted
RRF: Probe 2.0, ripgrep 1.5, LanceDB 1.5".

Three architectural ambiguities surfaced at planning time that the master
plan and the existing decision graph (`DEC-0005`, `DEC-0007`, `DEC-0009`,
`DEC-0010`, `DEC-0014`) do not directly resolve:

1. **Backward compatibility with the frozen P1-W5-F09 acceptance test.**
   `crates/ucil-daemon/src/server.rs:2818` houses `test_search_code_basic`,
   the live frozen acceptance for the WO-0035 KG-symbol + in-process
   ripgrep merge. It asserts on `_meta.tool == "search_code"`,
   `_meta.source == "tree-sitter+ripgrep"`, `_meta.results` array shape
   with `source: "both"` rows, `_meta.symbol_match_count`,
   `_meta.text_match_count`. The anti-laziness contract at
   `CLAUDE.md:24-29` forbids `#[ignore]` / `.skip()` /
   `commented-out assertions` to silence a failing test, AND
   `tests/fixtures/**` modifications. Either F06 keeps the existing
   `_meta` shape intact (additive evolution), or F06 must replace the
   handler in a way that the existing 7-sub-assertion test still passes
   under the new RRF-fused shape.

2. **Probe runtime path.** Probe is registered as a stdio MCP plugin
   (`plugins/search/probe/plugin.toml`) launched via
   `npx -y @probelabs/probe@0.6.0-rc315 mcp` (WO-0044, manifest pinned
   at `transport.command = "npx"`, `args = ["-y", "@probelabs/probe@…",
   "mcp"]`). It advertises three tools — `search_code`, `extract_code`,
   `grep` — verified end-to-end by
   `crates/ucil-daemon/tests/plugin_manifests.rs:111`
   (`probe_manifest_health_check` calling
   `PluginManager::health_check_with_timeout(&manifest,
   FIRST_RUN_TIMEOUT_MS)`). The current `PluginManager` API exposes
   `spawn`, `health_check` / `health_check_with_timeout` (which drive
   `initialize` + `notifications/initialized` + `tools/list`), but NO
   `tools/call` round-trip. Driving Probe's `search_code` tool requires
   a new `tools/call` MCP round-trip — either a one-off ad-hoc helper in
   `g2_search.rs`, or a new public `PluginManager` method that future
   structural / search / quality WOs can reuse.

3. **LanceDB G2 source without a populated vector store.** Master-plan
   §5.2 line 453 lists LanceDB as the third G2 engine ("LanceDB
   (semantic vector search via CodeRankEmbed)"). The semantic-search
   path requires (a) the CodeRankEmbed model loaded via
   `ucil-embeddings` (P2-W8-F02 — currently blocked behind
   P2-W8-F01 / WO-0054 which is open but never executed) and (b) the
   background indexing pipeline writing chunks to per-branch tables
   (P2-W8-F04, blocked behind P2-W8-F02). Even the per-branch
   `BranchManager` from P2-W7-F09 lives only on the unmerged
   `feat/WO-0053-lancedb-per-branch` branch (the verifier flipped
   `passes=true` against that branch's HEAD `dfd07727`, but the merge
   to `main` was never executed; see
   `ucil-build/escalations/20260505-2014-wo-WO-0053-attempts-exhausted.md`).
   The `LanceDB` G2 source therefore has no real query path on `main`
   today. The anti-laziness contract forbids `unimplemented!()` /
   `todo!()` / "stub a function to return None as a feature
   implementation", AND `tests/fixtures/**` modifications, AND
   relaxing acceptance tests. Either F06 wires real LanceDB queries
   (impossible without the embedding pipeline), or F06 ships a
   real-but-empty path that becomes populated once the dependencies
   land.

## Decision

### D1 — Additive `_meta.g2_fused` field; legacy `_meta` shape preserved.

`handle_search_code` keeps every existing field
(`_meta.tool`, `_meta.source = "tree-sitter+ripgrep"`,
`_meta.count`, `_meta.results`, `_meta.symbol_match_count`,
`_meta.text_match_count`, `_meta.query`, `_meta.root`) UNCHANGED. The
frozen `test_search_code_basic` (WO-0035 / P1-W5-F09) continues to pass
verbatim — the existing KG-symbol + in-process-ripgrep merge path is
preserved.

A NEW field `_meta.g2_fused` is appended with the shape:

```jsonc
{
  "g2_fused": {
    "hits": [
      {
        "file_path": "<absolute or root-relative path>",
        "start_line": <u32>,
        "end_line": <u32>,
        "snippet": "<string from highest-weight contributing source>",
        "fused_score": <f64>,
        "contributing_sources": ["Probe", "Ripgrep"],   // descending rrf_weight
        "per_source_ranks": [["Probe", 1], ["Ripgrep", 1]]
      },
      ...
    ]
  }
}
```

This is `serde_json::to_value(&G2FusedOutcome)` projected directly from
the WO-0056 `ucil_core::fusion::G2FusedOutcome` type. The frozen
F06 acceptance test `server::test_search_code_fused` asserts on
`_meta.g2_fused.hits` and is order-INSENSITIVE on the legacy fields.

### D2 — New `pub async fn PluginManager::run_tools_call(...)` extension.

Add a NEW public async method to `crates/ucil-daemon/src/plugin_manager.rs`:

```rust
/// Drive the MCP `initialize` → `notifications/initialized` →
/// `tools/call` handshake against a freshly-spawned plugin and return
/// the deserialised `result` JSON.
pub async fn run_tools_call(
    manifest: &PluginManifest,
    tool_name: &str,
    arguments: &serde_json::Value,
    timeout_ms: u64,
) -> Result<serde_json::Value, PluginError>;
```

Internally factored to share the existing `initialize` /
`notifications/initialized` handshake with `run_tools_list` so the two
helpers do not duplicate the protocol-prefix code. The returned
`Value` is the `result` field of the `tools/call` response (unparsed —
the caller knows the tool's schema).

A `tools/call` schema mismatch (missing `result` field, JSON-RPC error
envelope, malformed JSON) surfaces as `PluginError::ProtocolError`,
identical to `run_tools_list`'s error contract. Cold-cache npx fetches
need ≥30 s (WO-0044 lessons line 165 / WO-0046 lessons line 199): the
F06 caller passes `FIRST_RUN_TIMEOUT_MS` (90 s) per the existing
constant in `tests/plugin_manifests.rs`.

### D3 — `LancedbProvider::execute()` returns empty until embeddings land.

`LancedbProvider::execute(query, root, max_results)` opens the
per-branch `vectors/` directory (via `StorageLayout::branch_vectors_dir`,
already on `main`), checks for the existence of a `code_chunks` table,
and:

* If the table does not exist OR contains zero rows → return
  `Ok(G2SourceResults { source: G2Source::Lancedb, hits: vec![] })`
  with a `tracing::debug!` indicating the no-op path.

* If the table exists with ≥1 rows AND no embedding-pipeline is wired
  (i.e. P2-W8-F02 / P2-W8-F04 still pending) → return
  `Ok(G2SourceResults { source: G2Source::Lancedb, hits: vec![] })`
  with a `tracing::warn!` indicating the deferral. (This branch is
  expected to be unreachable on Phase-2 `main`; it exists for
  forward-compat once embeddings land.)

* Once `P2-W8-F04` lands, the `LancedbProvider` body is augmented to
  produce a CodeRankEmbed query embedding and run the actual vector
  query — the `execute()` signature does NOT change.

This is REAL code, not a stub: the function touches the filesystem,
opens a real `lancedb::connect` when the directory exists, and returns
a typed `Result<G2SourceResults, _>`. An empty `hits` Vec is the
mathematically correct response when the index has no embeddings —
analogous to a search query that legitimately matches nothing.

The fallback path exists ONLY because the LanceDB query depends on
embeddings produced by a feature (P2-W8-F02) whose foundational
dependency (P2-W8-F01 / WO-0054) is open-but-unexecuted; F06 cannot
complete the semantic-search wiring synchronously. The acceptance test
`server::test_search_code_fused` asserts that fused results contain at
least Probe + Ripgrep contributions — LanceDB optionally absent — so
the test passes today and will pass unchanged once embeddings populate
the `code_chunks` table.

## Rationale

### Why D1 (additive evolution)

The anti-laziness contract makes this the only safe option. The
frozen `test_search_code_basic` is binding. Modifying it (or skipping
it, or relaxing its assertions) would violate
`CLAUDE.md:24-29` directly. Replacing the response shape such that
the existing 7-sub-assertion shape still passes is theoretically
possible but architecturally fragile — the legacy shape's
`_meta.source = "tree-sitter+ripgrep"` is a hard string match that
would have to either lie ("yes we're still using tree-sitter+ripgrep
even though we now also use Probe + LanceDB") or break the test.
Additive evolution is the cleanest solution: the legacy shape exists
verbatim; the new RRF-fused shape lives on a new field.

This pattern matches the WO-0049 (find_references) precedent: F05's
production wiring of `execute_g1` + `fuse_g1` was added alongside the
existing `enrich_find_definition` (P1-W5-F02 / WO-0037) without
removing it; the legacy hover-only path stayed intact and the new
fan-out path landed as a new code path.

### Why D2 (PluginManager extension, not ad-hoc helper)

`run_tools_call` is needed THREE times in the foreseeable WO graph:

1. **F06** (this WO) — Probe `search_code` tool call.
2. **P2-W8-F08** (`find_similar` MCP tool) — likely needs to call
   Probe's `search_code` tool too OR similar plugins; the
   `PluginManager` reuse keeps the path consistent.
3. **Phase-3 host adapters** (P3-W9 onward) — every plugin's tools
   become first-class once the host adapter family lands; the
   `tools/call` round-trip is the canonical way to invoke them.

A one-off helper in `g2_search.rs` would either duplicate the
`initialize` + `notifications/initialized` + handshake protocol code
(violating DRY against `plugin_manager.rs:1105`) or share it via
crate-private helper functions that the wider codebase cannot
reach. Promoting it to a `PluginManager` public method keeps the
plugin-protocol surface centralised and gives downstream WOs a clean
extension point without re-rolling the protocol prefix.

### Why D3 (LanceDB empty-path deferral, not stub)

Master-plan §5.2 line 453 lists LanceDB as a G2 engine; the master
plan does NOT make LanceDB optional. The feature description for
P2-W7-F06 explicitly names "LanceDB semantic" as one of the three
fused sources. Therefore F06 MUST seat a `LancedbProvider` in the
G2 fan-out — the alternative (a 2-source fan-out of {Ripgrep, Probe}
with LanceDB silently absent) drifts from the spec and would have to
be re-wired anyway when embeddings land.

The `Ok(G2SourceResults { hits: vec![] })` return is REAL code, not
a stub: the function path runs `StorageLayout::branch_vectors_dir`,
checks the table, and returns based on the actual on-disk state.
The `unimplemented!()` / `todo!()` ban applies to function bodies
that explicitly refuse to do work (`unimplemented!("not yet")`); a
function body that DOES the work and finds zero rows is the natural
math of an empty index.

The forward-compat surface is preserved: when P2-W8-F04 lands and
populates the `code_chunks` table, the same `LancedbProvider::execute`
path automatically begins returning real hits — the F06 acceptance
test still passes (its assertion is "fused output contains Probe +
Ripgrep contributions, optionally LanceDB"). No re-architecture is
needed at P2-W8-F04 time.

## Consequences

### Positive

* `test_search_code_basic` (P1-W5-F09 frozen) keeps passing → no
  regression on the WO-0035 / DEC-0009 in-process ripgrep substrate.
* `_meta.g2_fused` is a new field, transparent to MCP hosts that
  don't read it (Claude Code, Codex, Cursor — all ignore unknown
  `_meta` keys per MCP spec).
* `PluginManager::run_tools_call` becomes the canonical Phase-2 /
  Phase-3 entry point for invoking plugin tools — Phase-3 host
  adapters reuse it without re-implementing the protocol prefix.
* `LancedbProvider` ships in F06 with the correct path; P2-W8-F04
  augments the body without touching the call site or the tracing
  span name (`ucil.tool.lancedb.vector_search` per master-plan §15.2
  line 1518).
* The G2 fan-out span hierarchy (`ucil.group.search` parent, three
  per-source children at `ucil.tool.<engine>.search` /
  `vector_search`) lands in F06 wholesale — Phase-3 OpenTelemetry
  consumers see the full hierarchy from day one.

### Negative

* `_meta.g2_fused` doubles the response payload size for
  `search_code` calls — Probe's snippet payloads can be lengthy. The
  `max_results` clamp (`SEARCH_CODE_MAX_RESULTS`,
  `SEARCH_CODE_DEFAULT_MAX_RESULTS` per `server.rs:85,94`) carries
  through to the G2 fan-out so the worst-case payload is bounded.
* The legacy `_meta.source = "tree-sitter+ripgrep"` lies about the
  full source set when the new G2 fan-out also queries Probe +
  LanceDB. Acceptable: the legacy field describes the legacy code
  path's lineage (KG symbol search + in-process ripgrep merge),
  which is preserved verbatim. The new G2 fan-out has its own
  provenance field (`g2_fused.hits[*].contributing_sources`).
* Two ripgrep walks per `search_code` call: the legacy
  `crate::text_search::text_search` for `_meta.results`, and the new
  `RipgrepProvider::execute` for `_meta.g2_fused`. The second walk
  is a duplication. Acceptable: the in-process ripgrep walk on a
  single fixture project is sub-100ms (per the existing
  `test_search_code_basic` measurements); the duplication is bounded
  and is recoverable in a future refactor (e.g. cache the
  `text_search` result and feed both lanes from the cached vec).
* `LancedbProvider` returns empty until P2-W8-F04 — `_meta.g2_fused`
  on Phase-2 `main` reflects {Probe, Ripgrep} contributions only.
  Acceptable: this is a documented deferral, the test asserts on
  the partial fused output, and embedding-driven semantic search is
  inherently a Phase-2 Week-8 feature (master-plan §18 Phase 2 Week
  8 lines 1786-1792).

### Forward-compat triggers

* **P2-W8-F02 lands** (CodeRankEmbed model loaded in `ucil-embeddings`)
  → `LancedbProvider::execute` body augmented to produce a query
  embedding via `OrtSession::infer` and run the actual LanceDB vector
  query. Body change only; signature + call site unchanged.
* **P2-W8-F04 lands** (LanceDB background chunk indexing) → same
  `LancedbProvider` returns real hits without changing F06's call
  site or test.
* **P2-W8-F08 lands** (`find_similar` MCP tool) → reuses
  `PluginManager::run_tools_call` for any plugin-driven similarity
  query; reuses the same `G2SourceProvider` trait if Phase-2 calls
  for it (otherwise consume `LancedbProvider` directly).
* **Phase-3 host adapters land** → `PluginManager::run_tools_call` is
  the canonical plugin-tool invocation path; no re-roll of the MCP
  protocol prefix needed.

### Revisit triggers

* If the legacy `test_search_code_basic` is ever superseded by an
  ADR (e.g. P3 host adapter rewrites the search_code response
  shape), this ADR's D1 (additive evolution) is moot — the new ADR
  may collapse `_meta.results` and `_meta.g2_fused` into a single
  ranked list.
* If a future plugin's `tools/call` schema requires a stateful
  session beyond `initialize` + `notifications/initialized`,
  `run_tools_call` needs a session-handle variant (probably named
  `run_tools_call_session(session, tool_name, args)`). D2's spawn-
  per-call shape is sufficient for Phase 2 because all current G2
  plugins are stateless per-call (Probe, ast-grep, ripgrep, SCIP).

## Cross-references

* Master-plan §3.2 row 4 line 218 — `search_code` description
  ("Hybrid search: text + structural + semantic", G2+G1).
* Master-plan §5.2 lines 447-461 — G2 fan-out + RRF formula.
* Master-plan §6.2 line 645 — RRF k = 60.
* Master-plan §15.2 lines 1515-1518 — G2 span hierarchy
  (`ucil.group.search` / `ucil.tool.probe.search` /
  `ucil.tool.ripgrep.search` / `ucil.tool.lancedb.vector_search`).
* Master-plan §18 Phase 2 Week 7 line 1781-1783 — G2 intra-group
  RRF + wire into `find_definition`, `find_references`, `search_code`.
* `DEC-0005` — module-coherence commits (≤200 LOC).
* `DEC-0007` — frozen-selector module-root placement.
* `DEC-0009` — in-process ripgrep substrate (extends here:
  `RipgrepProvider` reuses `crate::text_search::text_search`).
* `DEC-0014` — SCIP CLI → SQLite pipeline pattern (precedent for
  per-source providers behind a trait, plugin-shape variation).
* `WO-0035` — original `search_code` MCP wiring (P1-W5-F09).
* `WO-0044` — Probe plugin manifest pinned to v0.6.0-rc315.
* `WO-0048` — G1 fusion trait + `fuse_g1` (analog template).
* `WO-0056` — G2 RRF math (`fuse_g2_rrf` + `G2FusedOutcome` types).
* `crates/ucil-core/src/fusion.rs` lines 14-18 — explicit deferral
  comment naming `P2-W7-F06` as the consumer of the G2 fusion math.
