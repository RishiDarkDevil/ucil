//! Vector query latency benchmark — `P2-W8-F07` / `WO-0065`.
//!
//! Master-plan §4.2 lines 290-303 (Group 2 — Search; `CodeRankEmbed`
//! default model and CPU throughput target).  Master-plan §12.2
//! lines 1321-1346 (LanceDB `code_chunks` 12-column schema; the
//! 768-d `FixedSizeList<Float32, 768>` embedding column drives ANN
//! query latency).  Master-plan §18 Phase 2 Week 8 line 1789
//! (verbatim: "Benchmark: embedding throughput, query latency,
//! recall@10").  This file lands the query-latency leg.
//!
//! `DEC-0016` §Closed (lancedb-per-branch closure: merge commit
//! `57e50ab` brings `branch_manager.rs` to `main`, unblocking F04 /
//! F07 / F08).  `WO-0061` line 685 (canonical bench shape: criterion
//! 0.5 with `default-features = false`, `[[bench]]` table with
//! `harness = false`, frozen-bench-id load-bearing for criterion's
//! output path, `.expect()`-in-benches carve-out).
//!
//! # Bench shape
//!
//! - One-time setup (BEFORE criterion's measurement loop):
//!   * [`tempfile::tempdir`] for an isolated `LanceDB` connection
//!     root — every bench run starts from a clean dataset, so the
//!     measurement is reproducible across machines.
//!   * `lancedb::connect(path).execute().await` to open the
//!     connection (mirrors `branch_manager.rs:449`).
//!   * Build a SIMPLIFIED 2-column Arrow schema (`id: Utf8`,
//!     `embedding: FixedSizeList<Float32, 768>`).  The production
//!     12-column schema (`code_chunks_schema()` at
//!     `crates/ucil-daemon/src/branch_manager.rs:318`) carries
//!     `file_path` / `start_line` / `end_line` / `content` /
//!     `language` / `symbol_name` / `symbol_kind` / `token_count` /
//!     `file_hash` / `indexed_at` for the production indexer
//!     pipeline.  None of those columns participate in the ANN
//!     index — `IvfHnswPq` operates ONLY on the `embedding` column —
//!     so the 10-column-trimmed bench schema isolates the query
//!     latency we are measuring without coupling to the production
//!     indexer's columnar overhead.
//!   * Populate [`CORPUS_SIZE`] (= `2000`) rows with deterministic
//!     [`rand::rngs::StdRng`]-seeded random 768-d `Float32` vectors
//!     (corpus seed `0x_C0DE_BABE`; query seed `0x_FACE_FEED`).
//!     Different seeds for corpus vs queries so the queries are not
//!     trivially in the corpus — keeps the ANN search realistic.
//!   * `Table::create_index(&["embedding"], lancedb::index::Index::
//!     IvfHnswPq(...))` with `num_partitions = ceil(sqrt(2000)) = 45`
//!     and HNSW defaults (`m = 20`, `ef_construction = 300`,
//!     `num_sub_vectors = 768 / 16 = 48`).
//!
//! - Per-iter (criterion's measurement loop body):
//!   * Pick a query vector via round-robin index `i % QUERY_COUNT`
//!     so the engine cannot trivially cache a single result.
//!   * `table.query().nearest_to(&query_vec[..]).unwrap().limit(10)
//!     .execute().await.unwrap()` returns the top-10 nearest rows
//!     as a `SendableRecordBatchStream`.
//!   * `criterion::black_box` the resulting stream so the optimiser
//!     cannot eliminate the call.
//!
//! # Frozen identifiers (load-bearing — DO NOT rename)
//!
//! - [`CORPUS_SIZE`], [`QUERY_COUNT`], [`EMBEDDING_DIM`] — the
//!   `WO-0061` mutation pattern stashes these constants.  The
//!   verifier's M2 mutation (shrink `QUERY_COUNT` to 1) corners this.
//! - The criterion `bench_function` literal `"vector_query_p95_warm"`
//!   is load-bearing because criterion writes
//!   `target/criterion/vector_query_p95_warm/vector_query_p95_warm/
//!   new/sample.json`, which is the parser-script's read path.
//!   `scripts/verify/P2-W8-F07.sh` greps for the literal at the
//!   `group.bench_function(...)` call site.  Renaming it breaks
//!   BOTH consumers.
//!
//! # Pre-baked mutation contract
//!
//! Three mutations delegated to verifier per `WO-0061` line 690 (DO
//! NOT commit-then-revert in-line):
//!
//! - **M1**: replace the per-iter `Table::query().nearest_to(...).
//!   execute()` call with `let _ = black_box(query_vec);` (drops the
//!   actual `LanceDB` call).  Expected: wall-time floor breached
//!   (`mean_ns_per_iter < 100_000`).  Restore: `git checkout --`.
//! - **M2**: `pub const QUERY_COUNT: usize = 20;` → `... = 1;`.
//!   Expected: round-robin always picks index 0; per-iter time
//!   drops below 100 µs floor.  Restore: `git checkout --`.
//! - **M3**: edit `scripts/bench-vector-query.sh` p95-floor branch
//!   from `< 100` to `< 1000000`.  Expected: bench script passes
//!   (false green); `scripts/verify/P2-W8-F07.sh` INDEPENDENTLY
//!   re-asserts `< 100` and is the AUTHORITATIVE asserter.  Restore:
//!   `git checkout --`.
//!
//! # Async runtime contract
//!
//! `lancedb` futures need a Tokio runtime.  Criterion's `b.iter`
//! body is sync, so the bench builds a single
//! [`tokio::runtime::Runtime`] in setup and uses
//! [`tokio::runtime::Runtime::block_on`] for every async call —
//! both setup (corpus build, index create) and per-iter (query).
//! This is the canonical pattern documented in `lancedb 0.16`'s
//! own `Table::create_index` doctest (`src/table.rs:715`).

