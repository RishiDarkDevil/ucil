# WO-0062 — Ready for review

**Feature:** P2-W8-F03 — Qwen3-Embedding GPU upgrade config gate
**Branch:** `feat/WO-0062-ucil-embeddings-qwen3-config-gate`
**Final commit (pre-marker):** `0e9c20086a8f25200e9840e2c59a744d0868607c`

## What I verified locally

- **AC01** — `cargo build -p ucil-embeddings` exits 0.
- **AC02** — `cargo clippy -p ucil-embeddings --all-targets -- -D warnings` exits 0.
- **AC03** — `cargo test -p ucil-embeddings models::test_qwen3_config_gate -- --nocapture` exits 0; output shows `test result: ok. 1 passed`.
- **AC04** — Frozen selector `fn test_qwen3_config_gate` lives at column 0 of `crates/ucil-embeddings/src/models.rs:755` (module root, DEC-0007).
- **AC05** — `crates/ucil-embeddings/src/config.rs` exists.
- **AC06** — `pub struct VectorStoreConfig` defined at `config.rs:182`.
- **AC07** — `EmbeddingBackend::CodeRankEmbed` / `Qwen3` variants + match arms ≥4 hits in `config.rs`.
- **AC08** — `pub fn validate_matryoshka_dimension` defined at `models.rs:570`.
- **AC09** — `pub const MIN_MATRYOSHKA_DIM: usize = 32` at `models.rs:439` and `pub const MAX_MATRYOSHKA_DIM: usize = 7168` at `models.rs:445`.
- **AC10** — `pub fn detect_gpu_execution_provider` defined at `models.rs:608`.
- **AC11** — `pub struct Qwen3Embedding` defined at `models.rs:645`.
- **AC12** — `Qwen3EmbeddingError` enum has 6 variants: NoGpuDetected, DimensionOutOfRange, MissingModelFile, Onnx, Tokenizer, Io.
- **AC13** — `ConfigError` enum has 4 variants: Toml, UnknownEmbeddingModel, UnknownBackend, Validation.
- **AC14** — `lib.rs` has both `pub use config::{...}` and `pub use models::{... Qwen3Embedding ...}` re-exports.
- **AC15** — `pub mod config;` declared at `lib.rs:56`.
- **AC16** — `crates/ucil-embeddings/Cargo.toml` declares `serde.workspace = true` + `toml.workspace = true`.
- **AC17** — F02 frozen acceptance regression: `cargo test -p ucil-embeddings models::test_coderankembed_inference` passes (ran installer + test).
- **AC18** — F05 frozen acceptance regression: `cargo test -p ucil-embeddings chunker::test_embedding_chunker_real_fixture` passes.
- **AC19** — F01 frozen acceptance regression: `cargo test -p ucil-embeddings onnx_inference::test_onnx_session_loads_minimal_model` passes.
- **AC20** — `cargo build -p ucil-embeddings --benches` exits 0.
- **AC21** — `cargo test --workspace --no-fail-fast` — all crate test suites pass; zero failures.
- **AC22** — `scripts/verify/P2-W8-F03.sh` is executable; `set -euo pipefail` in line 3 (within `head -3`).
- **AC23** — `bash scripts/verify/P2-W8-F03.sh` exits 0; stdout contains `[OK] P2-W8-F03 qwen3 config gate verified`.
- **AC24** — `bash scripts/verify/coverage-gate.sh ucil-embeddings 85 75` exits 0; reported line=89% ≥ 85.
- **AC25** — Stub-scan in NEW additions: 0 hits for `todo!()` / `unimplemented!()` / `TODO` / `FIXME`.
- **AC26** — Mock-scan additive lines: 0 hits for `mock|fake|stub|fixture` (case-insensitive).
- **AC27** — Allow-list path verification: `git diff --name-only main...HEAD` lists only the 6 expected paths (Cargo.lock, crates/ucil-embeddings/Cargo.toml, config.rs, lib.rs, models.rs, scripts/verify/P2-W8-F03.sh) — plus this RFR marker file when committed.
- **AC28** — `tests/fixtures/**` not modified.
- **AC29** — `feature-list.json` / `feature-list.schema.json` not modified.
- **AC30** — `ucil-master-plan-v2.1-final.md` not modified.
- **AC31** — All forbidden-paths sub-trees untouched (verified via `git diff --name-only main...HEAD`).
- **AC32** — `feat/WO-0053-lancedb-per-branch` not touched / not merged / not cherry-picked.
- **AC33** — Pre-baked mutation #1 (`validate_matryoshka_dimension` body neutered to `Ok(d)`): verified locally — SA5 fails with operator-readable assertion message; restored, green.
- **AC34** — Pre-baked mutation #2 (`detect_gpu_execution_provider` neutered to `Ok(GpuKind::Cuda)`): verified locally — SA1 fails on the `Err(NoGpuDetected)` match arm; restored, green.
- **AC35** — Pre-baked mutation #3 (`EmbeddingBackend::from_config_str` neutered to `Ok(CodeRankEmbed)`): verified locally — SA7 fails on the Qwen3-vs-CodeRankEmbed mismatch; restored, green.
- **AC36** — Commit count: 6 commits on the feature branch (≥5 minimum, target 6 hit).
- **AC37** — Every commit subject ≤70 characters (max observed = 64).
- **AC38** — Branch up-to-date with origin; working tree clean after marker commit.
- **AC39** — Cumulative re-export discipline: `lib.rs` re-exports all 10 new public symbols (`Qwen3Embedding`, `Qwen3EmbeddingError`, `GpuKind`, `MIN_MATRYOSHKA_DIM`, `MAX_MATRYOSHKA_DIM`, `validate_matryoshka_dimension`, `detect_gpu_execution_provider`, `ConfigError`, `EmbeddingBackend`, `VectorStoreConfig`); streak now 15 consecutive WOs.
- **AC40** — Module-coherence: commit (4) `feat(embeddings): add Qwen3Embedding + GPU detection (P2-W8-F03)` is ~490 LOC bundling models.rs additions + lib.rs re-exports — body cites the WO-0046 / 0048 / 0056 / 0058 / 0059 / 0060 / 0061 precedent stack.

