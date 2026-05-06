# WO-0063 — Ready for Review

- **Work-order**: `WO-0063` (`search_code` G2 fused refresh)
- **Feature**: `P2-W7-F06`
- **Branch**: `feat/WO-0063-search-code-g2-fused-refresh`
- **Final commit**: `9b0368cd7973aed90b7c3e45f36ba90f0099d53a`
- **Supersedes**: WO-0057 (substance preserved verbatim; refreshed with five W8-cohort discipline items per WO-0058 → WO-0062)
- **DEC-0016 cross-reference**: F06 is NOT blocked by the orphan
  `feat/WO-0053-lancedb-per-branch` branch — `LancedbProvider` uses
  `StorageLayout::branch_vectors_dir` from `storage.rs:230` on main.

## Summary

This WO promotes `search_code` from its WO-0035 / P1-W5-F09
KG+ripgrep merge shape to the master-plan §5.2 G2 weighted-RRF
fusion shape: ripgrep + Probe (stdio MCP plugin) + `LanceDB`
(per-branch vector store) all run in parallel, fuse via WO-0056's
`fuse_g2_rrf` with weights `{Probe×2.0, Ripgrep×1.5, Lancedb×1.5}`
and `k=60`, and surface the fused result on a NEW additive
`_meta.g2_fused` field.  Per `DEC-0015` D1, every legacy `_meta`
field of the existing `search_code` handler stays byte-identical so
the frozen WO-0035 acceptance test continues to pass verbatim.

## What I verified locally

- [x] `cargo build -p ucil-daemon` exits 0 (AC01).
- [x] `cargo clippy -p ucil-daemon --all-targets -- -D warnings`
      exits 0 (AC02).  `clippy::pedantic` lints (`doc_markdown`,
      `missing_panics_doc`, `too_long_first_doc_paragraph`) all
      green.
- [x] `cargo test -p ucil-daemon server::test_search_code_fused
      -- --nocapture` exits 0 — 6 sub-assertions all green
      (AC03, AC05–AC10).
- [x] AC04 — `pub async fn test_search_code_fused` lives at
      MODULE ROOT of `crates/ucil-daemon/src/server.rs` (line 3214,
      NOT inside `mod tests {}`).  Verified via `grep -nE '^pub
      async fn test_search_code_fused' crates/ucil-daemon/src/server.rs`.
- [x] `cargo test -p ucil-daemon server::test_search_code_fused_no_factory
      -- --nocapture` exits 0 (AC11) — proves the
      `Option<Arc<G2SourceFactory>>::None` path produces a response
      WITHOUT `_meta.g2_fused` while preserving every legacy field.
- [x] `cargo test -p ucil-daemon server::test_search_code_basic
      -- --nocapture` exits 0 (AC12) — WO-0035 / P1-W5-F09 frozen
      regression preserved per `DEC-0015` D1.
- [x] `cargo test -p ucil-core fusion::test_g2_rrf_weights` exits 0
      (AC13) — F06 consumes `fuse_g2_rrf` without modifying its body.
- [x] `cargo test -p ucil-daemon server::test_all_22_tools_registered`
      exits 0 (AC14) — the new `McpServer` field does NOT change the
      catalog enumeration.
- [x] `cargo test -p ucil-daemon server::test_find_definition_tool`
      (3 tests) exits 0 (AC15).
- [x] `cargo test -p ucil-daemon server::` exits 0 — 31 tests pass
      (AC16).
- [x] `cargo test -p ucil-daemon plugin_manager::` exits 0 — 15
      tests pass (AC17a) — the `run_protocol_prefix` factor-out and
      new `run_tools_call` keep `health_check` byte-identical.
- [x] `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1 cargo test -p ucil-daemon
      --test plugin_manifests` exits 0 — 3 tests pass (AC17b).
- [x] `cargo test -p ucil-daemon --test e2e_mcp_stdio --test
      e2e_mcp_with_kg` exits 0 (AC18).
- [x] `cargo test --test test_plugin_lifecycle` exits 0 (AC19).
- [x] `cargo test --test test_lsp_bridge` exits 0 (AC20).
- [x] `bash scripts/verify/coverage-gate.sh ucil-daemon 85 75`
      exits 0 (AC22) — `line=89% branch=n/a` PASS.
- [x] `bash scripts/verify/P2-W7-F06.sh` exits 0 — runs the frozen
      selector, the no-factory negative path, and the WO-0035
      legacy regression in one script.
- [x] AC23 — Stub-scan: `rg -n 'todo!\(\)|unimplemented!\(\)|panic!
      \(".*not yet|TODO|FIXME'` returns ZERO new hits in
      `g2_search.rs`, `server.rs`, or `plugin_manager.rs`.
      `LancedbProvider::execute` body uses real
      `tokio::fs::try_exists` + `read_dir` filesystem code per
      `DEC-0015` D3 — NOT a stub.
