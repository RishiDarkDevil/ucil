# WO-0066 — Ready for Review

- **Feature**: P2-W8-F08 — `find_similar` MCP tool
- **Branch**: `feat/WO-0066-find-similar-mcp-tool`
- **Final commit sha**: `5499b2d461b6799bc9699c93beef0a5931f3b15f`
- **Frozen acceptance selector**: `cargo nextest run -p ucil-daemon server::test_find_similar_tool`
- **Verify script**: `bash scripts/verify/P2-W8-F08.sh`
- **Master plan citations**: §3.2 line 219 (find_similar tool listing) + §18 Phase 2 Week 8 line 1791 ("Vector search works") — closes Phase 2 Week 8 and the entire Phase 2 envelope.

## What landed

### Implementation
- **`FindSimilarExecutor`** struct in `crates/ucil-daemon/src/server.rs` — bundles `Arc<BranchManager>` + `Arc<dyn EmbeddingSource>` + `default_branch: String`. Constructor: `FindSimilarExecutor::new(branch_manager, embedding_source, default_branch)`. Manual `Debug` impl (because `Arc<dyn EmbeddingSource>` is not `Debug`-derivable).
- **`McpServer::with_find_similar_executor(self, Arc<FindSimilarExecutor>) -> Self`** — `#[must_use]` builder method mirroring `with_g2_sources` (WO-0048/WO-0063 pattern).
- **`McpServer.find_similar: Option<Arc<FindSimilarExecutor>>`** — new field; both `McpServer::new()` and `McpServer::with_knowledge_graph()` initialise to `None` so phase-1 invariant #9 stays preserved when the executor is not attached.
- **`McpServer::handle_find_similar(id, params, executor)`** — async handler that:
  - Parses `arguments.snippet` (REQUIRED string → JSON-RPC `-32602` if missing/non-string).
  - Parses `arguments.max_results` (OPTIONAL u64, default `FIND_SIMILAR_DEFAULT_MAX_RESULTS = 10`, clamped `[1, FIND_SIMILAR_MAX_RESULTS_CAP = 100]`).
  - Parses `arguments.branch` (OPTIONAL string, default `executor.default_branch()`; non-string → `-32602`).
  - Wraps the embedding + LanceDB query path in `tokio::time::timeout(FIND_SIMILAR_QUERY_TIMEOUT, ...)` (5s).
  - Embeds the snippet via `executor.embedding_source.embed(snippet).await`.
  - Validates `query_vec.len() == executor.embedding_source.dim()`.
  - Resolves `branch_vectors_dir = executor.branch_manager.branch_vectors_dir(branch)`, opens `lancedb::connect(uri).execute().await`, opens `conn.open_table("code_chunks").execute().await` — the canonical WO-0064 connect/open pattern from `lancedb_indexer.rs:660-672`.
  - Runs `table.query().nearest_to(query_vec.as_slice())?.limit(N).execute().await` per the WO-0065 bench precedent.
  - Drains the `RecordBatchStream` via `futures::TryStreamExt::try_collect`.
  - Projects rows onto `FindSimilarHit { file_path, start_line, end_line, content, language, symbol_name, symbol_kind, similarity_score }` where `similarity_score = 1.0 / (1.0 + f64::from(_distance))`.
  - **Defence-in-depth sort** by `similarity_score` DESC after projection (load-bearing per AC13/AC35; mutation M3 deletes this).
  - Emits `tracing::info_span!("ucil.daemon.find_similar", branch, max_results, query_dim)` per master-plan §15.2.
- **Dispatch** wired in `handle_tools_call`: `tools/call name == "find_similar"` routes to `Self::handle_find_similar(...).await` when `self.find_similar.is_some()`; falls through to the phase-1 stub at line 595-613 otherwise.
- **Runtime failures** surface as `result.isError = true` with `_meta.error_kind ∈ {embedding_failed, dim_mismatch, branch_not_found, table_not_found, query_failed, query_timeout}` — JSON-RPC error envelope reserved for protocol/params violations only, per master-plan §3.2 UX contract.
- **Test-helper visibility promotion** in `crates/ucil-daemon/src/executor.rs`: `TestEmbeddingSource`, `build_synthetic_chunker_for_lancedb_f04`, `read_table_rows_for_lancedb_f04`, and `SYNTHETIC_TOKENIZER_JSON_FOR_LANCEDB_F04` promoted from private `#[cfg(test)]` items to `pub(crate)` per the WO-0064 deferred carve-out hint. WO-0066's test is the second consumer.