## Upstream-fit deviation note

Per WO-0058 lessons line 543 (upstream-fitting deviations don't need ADRs):

The WO scope_in[7] prescribed an `if cfg!(feature = "<gpu>")` ladder around the EP probes. The `cuda` / `tensorrt` / `directml` features are declared on the `ort` workspace dep but NOT on `ucil-embeddings` itself, so `cfg!(feature = "cuda")` evaluated against `ucil-embeddings`'s own features (which don't declare any GPU feature) — the rustc check-cfg lint rejected the unknown feature names. Rather than introduce per-crate feature flags that just forward to `ort/cuda` etc. (which would expand the API surface and the dep graph for zero functional benefit on the current build), the implementation calls `is_available()` directly. The runtime semantics are equivalent: `is_available()` queries the loaded `ONNX` Runtime shared library via `GetAvailableProviders` (`ort 2.0.0-rc.12 src/ep/mod.rs:371`); the workspace `download-binaries` feature pulls in a CPU-only build, so all three probes return `Ok(false)` and the function returns `Err(NoGpuDetected)` exactly as the WO specifies. A future workspace `ort` feature flip pulls in a GPU-capable shared library and the same probe activates without any API surface change here.

## Pre-baked mutation execution log (local sanity check, NOT committed)

All three mutations were applied locally, the test was confirmed to fail with operator-readable messages, then `git checkout --` restored the file. Per WO-0061 lessons line 690, the verifier will re-run these mutations independently.

```
mutation #1 — SA5 fails: "validate_matryoshka_dimension(31) must be Err(DimensionOutOfRange { value: 31 }); got Ok(31)"
mutation #2 — SA1 fails: "expected Err(Qwen3EmbeddingError::NoGpuDetected); got Ok(Qwen3Embedding { ... })"
mutation #3 — SA7 fails: "from_config_str(\"qwen3-embedding\") must be Ok(EmbeddingBackend::Qwen3); got Ok(CodeRankEmbed)"
```

## Coverage report

`bash scripts/verify/coverage-gate.sh ucil-embeddings 85 75` reports:

```
[coverage-gate] PASS — ucil-embeddings line=89% branch=n/a
```