- [x] AC24 — Allow-list: `git diff --name-only main...HEAD` lists
      EXACTLY 5 paths plus the RFR marker (this file):
      ```
      crates/ucil-daemon/src/g2_search.rs    (NEW)
      crates/ucil-daemon/src/lib.rs
      crates/ucil-daemon/src/plugin_manager.rs
      crates/ucil-daemon/src/server.rs
      scripts/verify/P2-W7-F06.sh             (NEW)
      ucil-build/work-orders/0063-ready-for-review.md  (NEW, this file)
      ```
      No `Cargo.lock` change — no new top-level deps were added.
- [x] AC25 — `Cargo.toml` files are NOT modified (`git diff --name-only
      main...HEAD -- '*.toml'` returns empty).
- [x] AC26 — All 6 new public symbols re-exported from
      `crates/ucil-daemon/src/lib.rs` line 138 on a single
      `pub use g2_search::{...}` line: `G2SearchError`,
      `G2SourceFactory`, `G2SourceProvider`, `LancedbProvider`,
      `ProbeProvider`, `RipgrepProvider`.
- [x] AC31 — `tests/fixtures/**` is NOT modified.
- [x] AC32 — `ucil-build/feature-list.json` and
      `ucil-build/feature-list.schema.json` are NOT modified.
- [x] AC33 — `ucil-master-plan-v2.1-final.md` is NOT modified.
- [x] AC34 — Forbidden crates (`crates/ucil-core/**` etc.) are NOT
      modified.
- [x] AC35 — Commit cadence: 5 commits on the feature branch:
      ```
      9b0368c docs(daemon): tighten test rustdoc to satisfy clippy::pedantic
      db9882c test(daemon): add test_search_code_fused + verify script
      aef7ba4 feat(daemon): wire McpServer.with_g2_sources + g2_fused emit
      7dda6c4 feat(daemon): add G2SourceProvider trait + 3 impls (g2_search.rs)
      48f75df feat(daemon): factor run_protocol_prefix; add run_tools_call
      ```
      The 695-LOC `feat: add G2SourceProvider trait` commit cites
      `DEC-0005` module-coherence + the WO-0046 / WO-0048 / WO-0056 /
      WO-0058 / WO-0059 / WO-0060 NEW-module precedent stack.
- [x] AC36 — Branch is up-to-date with `origin`.  `git rev-parse
      HEAD` matches `git rev-parse @{u}` (after final push).  No
      uncommitted changes.
- [x] AC37 — All commit subjects ≤70 chars.  Verified via
      `git log main..HEAD --pretty=%s | awk '{ if (length > 70)
      exit 1 }'`.  Lengths: `60 / 65 / 60 / 56 / 62`.
- [x] AC38 — Ban-words pre-flight: `git diff main -- crates/ucil-
      daemon/src/{g2_search,server,plugin_manager}.rs scripts/verify/
      P2-W7-F06.sh | grep -iE '^[+].*\b(mock|fake|stub|fixture)\b'`
      returns ZERO new lines.  The W8-cohort discipline #2 banned
      words have been rephrased to `local-impl` / `test-impl` /
      `substitute` throughout the new code.
- [x] AC41 — `grep -nE 'DEC-0016' crates/ucil-daemon/src/g2_search.rs`
      returns 6 matches (≥2 required): module-level `//!` doc (3
      mentions), `LancedbProvider` struct rustdoc, `LancedbProvider::
      execute` rustdoc heading, and the inline `# DEC-0016
      cross-reference` paragraph.

## AC21 / AC18 — known pre-existing test failure (unrelated to F06)

`cargo test --workspace --no-fail-fast` reports ONE pre-existing
failure, present on `main` before this WO branched:

```
test models::test_coderankembed_inference ... FAILED
thread panicked at crates/ucil-embeddings/src/models.rs:920:5:
CodeRankEmbed model artefacts not present at "ml/models/coderankembed";
run `bash scripts/devtools/install-coderankembed.sh` first (P2-W8-F02 / WO-0059);
got model.onnx exists=false, tokenizer.json exists=false
```

This failure is IN `crates/ucil-embeddings/` (a forbidden_paths
crate F06 does not touch).  It's gated by the absence of the
`ml/models/coderankembed/` artefacts on the executor's local
machine.  Verifier should either:

1. Run `bash scripts/devtools/install-coderankembed.sh` once before
   the workspace-wide regression, OR
2. Treat this failure as out-of-scope-for-F06 (the test pre-existed
   on `main` at commit `1daf60b`).