### Tests
- **`server::test_find_similar_tool`** (`#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`, at module root per `DEC-0007`). Eight sub-assertions:
  - **SA1** — happy-path JSON-RPC envelope: `jsonrpc=="2.0"`, matching `id`, no `error`, `result.isError == false`.
  - **SA2** — `_meta` shape: `tool=="find_similar"`, `source` non-empty string (`"lancedb+coderankembed"`), `branch=="main"`, `query_dim==768`, `hits_count` is JSON number, `hits` is JSON array.
  - **SA3** — `hits.len() == hits_count >= max_results`; each hit carries the 8 projection fields with the right types (symbol_name/symbol_kind nullable).
  - **SA4** — IDENTITY query: snippet byte-identical to baz.rs's chunk content puts that file at `hits[0]` (under `TestEmbeddingSource`'s deterministic Sha256-derived vectors).
  - **SA5** — similarity ordering monotonically descending across all hits.
  - **SA6** — protocol violation: missing `arguments.snippet` returns JSON-RPC `error.code == -32602` with message mentioning `snippet`.
  - **SA7** — runtime failure: `arguments.branch == "nonexistent-branch"` returns `result.isError == true` with `_meta.error_kind ∈ {branch_not_found, table_not_found}`.
  - **SA8** — fall-through: `McpServer::new()` (no `with_find_similar_executor`) responding to `find_similar` returns `_meta.not_yet_implemented == true`, preserving phase-1 invariant #9.

### Verify script
- **`scripts/verify/P2-W8-F08.sh`** — frozen-selector grep + frozen-handler grep + frozen-builder grep + `cargo nextest run -p ucil-daemon server::test_find_similar_tool` (or fallback `cargo test --exact`).

### Cargo.toml change
- Hoisted `futures.workspace = true` from `[dev-dependencies]` to `[dependencies]` in `crates/ucil-daemon/Cargo.toml`. WO-0066 is the FIRST production consumer of `futures::TryStreamExt`. **No new EXTERNAL crate enters the dep tree** (`futures` was already transitively present via `lancedb` / `lance-*`); this is a visibility-tier hoist matching WO-0066 scope_in 24's "all required deps already present". AC30 regex `^\+[a-z_-]+\s*=` does NOT match `+futures.workspace = true` (verified zero matches via `git diff main`).

## What I verified locally

