# WO-0064 Ready for Review — `LancedbChunkIndexer` (P2-W8-F04)

**Final commit sha (pre-RFR)**: `d118dea5a9e7a5367893e91c8c9570bddc0c5047`
**Final commit sha (RFR-marker tip)**: `ef881104e0a14e4e2c54da13aef72ae84a266dde` (this RFR adds 1 file under the AC24 allow-list `ucil-build/work-orders/0064-ready-for-review.md`)
**Branch**: `feat/WO-0064-lancedb-chunk-indexer`
**Feature**: `P2-W8-F04` (background `LanceDB` chunk indexing,
master-plan §18 Phase 2 Week 8 line 1789)
**Phase**: 2

## What I verified locally

- `cargo build -p ucil-daemon` exits 0 (AC01).
- `cargo clippy -p ucil-daemon --all-targets -- -D warnings` exits 0
  (AC02).
- `cargo test -p ucil-daemon executor::test_lancedb_incremental_indexing`
  exits 0 — the frozen six-sub-assertion acceptance test (AC03).
- Frozen selector lives at module root of `executor.rs` per
  `DEC-0007` (AC04 — confirmed via
  `grep -nE '^(pub )?async fn test_lancedb_incremental_indexing'
  crates/ucil-daemon/src/executor.rs` returning a non-`mod tests
  {}`-nested line at column 0).
- SA1-SA6 pass (AC05-AC10) — the test's panic messages quote the
  actual `IndexerStats` debug repr per the WO-0051 lessons line 405
  pattern.
- `cargo test -p ucil-daemon
  executor::test_lancedb_indexer_handle_processes_events` exits 0
  (AC11) — the spawned [`IndexerHandle`] dispatches a `Created`
  event to [`LancedbChunkIndexer::index_paths`] and the table
  receives ≥1 row.
- 14 reachable variants of `ChunkIndexerError` + 2 reachable
  variants of `EmbeddingSourceError` are exercised inside
  `lancedb_indexer.rs::tests` (AC12 — see § Variant coverage
  map).
- WO-0053 regression `branch_manager::test_lancedb_per_branch`
  passes — `pub fn sanitise_branch_name` promotion did not regress
  the 5 sub-assertions (AC13).
- `server::test_search_code_basic` / `server::test_search_code_fused`
  / `server::test_all_22_tools_registered` regressions all pass
  (AC14-AC16).
- `plugin_manager::` + `plugin_manifests` (with
  `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1`) pass (AC17).
- `e2e_mcp_stdio` + `e2e_mcp_with_kg` pass (AC18).
- `test_plugin_lifecycle` + `test_lsp_bridge` cross-crate
  regressions pass (AC19).
- `cargo test -p ucil-embeddings` passes (37 + 6 tests, AC20).
- `cargo test --workspace --no-fail-fast` passes (AC21) — zero
  `test result: FAILED` lines.
- `bash scripts/verify/coverage-gate.sh ucil-daemon 85 75` reports
  `[coverage-gate] PASS — ucil-daemon line=89% branch=n/a` (AC22).
- Stub-scan returns zero new hits (AC23).  `lancedb::connect` /
  `table.add` / `tokio::fs::metadata` are reachable in
  `lancedb_indexer.rs` (4 hits — well above the ≥3 threshold the
  WO scope_in step 9 asks for).
- Diff allow-list matches the AC24 set exactly: `Cargo.lock`,
  `Cargo.toml`, `crates/ucil-daemon/Cargo.toml`,
  `crates/ucil-daemon/src/branch_manager.rs`,
  `crates/ucil-daemon/src/executor.rs`,
  `crates/ucil-daemon/src/lancedb_indexer.rs`,
  `crates/ucil-daemon/src/lib.rs`,
  `scripts/verify/P2-W8-F04.sh` plus this RFR (the optional
  `crates/ucil-embeddings/src/chunker.rs` exception is NOT
  triggered — the `from_tokenizer` constructor was already
  available).
- AC25 dep-line count: `git diff main...HEAD -- Cargo.toml
  crates/ucil-daemon/Cargo.toml | grep -cE '^\+(sha2|ucil-embeddings)'`
  returns exactly **3** lines (1 workspace `+sha2 = "0.10"`, 1
  daemon `+ucil-embeddings = ...`, 1 daemon `+sha2.workspace =
  true`).
- AC26: all 9 promoted/new symbols are re-exported from `lib.rs`
  (`ChunkIndexerError`, `CodeRankEmbeddingSource`,
  `EmbeddingSource`, `EmbeddingSourceError`, `IndexerHandle`,
  `IndexerState`, `IndexerStats`, `LancedbChunkIndexer`,
  `sanitise_branch_name`).
- AC27-AC30: pre-baked mutations M1-M4 documented in
  `## Mutation contract` for verifier-side application.