use std::sync::Arc;
use std::time::Duration;

use arrow_array::types::Float32Type;
use arrow_array::{FixedSizeListArray, RecordBatch, RecordBatchIterator, StringArray};
use arrow_schema::{DataType, Field, Schema};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lancedb::index::vector::IvfHnswPqIndexBuilder;
use lancedb::index::Index;
use lancedb::query::{ExecutableQuery, QueryBase};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::runtime::Runtime;

/// Number of rows in the synthetic corpus inserted into LanceDB.
///
/// `2000` is sized to exercise the IVF/HNSW index without making
/// setup wall-time dominate the bench run (master-plan §12.2 line
/// 1341 documents "1M 128-dim vectors in 33ms" as the upper-bound
/// shape; 2K rows is well within the index's reasonable training
/// envelope).
///
/// **Load-bearing** per the `WO-0061` mutation pattern: the
/// verifier's M2 mutation stashes [`QUERY_COUNT`], not this constant,
/// but the same load-bearing-identifier contract applies — renaming
/// to `N_ROWS` would break the verifier's frozen-grep selectors.
pub const CORPUS_SIZE: usize = 2000;

/// Number of distinct query vectors used in the per-iter loop.
///
/// A round-robin over [`QUERY_COUNT`] distinct vectors keeps the
/// engine from caching a single result.  Setting this to `1` is the
/// pre-baked **M2** mutation (verifier-applied): the per-iter call
/// always picks index `0`, the engine caches the answer, and the
/// per-iter wall-time drops below the 100 µs floor.
///
/// **Load-bearing** — DO NOT rename or change the literal `20`
/// outside of the M2 mutation.
pub const QUERY_COUNT: usize = 20;

/// Embedding dimension of the synthetic corpus.
///
/// `768` matches `CodeRankEmbed`'s default (master-plan §12.2 line
/// 1332 documents the production `FixedSizeList<Float32, 768>`
/// embedding column).  The bench's simplified 2-column schema
/// re-uses the same dimension so the IVF/HNSW index sees the
/// production-shape input vector.
///
/// **Load-bearing** for the WO-0061 mutation pattern.
pub const EMBEDDING_DIM: usize = 768;

/// Build a 2-column Arrow [`Schema`] mirroring the production
/// 12-column `code_chunks` schema's load-bearing columns: an
/// `id: Utf8` row key and the 768-d `embedding:
/// FixedSizeList<Float32, 768>` column that the IVF/HNSW index
/// targets.  See file-level rustdoc for the simplification rationale.
fn build_bench_schema() -> Arc<Schema> {
    let inner = Arc::new(Field::new("item", DataType::Float32, false));
    let dim_i32 = i32::try_from(EMBEDDING_DIM).expect("EMBEDDING_DIM 768 fits in i32");
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("embedding", DataType::FixedSizeList(inner, dim_i32), false),
    ]))
}

/// Generate [`CORPUS_SIZE`] × [`EMBEDDING_DIM`] random `f32`s with
/// a deterministic seed (`0x_C0DE_BABE`).  The values are sampled
/// uniformly in `[-1.0, 1.0]` — same range as the post-normalisation
/// output of `CodeRankEmbed::embed` (per the production model's
/// final L2-normalised pooling layer).
fn generate_corpus_vectors() -> Vec<Vec<f32>> {
    let mut rng = StdRng::seed_from_u64(0x_C0DE_BABE);
    (0..CORPUS_SIZE)
        .map(|_| {
            (0..EMBEDDING_DIM)
                .map(|_| rng.gen_range(-1.0..1.0))
                .collect()
        })
        .collect()
}