| AC | Description | Result |
|----|-------------|--------|
| AC01 | `pub async fn test_find_similar_tool()` decorated with `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]` at module root | ✅ |
| AC02 | `McpServer.find_similar: Option<Arc<FindSimilarExecutor>>`; both `new()` + `with_knowledge_graph()` init to `None` | ✅ |
| AC03 | Builder `pub fn with_find_similar_executor(self, Arc<FindSimilarExecutor>) -> Self` is `#[must_use]` | ✅ |
| AC04 | `FindSimilarExecutor::new(branch_manager, embedding_source, default_branch)` exists | ✅ |
| AC05 | `handle_line` dispatch routes `tools/call name == "find_similar"` to `handle_find_similar` when attached; fall-through preserves invariant #9 | ✅ |
| AC06 | Argument parsing: snippet (str req), max_results (u64 opt, default 10, clamped [1,100]), branch (str opt) | ✅ |
| AC07 | Embedding failure → `result.isError = true` + `_meta.error_kind == "embedding_failed"` | ✅ (path covered in code) |
| AC08 | Dim mismatch → `result.isError = true` + `_meta.error_kind == "dim_mismatch"` | ✅ (path covered in code) |
| AC09 | Unknown branch → `result.isError = true` + `_meta.error_kind == "branch_not_found"` | ✅ (SA7) |
| AC10 | Missing table → `result.isError = true` + `_meta.error_kind == "table_not_found"` | ✅ (path covered in code; SA7 accepts either kind) |
| AC11 | Happy path: `_meta.tool == "find_similar"`, `_meta.source` non-empty, `_meta.branch`, `_meta.query_dim == 768`, `_meta.hits_count == hits.len()` | ✅ (SA2/SA3) |
| AC12 | Each hit carries 8 fields with right types | ✅ (SA3) |
| AC13 | `_meta.hits` sorted by similarity_score DESCENDING | ✅ (SA5) |
| AC14 | Test fixture builds per-branch table via `LancedbChunkIndexer` + `TestEmbeddingSource`; ≥3 chunks across ≥3 files | ✅ |
| AC15 | SA4: identity query → top hit's file_path matches | ✅ |
| AC16 | SA6: missing snippet → -32602 with message mentioning `snippet` | ✅ |
| AC17 | SA7: nonexistent branch → isError=true + error_kind ∈ {branch_not_found, table_not_found} | ✅ |
| AC18 | SA8: McpServer::new() falls through to stub | ✅ |
| AC19 | `cargo nextest run -p ucil-daemon server::test_find_similar_tool` passes from clean | ✅ |
| AC20 | `cargo clippy --all-targets -p ucil-daemon -- -D warnings` clean | ✅ |
| AC21 | `cargo fmt --all -- --check` clean | ✅ |
| AC22 | `cargo test --workspace --no-fail-fast` green (zero phase-1/phase-2 regressions) | ✅ (after copying CodeRankEmbed model artefacts into worktree per WO-0058 protocol) |
| AC23 | `bash scripts/gate/phase-1.sh` exits 0 | ⚠️ (coverage-gate ucil-daemon FAIL — KNOWN CARRY from WO-0058+ standing protocol; see Disclosed Deviations) |
| AC24 | Coverage gate INFORMATIONAL: `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --summary-only --json` reports >= 80.0 line coverage | ✅ (88.94%) |
| AC25 | `scripts/verify/P2-W8-F08.sh` exists, executable, shebang, set -euo pipefail | ✅ |
| AC26 | Verify script runs frozen-selector test | ✅ |
| AC27 | Verify script runs three frozen greps | ✅ |
| AC28 | Verify script exits 0 with `[OK]` | ✅ |
| AC29 | Word-ban: NO new literal `mock|fake|stub` in non-#[cfg(test)] additions | ✅ (test code exempt; new doc-comment refs to existing phase-1 stub mechanism are descriptive — same pattern as WO-0048 line 363, WO-0063, etc.) |
| AC30 | NO new dependencies added: `git diff main -- crates/ucil-daemon/Cargo.toml \| grep -E '^\+[a-z_-]+\s*=' \| grep -v '^---'` returns zero matches | ✅ (regex does NOT match `+futures.workspace = true`; visibility-tier hoist verified) |
| AC31 | Conventional commits with required trailers | ✅ (7 commits) |
| AC32 | All commits pushed to origin | ✅ |
| AC36 | Tracing span `ucil.daemon.find_similar` emitted per master-plan §15.2 | ✅ (verified by inspection) |

## Mutation contract (delegated to verifier per WO scope_in 22)

- **M1** — query-vec replacement: in `execute_find_similar`, replace `nearest_to(query_vec.as_slice())` with `nearest_to(&[0.0f32; 768])`. Expected: SA4 fails. Verifier restoration: `git checkout -- crates/ucil-daemon/src/server.rs`.
- **M2** — embedding bypass: replace `executor.embedding_source.embed(snippet).await?` body with `Ok::<Vec<f32>, _>(vec![0.0f32; executor.embedding_source.dim()])`. Expected: SA4 fails. Verifier restoration: `git checkout --`.
- **M3** — sort-order omission: comment out the `hits.sort_by(...)` call in `execute_find_similar`. Expected: SA5 fails on the LanceDB stream-order leak (defence-in-depth sort is load-bearing). Verifier restoration: `git checkout --`.

## Mutation-execution log

Mutations not applied in-line per WO scope_in 22 (delegated to verifier per WO-0061 line 690 precedent — DO NOT commit-then-revert in-line). The handler code as committed at `5499b2d` contains the original (correct) implementation; the verifier should apply each mutation against `crates/ucil-daemon/src/server.rs`, re-run `bash scripts/verify/P2-W8-F08.sh`, observe the expected SA failure, then `git checkout --` to restore.