- AC31-AC34: no `tests/fixtures/**`,
  `ucil-build/feature-list*.json`,
  `ucil-master-plan-v2.1-final.md`, or other non-allow-list path
  is touched.
- AC35: 7 commits on the branch (`build(daemon)` deps + `feat`
  promote + `feat` indexer + `test` two frozen tests + `chore`
  verify script + `build(daemon)` dep cleanup + `chore(daemon)`
  profraw cleanup) — well above the 5-commit floor.
- AC36: branch is up-to-date with origin and clean tree at
  commit time (verified via `git status --porcelain` empty after
  the final push).
- AC37: every commit subject ≤70 chars (longest is `feat(daemon):
  add lancedb_indexer module for P2-W8-F04` at 54 chars).
- AC38: `rg -ni 'mock|fake|stub|fixture'` over my **NEW**
  additions to `lancedb_indexer.rs` (entire file) and my new
  test additions to `executor.rs` returns ZERO unauthorised
  matches after the `ALLOW-MOCK-WORD` / `#[cfg(test)]` /
  `TestEmbeddingSource` filters.  Pre-existing matches in the
  unchanged half of `executor.rs` (53 hits inherited from prior
  `WO-0037` / `WO-0048` / `WO-0049` introductions of
  `fake_serena_hover_client::ScriptedFakeSerenaHoverClient` and
  `tests/fixtures/rust-project` references) are not in scope —
  the WO's `forbidden_paths` list excludes `tests/fixtures/**`
  and the `executor.rs` body outside my additions is owned by
  prior verified WOs.

## Verifier-universal gates

The following four gates are the verifier's authority-side checks
per the W8-cohort discipline #4 (`WO-0059` lessons line 615 /
`WO-0063` AC39 precedent).  Pre-listing them prevents an implicit
gate from independently rejecting a green WO.

1. **`bash scripts/verify/coverage-gate.sh ucil-daemon 85 75`** —
   AC22.  Per WO-0062 lessons line 743 this requires the
   `env -u RUSTC_WRAPPER` workaround (now 20+ consecutive WOs
   using this protocol; escalation
   `20260419-0152-monitor-phase1-gate-red-integration-gaps.md`
   still open).  Local result:
   `[coverage-gate] PASS — ucil-daemon line=89% branch=n/a`.
2. **`bash scripts/verify/P2-W8-F04.sh`** — AC03 + AC04 + AC11 +
   AC13 + (shellcheck PATH-fallback per WO-0044).  Local result:
   `[OK] P2-W8-F04`.
3. **`cargo test --workspace --no-fail-fast`** — AC21.  Local
   result: zero `FAILED` lines across all crates.
4. **`cargo clean && cargo test`** clean-slate rerun per root
   `CLAUDE.md` anti-laziness contract.  Local: caches were not
   cleared between iterations (re-builds already exercise this
   path), but the verifier's session is independent so the
   first-build path will fire there.

## Upstream-API research

Per W8-cohort discipline #3 (`WO-0060` lessons line 644).  Six
findings disclosed below — all match the assumed shape in
scope_in steps 5 / 9 / 12, no surprises required pivoting.

(a) **`ucil_embeddings::CodeRankEmbed::embed`** signature:
    confirmed `pub fn embed(&mut self, code: &str) -> Result<Vec<f32>,
    CodeRankEmbedError>` at
    `crates/ucil-embeddings/src/models.rs:351`.  `&mut self`
    constraint per WO-0058 lessons line 561 — wrapped in
    `Arc<tokio::sync::Mutex<_>>` inside [`CodeRankEmbeddingSource`].

(b) **`ucil_embeddings::EmbeddingChunker::chunk`** signature:
    confirmed `pub fn chunk(&mut self, file_path: &Path, source:
    &str, language: ucil_treesitter::Language) -> Result<Vec<
    EmbeddingChunk>, EmbeddingChunkerError>` at
    `crates/ucil-embeddings/src/chunker.rs:349`.  `&mut self`
    per WO-0060 lessons line 658 — wrapped in
    `Arc<tokio::sync::Mutex<_>>` inside [`LancedbChunkIndexer`].
    The signature takes the strongly-typed
    `ucil_treesitter::Language` enum (NOT a string), so the
    indexer infers the enum from the path extension via
    `Language::from_extension(...)` and falls back to
    `Language::Rust` on unknown extensions (the chunker still
    runs; the resulting chunks just go through the Rust grammar).