/// Generate [`QUERY_COUNT`] × [`EMBEDDING_DIM`] random `f32`s with
/// a SECOND deterministic seed (`0x_FACE_FEED`) so the query data
/// is decorrelated from the corpus data — keeps the ANN search
/// realistic (a query trivially in the corpus collapses to a
/// row-id lookup).
fn generate_query_vectors() -> Vec<Vec<f32>> {
    let mut rng = StdRng::seed_from_u64(0x_FACE_FEED);
    (0..QUERY_COUNT)
        .map(|_| {
            (0..EMBEDDING_DIM)
                .map(|_| rng.gen_range(-1.0..1.0))
                .collect()
        })
        .collect()
}

/// Build a [`RecordBatch`] of `CORPUS_SIZE` rows from the corpus
/// vectors.  The `id` column is the row index as a string; the
/// `embedding` column is a [`FixedSizeListArray`] with a flat
/// `Float32` value buffer of length `rows × dim`.
fn build_corpus_batch(corpus: &[Vec<f32>], schema: Arc<Schema>) -> RecordBatch {
    let ids: StringArray = (0..corpus.len())
        .map(|i| format!("row-{i}"))
        .collect::<Vec<_>>()
        .into();

    let flat_floats: Vec<f32> = corpus.iter().flat_map(|v| v.iter().copied()).collect();
    let inner = Arc::new(Field::new("item", DataType::Float32, false));
    let dim_i32 = i32::try_from(EMBEDDING_DIM).expect("EMBEDDING_DIM 768 fits in i32");
    let flat = arrow_array::PrimitiveArray::<Float32Type>::from(flat_floats);
    let embedding = FixedSizeListArray::try_new(inner, dim_i32, Arc::new(flat), None)
        .expect("FixedSizeListArray::try_new on flat corpus buffer");

    RecordBatch::try_new(schema, vec![Arc::new(ids), Arc::new(embedding)])
        .expect("RecordBatch::try_new on bench corpus")
}

/// Compute `ceil(sqrt(rows))` as a `u32`.  Master-plan §12.2 line
/// 1341 ("1M 128-dim vectors in 33ms") implicit IVF partition
/// sizing: `sqrt(N)` partitions is the canonical Lance/Faiss
/// heuristic.  For `rows = 2000` this is `45` (`44.72 → ceil = 45`).
fn ceil_sqrt_u32(rows: usize) -> u32 {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let sqrt_f = (rows as f64).sqrt();
    let floor = sqrt_f as u32;
    if (f64::from(floor) - sqrt_f).abs() < f64::EPSILON {
        floor
    } else {
        floor.saturating_add(1)
    }
}

/// The criterion bench function — measures p95 warm-cache query
/// latency over the synthetic IVF/HNSW-indexed corpus.
///
/// Setup runs ONCE before the measurement loop:
///   1. open a tempdir-rooted `LanceDB` connection,
///   2. populate `CORPUS_SIZE` rows of 768-d random vectors,
///   3. create an `IvfHnswPq` index on the `embedding` column.
///
/// Per-iter:
///   - round-robin pick a query vector,
///   - `table.query().nearest_to(...).limit(10).execute().await`,
///   - `black_box` the resulting stream.
fn bench_vector_query_p95(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio Runtime::new");

    let tmp = tempfile::tempdir().expect("tempfile::tempdir for LanceDB connection root");
    let uri = tmp
        .path()
        .to_str()
        .expect("tempdir path is valid UTF-8")
        .to_owned();

    let schema = build_bench_schema();
    let corpus = generate_corpus_vectors();
    let queries = generate_query_vectors();

    let table = runtime.block_on(async {
        let conn = lancedb::connect(&uri)
            .execute()
            .await
            .expect("lancedb::connect on tempdir");

        let table = conn
            .create_empty_table("vector_query_corpus", schema.clone())
            .execute()
            .await
            .expect("Connection::create_empty_table on synthetic schema");

        let batch = build_corpus_batch(&corpus, schema.clone());
        let iter = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema.clone());
        table
            .add(iter)
            .execute()
            .await
            .expect("Table::add on synthetic corpus");

        let index = Index::IvfHnswPq(
            IvfHnswPqIndexBuilder::default().num_partitions(ceil_sqrt_u32(CORPUS_SIZE)),
        );
        table
            .create_index(&["embedding"], index)
            .execute()
            .await
            .expect("Table::create_index Index::IvfHnswPq on `embedding`");

        table
    });

    let mut group = c.benchmark_group("vector_query_p95_warm");
    group.bench_function("vector_query_p95_warm", |b| {
        let mut iter_idx: usize = 0;
        b.iter(|| {
            let query_vec: &[f32] = queries[iter_idx % QUERY_COUNT].as_slice();
            iter_idx = iter_idx.wrapping_add(1);
            runtime.block_on(async {
                let stream = table
                    .query()
                    .nearest_to(query_vec)
                    .expect("VectorQuery::nearest_to on synthetic 768-d query vector")
                    .limit(10)
                    .execute()
                    .await
                    .expect("ExecutableQuery::execute on warm IVF/HNSW index");
                let _ = black_box(stream);
            });
        });
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(100)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(20));
    targets = bench_vector_query_p95
}
criterion_main!(benches);