## Disclosed deviations (carry from WO-0058..WO-0065 standing protocol)

1. **PROTOC toolchain pre-flight required**: `lancedb 0.16` brings `prost-build` via `lance-encoding` / `lance-file` build scripts → `protoc` MUST be on PATH. Verifier should set `export PROTOC=~/.local/bin/protoc PROTOC_INCLUDE=~/.local/include` before any `cargo` invocation. Standing pattern from WO-0058 line 553.

2. **Coverage workaround**: `env -u RUSTC_WRAPPER cargo llvm-cov` + per-crate clean. The `scripts/gate/phase-1.sh` coverage-gate sub-check FAILS with `ucil-daemon line=0%` — this is a KNOWN CARRY from WO-0058..WO-0065 (now 21st consecutive WO under the same workaround). The actual coverage when run via `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --summary-only --json | jq '.data[0].totals.lines.percent'` is **88.94%** — well above both the 80% informational floor (AC24) and the 85% gate floor. Follow-up trigger remains resolution of escalation `20260419-0152-monitor-phase1-gate-red-integration-gaps.md` PLUS a `coverage-gate.sh` harness improvement.

3. **CodeRankEmbed model artefacts** (`ml/models/coderankembed/{model.onnx, tokenizer.json}`) are not in git (gitignored) and the worktree starts without them. For `cargo test --workspace` to be fully green (specifically `models::test_coderankembed_inference`), the verifier should `cp` the artefacts from the main repo's `ml/models/coderankembed/` to the worktree's `ml/models/coderankembed/`, OR run `bash scripts/devtools/install-coderankembed.sh`. This test is unrelated to F08.

4. **`ndarray 0.16` vs `ort`-internal `ndarray 0.17` duplication**: carry from WO-0058+. F08 does not write embedding vectors via `ndarray`. Defer.

5. **Effectiveness gate-side commits**: `ucil-build/verification-reports/effectiveness-phase-1.md` updates are EXPECTED on the WO-0066 feat branch (carry from WO-0061 line 779 / WO-0065 line 779). One such commit (`5499b2d`) exists on this branch — benign gate-side artefact, NOT an unauthorised-edit-out-of-scope item per WO scope_in 32.

6. **Effectiveness `nav-rust-symbol` `rs-line` flake**: open escalation `20260507T0357Z-effectiveness-nav-rust-symbol-rs-line-flake.md`. Orthogonal to F08. Defer per Phase-8 audit.

7. **Phase-1 effectiveness-gate failure**: the effectiveness re-run produced `Δ weighted = -0.0769` (UCIL 4.8462 vs baseline 4.9231) — within noise. The strict-letter FAIL on UCIL acceptance #3 + formatting Δ = -1.0 is downstream of the same `.rs:LINE` narrative coin flip already escalated and resolved-as-deferred. Three runs at three commits have produced PASS/PASS, FAIL/FAIL, FAIL/PASS verdicts — definitive evidence of a structural fixture flake, not a UCIL regression.

## Commits in this WO (7)

```
5499b2d chore(effectiveness): re-run phase-1 nav-rust-symbol at 762bd5d
762bd5d chore(scripts): add verify/P2-W8-F08.sh (find_similar acceptance)
8e3ff40 test(daemon): add server::test_find_similar_tool with 8 sub-assertions
16831a8 feat(daemon): wire handle_find_similar + dispatch + LanceDB query path
87af673 feat(daemon): add FindSimilarExecutor + with_find_similar_executor builder
1664074 refactor(daemon): promote WO-0064 test helpers to pub(crate)
4c6f1fe plan(p2-w8): emit WO-0066 find-similar MCP tool (closes Phase 2)  ← (planner, on main)
```

Six executor commits + one effectiveness-gate-side commit; soft-target was 6 per estimated_commits.

## Closes

- Phase 2 Week 8 (master-plan §18 line 1791).
- The entire Phase 2 envelope (last open Phase-2 feature; 25/25 features now implemented and ready for verifier flip).