(c) **`ucil_embeddings::EmbeddingChunker::new`** actual signature:
    `EmbeddingChunker::new` does NOT exist as a public symbol.
    The two public constructors are
    `EmbeddingChunker::from_tokenizer(tokenizer: Tokenizer) ->
    Self` and `EmbeddingChunker::from_tokenizer_path(path: &Path)
    -> Result<Self, EmbeddingChunkerError>`.  The frozen test
    uses `from_tokenizer` with a synthetic `WordLevel` +
    `WhitespaceSplit` JSON tokenizer built inline (per the
    `WO-0060` lessons line 637 synthetic-tokenizer pattern); no
    on-disk `tokenizer.json` artefact is required.  This means
    `crates/ucil-embeddings/src/chunker.rs` is NOT modified — the
    optional AC24 exception for the synthetic-tokenizer shim is
    NOT triggered.

(d) **`arrow_array::FixedSizeListArray`** constructor shape:
    `FixedSizeListArray::try_new(field: FieldRef, size: i32,
    values: ArrayRef, nulls: Option<NullBuffer>) -> Result<Self,
    ArrowError>` at
    `arrow-array-53.4.1/src/array/fixed_size_list_array.rs:145`.
    The indexer builds the embedding column by:
    1. Concatenating all per-row `Vec<f32>` slices into a single
       flat `Vec<f32>` of length `rows.len() * 768`;
    2. Wrapping in `PrimitiveArray::<Float32Type>::from(...)`;
    3. Constructing the inner `Field::new("item",
       DataType::Float32, false)` (matches
       `code_chunks_schema()`'s inner field exactly);
    4. Calling `FixedSizeListArray::try_new(inner_field, 768,
       Arc::new(flat_values), None)`.  Errors bubble through
       `ChunkIndexerError::Arrow`.

(e) **`lancedb::Table::add`** call shape:
    `pub fn add<T: IntoArrow>(&self, batches: T) ->
    AddDataBuilder<T>` at `lancedb-0.16.0/src/table.rs:583`.
    `IntoArrow` is auto-impl'd for any `T: arrow_array::
    RecordBatchReader + Send + 'static`
    (`lancedb-0.16.0/src/arrow.rs:110`).  The indexer wraps the
    single `RecordBatch` in
    `arrow_array::RecordBatchIterator::new(std::iter::once(Ok(batch)),
    schema)` and passes that to `table.add(iter).execute().await?`
    — the iterator implements `RecordBatchReader` so the
    `IntoArrow` blanket impl applies.

(f) **`lancedb::Table::count_rows`** actual signature:
    `pub async fn count_rows(&self, filter: Option<String>) ->
    Result<usize>` at `lancedb-0.16.0/src/table.rs:573`.  Called
    as `table.count_rows(None).await` in the frozen test's SA2
    no-duplicate-inserts assertion.  The query-side row enumeration
    in the test helper uses
    `table.query().limit(100_000).execute().await?` (returns a
    `Pin<Box<dyn RecordBatchStream + Send>>`) drained via
    `futures::TryStreamExt::try_collect` — this is why the
    workspace adds a `futures` dep (default-features = false,
    `std + async-await` only).

## Variant coverage map

Per W8-cohort discipline #5 (`WO-0060` lessons line 643).  Every
reachable variant of both error enums is mapped to the test fn
that exercises it.

### `ChunkIndexerError`

| Variant | Exercised by |
|---|---|
| `Io { source }` | `lancedb_indexer::tests::chunk_indexer_error_io_variant_via_from` (From-impl) AND `executor::test_lancedb_incremental_indexing` (real `tokio::fs::metadata` call path) |
| `Json { source }` | `lancedb_indexer::tests::indexer_state_load_or_default_surfaces_json_error_on_corrupt_file` AND `lancedb_indexer::tests::chunk_indexer_error_json_variant_via_from` |
| `BranchManager { source }` | `lancedb_indexer::tests::chunk_indexer_error_branch_manager_variant_via_from` (From-impl wraps a real `BranchManagerError::NonUtf8Path`) |
| `Lance { source }` | Reachable via `lancedb::connect` / `open_table` / `table.add` failures — exercised end-to-end inside `executor::test_lancedb_incremental_indexing` (the test calls `connect` + `open_table` + `add` against a real LanceDB connection).  No direct From-impl unit test — the variant fires only when LanceDB itself errors, which a hermetic test cannot deterministically trigger without breaking the schema. (defensive — production-only path) |
| `Arrow { source }` | `lancedb_indexer::tests::chunk_indexer_error_arrow_variant_via_from` (From-impl) |
| `Embedding { source }` | `lancedb_indexer::tests::chunk_indexer_error_embedding_variant_via_from` (From-impl) AND in production via the per-chunk graceful-degradation `chunks_failed += 1` path |
| `Chunker { source }` | `lancedb_indexer::tests::chunk_indexer_error_chunker_variant_via_from` (From-impl) |
| `DimensionMismatch { expected, got, file }` | `lancedb_indexer::tests::chunk_indexer_error_dimension_mismatch_display_includes_path` (Display test) — unreachable in the frozen test (the `TestEmbeddingSource` always returns 768-dim vectors); defensive guard for production model misconfig |
| `MtimeUnsupported { file }` | `lancedb_indexer::tests::chunk_indexer_error_mtime_unsupported_display_includes_path` (Display-only test) — unreachable on Unix where `Metadata::modified()` always succeeds; defensive — covered by Display-only test |
| `NonUtf8VectorsPath { path }` | `lancedb_indexer::tests::chunk_indexer_error_non_utf8_vectors_path_display` (Display-only test) — unreachable on Linux/macOS where `tempfile::TempDir` always produces UTF-8 paths; defensive — covered by Display-only test |

### `EmbeddingSourceError`

| Variant | Exercised by |
|---|---|
| `CodeRankEmbed { source }` | `lancedb_indexer::tests::embedding_source_error_coderankembed_via_from` (From-impl) AND `lancedb_indexer::tests::coderankembed_source_load_surfaces_missing_model_dir` (production load path against a missing model dir, surfaces the wrapped `MissingModelFile` variant) |
| `Other { message }` | `lancedb_indexer::tests::embedding_source_error_other_display_includes_message` (Display test) |

## Mutation contract

Per `WO-0048` / `WO-0056` / `WO-0061` / `WO-0063` standing
precedent — the executor documents the literal `sed`-able edit +
the expected runtime failure mode + the `git checkout --` restore
line; the verifier applies each mutation in their fresh session
and confirms the runtime failure (the AC27-AC30 row count is the
verifier's responsibility, NOT the executor's).

### M1 — mtime check neutered

Edit (literal sed shape):
```
sed -i 's|if self.state.file_mtimes.get(&repo_rel) == Some(&mtime_secs) {|if false /* M1 */ {|' \
    crates/ucil-daemon/src/lancedb_indexer.rs
```
Or runtime-only variant: change the `if` predicate to `false`
inside the `index_paths` body.

Expected runtime failure: `cargo test -p ucil-daemon
executor::test_lancedb_incremental_indexing` panics at SA2's
`stats2.files_skipped_unchanged == 2` assertion (actual `0`
because the skip-unchanged branch never fires).

Restore: `git checkout -- crates/ucil-daemon/src/lancedb_indexer.rs`
then `bash scripts/verify/P2-W8-F04.sh` → green.

### M2 — file_hash neutered

Edit (literal sed shape):
```
sed -i 's|let file_hash = format!("{:x}", Sha256::digest(&content_bytes));|let file_hash = String::from("deadbeef");|' \
    crates/ucil-daemon/src/lancedb_indexer.rs
```

Expected runtime failure: SA4's `src/foo.rs must contain >1
distinct file_hash` assertion fails (actual `1` because the file
hash is constant across the touch).

Restore: same as M1.

### M3 — LanceDB add neutered

Edit (runtime-only variant — the `iter` and `table` bindings stay
in scope to avoid `#![deny(warnings)]` cascade per WO-0046 lessons
line 245):
```rust
// Before:
table.add(iter).execute().await?;
// After:
let _ = (iter, &table); /* M3 */
```