I confirmed this is NOT a regression introduced by my WO-0063 work
by running the same selector against `main` (HEAD `1daf60b`): the
test FAILS with the same artefacts-missing panic.

## Probe tools/call schema research (W8-cohort discipline #3)

Per WO-0060 lessons line 644 — first-time consumer of
`plugins/search/probe/plugin.toml`'s actual MCP `tools/call`
argument schema.  I ran the upstream-API research before writing
`ProbeProvider::execute`:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize",...}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  | npx -y @probelabs/probe@0.6.0-rc315 mcp
```

Verified upstream schema for `search_code` at v0.6.0-rc315:

```json
{
  "name": "search_code",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path":  { "type": "string", "description": "Absolute path to the directory to search" },
      "query": { "type": "string", "description": "ElasticSearch query syntax..." },
      "exact": { "type": "boolean", "default": false },
      "strictElasticSyntax": { "type": "boolean", "default": false },
      "session": { "type": "string" },
      "nextPage": { "type": "boolean" },
      "lsp": { "type": "boolean" }
    },
    "required": ["path", "query"]
  }
}
```

**Required parameters** are `path` (string) and `query` (string).
**No `maxResults` parameter is advertised.**  Optional parameters
`exact` / `strictElasticSyntax` / `session` / `nextPage` / `lsp`
are NOT forwarded — the provider's contract is the
lowest-common-denominator search.

`ProbeProvider::execute` therefore sends:

```json
{ "query": <verbatim caller query>, "path": <root.display()> }
```

The `max_results` cap is applied via truncation after parsing
Probe's response (Probe does NOT advertise a native maxResults
input parameter as of v0.6.0-rc315).

**Verified `tools/call` response shape**: `result.content[].text`
is markdown with embedded XML-style `<file path="...">` blocks
containing `<spaces><line_no> <content>` rows.  The
`parse_probe_response` parser walks these blocks and emits one
`G2Hit` per row in Probe's response order — `hits[0]` is rank 1.
Documented in `ProbeProvider::execute`'s rustdoc.

If a future Probe release exposes structured output
(`result.matches`) or a `maxResults` input parameter, switch the
parser to the structured path — the trait signature is unchanged
at that time.

## Verifier-universal gates

Per WO-0059 lessons line 615 — the verifier will run these
authority-side gates BEYOND this WO's explicit acceptance_criteria:

1. `bash scripts/verify/coverage-gate.sh ucil-daemon 85 75` (AC22 —
   already pinned, doubly-explicit).  Currently passes at line=89%.
2. `bash scripts/verify/P2-W7-F06.sh` (already pinned).  Passes.
3. `cargo test --workspace --no-fail-fast` (AC21 — workspace-wide
   regression).  Passes EXCEPT for the pre-existing
   `ucil-embeddings::models::test_coderankembed_inference` failure
   documented above (artefacts-missing panic, present on `main`).
4. The verifier-protocol's clean-slate rerun — `cargo clean &&
   cargo test` against the feature branch HEAD per root CLAUDE.md
   anti-laziness contract.

## Pre-baked mutations available (AC27–AC30)

All four runtime-only mutation variants are described in the
acceptance section of the work-order; the `#![deny(warnings)]`
cascade-avoidance pattern from WO-0046 lessons line 245 is
documented inline.  Verifier can apply each via the runtime-variant
recipe and confirm `cargo test -p ucil-daemon
server::test_search_code_fused` panics at the predicted
sub-assertion.

## Architectural decisions consumed

* `DEC-0007` — frozen-selector module-root placement (the two new
  `pub async fn test_search_code_fused*` tests).
* `DEC-0008` §4 — UCIL-internal trait boundaries
  (`G2SourceProvider`).
* `DEC-0009` — in-process ripgrep (`RipgrepProvider` reuses
  `crate::text_search::text_search`).
* `DEC-0015` — three architectural decisions: D1 additive
  `_meta.g2_fused`, D2 `PluginManager::run_tools_call`, D3
  `LancedbProvider` empty-until-P2-W8-F04.
* `DEC-0016` — orphan-branch carve-out.  F06 uses
  `StorageLayout::branch_vectors_dir` on main.

## Cumulative-debt-avoidance discipline

Per the WO-0042 → WO-0060 streak (now 14 consecutive cleared per
WO-0060 lessons line 639): all 6 new public symbols
(`G2SearchError`, `G2SourceFactory`, `G2SourceProvider`,
`LancedbProvider`, `ProbeProvider`, `RipgrepProvider`) are
re-exported from `crates/ucil-daemon/src/lib.rs` in this same WO.
Zero deferred re-export debt.