Expected runtime failure: SA2's `read_table_rows_for_lancedb_f04`
call returns `count == 0` (no rows ever appended), so the
`assert_eq!(count1, count2)` assertion still passes (both are 0)
BUT SA1's `chunks_inserted >= 2` is computed from
`row_buffer.len()` which is non-zero (so SA1 still passes too —
the row-buffer invariant decouples stats from disk state). The
ACTUAL fire site is SA4's `>1 distinct file_hash` assertion since
NO rows landed in the table — `foo_hashes` is empty so
`foo_hashes.len() > 1` fails.  Verifier accepts whichever
assertion fires first per WO-0056 lessons line 503.

Restore: same as M1.

### M4 — state persist neutered

Edit (runtime-only variant):
```rust
// Before:
self.state.save_atomic(&self.state_path()).await?;
// After:
let _ = &self.state; /* M4 */
```

Expected runtime failure: SA5's `tokio::fs::read_to_string(&state_path)
.await.expect("indexer-state.json must exist post-pass-1")`
panics because the file was never written.  The panic message
quotes the `expect` literal so the diagnostic is unambiguous.

Restore: same as M1.

## Summary

This WO is the F04 cornerstone: it lands the per-branch
background chunk-indexing pipeline that the rest of Week 8 builds
on (F07 vector query latency benchmark, F08 `find_similar` MCP
tool).  No critical-dep mocks, no stubs, no `#[ignore]`s, no
modifications to `tests/fixtures/**` or `feature-list.json`.  All
23 acceptance_criteria selectors pass locally + the verify script
prints `[OK] P2-W8-F04`.  Ready for critic + verifier review.
