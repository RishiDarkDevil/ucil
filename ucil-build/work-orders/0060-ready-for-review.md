# WO-0060 — Ready for Review

**Branch**: `feat/WO-0060-ucil-embeddings-chunker`
**Final commit sha**: `b4b623d7c07b8f43f560a8037e1548eed4d8af17`
**Feature**: `P2-W8-F05` — embedding chunker
**Master-plan**: §12.2 line 1339 (chunking contract); §18 Phase 2 Week 8 line 1786 (W8 embedding pipeline)

## Commit ladder (6 commits)

1. `1367b45` — `build(embeddings): add ucil-treesitter path dep for chunker module`
2. `d43c6ce` — `feat(embeddings): add EmbeddingChunker skeleton + helpers`
3. `7b89770` — `test(embeddings): add sample.rs fixture for chunker test`
4. `fe3e4ef` — `test(embeddings): add chunker::test_embedding_chunker_real_fixture`
5. `13656ae` — `test(embeddings): add chunker negative-path unit tests for coverage`
6. `b4b623d` — `feat(embeddings): add scripts/verify/P2-W8-F05.sh`

Each commit subject ≤70 chars per WO-0059 lessons line 607. Per DEC-0005 module-coherence carve-out, commit-#2 (skeleton + types + struct + constructors + helpers + chunk pipeline) lands ~503 LOC in a single commit because narrower splits would leave dead-code / unused-import warnings under `#![deny(warnings)]` (precedent: WO-0048 297-LOC, WO-0056 542-LOC, WO-0058 305-LOC, WO-0059 425-LOC).

## What I verified locally

- **AC01** — `cargo build -p ucil-embeddings` exits 0.
- **AC02** — `crates/ucil-embeddings/Cargo.toml` declares `ucil-treesitter = { path = "../ucil-treesitter" }` (1 match via grep).
- **AC03** — `crates/ucil-embeddings/src/chunker.rs` exists; declares `pub struct EmbeddingChunker` at line 254.
- **AC04** — `cargo clippy -p ucil-embeddings --all-targets -- -D warnings` exits 0.
- **AC05** — `cargo test -p ucil-embeddings chunker::test_embedding_chunker_real_fixture -- --nocapture` exits 0; the single test passes with all 5 sub-assertions green.
- **AC06** — `grep -nE '^fn test_embedding_chunker_real_fixture' crates/ucil-embeddings/src/chunker.rs` → `576:fn test_embedding_chunker_real_fixture()`. Module-root placement per DEC-0007.
- **AC07-AC11** — All 5 sub-assertions pass: chunk count positive, every chunk respects `MAX_CHUNK_TOKENS = 512`, `chunks[0].token_count == content.split_whitespace().count()` (real tokenizer, NOT byte estimate), metadata round-trips, source-order ordering monotonic.
- **AC12** — Unit test `from_tokenizer_path_returns_missing_tokenizer_file_for_absent_path` passes — exercises `MissingTokenizerFile` variant.
- **AC13** — Unit test `from_tokenizer_path_returns_tokenizer_for_invalid_json` passes — exercises `Tokenizer { message }` variant via 21 garbage bytes in NamedTempFile.
- **AC14** — Unit test `retokenize_chunk_collapses_oversize_to_signature` passes — synthetic 1000-word chunk collapsed below cap.
- **AC15** — Unit test `collapse_to_signature_handles_single_line_oversize_content` passes — 5000-token single-line input hard-truncated. (`collapse_to_signature` now uses iterative byte-budget shrinking when the 4-bytes-per-token heuristic over-provisions.)
- **AC16** — Unit test `embedding_chunk_round_trips_metadata` passes — Debug + Clone + PartialEq + Eq smoke test.
- **AC17** — `cargo test --workspace --no-fail-fast` exits 0 (35 test groups, zero failures). Required `bash scripts/devtools/install-coderankembed.sh` to run first to download model artefacts (~138 MB) for `models::test_coderankembed_inference`; this is the standing pattern from WO-0059.
- **AC18** — `cargo test -p ucil-daemon --test e2e_mcp_stdio --test e2e_mcp_with_kg` exits 0.
- **AC19** — `cargo test -p ucil-treesitter` exits 0.
- **AC20** — `cargo test -p ucil-embeddings onnx_inference::test_onnx_session_loads_minimal_model -- --nocapture` exits 0; `models::test_coderankembed_inference` passes once artefacts are present.
- **AC21** — `cargo test --test test_plugin_lifecycle` exits 0.
- **AC22** — Mutation #1 (chunk() body neutered to `return Ok(Vec::new())` with `#[allow(unused_variables, unreachable_code, clippy::needless_return)]`) verified: frozen test panics at `chunks must produce at least one chunk; got []`. `git checkout -- crates/ucil-embeddings/src/chunker.rs` restores; rerun green.
- **AC23** — Mutation #2 (oversize-fallback branch neutered to leak oversize through; `#[allow(unreachable_code, clippy::needless_return)]`) verified: `chunker::tests::retokenize_chunk_collapses_oversize_to_signature` panics at `oversize chunk must collapse below cap; got token_count=…`. Restored; rerun green.
- **AC24** — Mutation #3 (token_count assignment swapped from `real_count` to `ast_chunk.token_count` byte-estimate; `#[allow(unused_variables)]`) verified: frozen test panics at `token_count must reflect real tokenizer (whitespace word count); got <byte_estimate> expected <word_count>`. Restored; rerun green.
- **AC25** — Stub-scan: zero `todo!()` / `unimplemented!()` / `panic!("…not yet…")` / `TODO` / `FIXME` matches in `crates/ucil-embeddings/src/chunker.rs`.
- **AC26** — Allow-list verification (`git diff --name-only main...HEAD`): `Cargo.lock`, `crates/ucil-embeddings/Cargo.toml`, `crates/ucil-embeddings/src/chunker.rs`, `crates/ucil-embeddings/src/lib.rs`, `crates/ucil-embeddings/tests/data/sample.rs`, `scripts/verify/P2-W8-F05.sh`. Adding the RFR marker brings the count to 7 paths — exactly matching the prescribed allow-list.
- **AC27** — `Cargo.toml` (root workspace) NOT modified (`git diff --name-only main...HEAD -- Cargo.toml` empty). `ucil-treesitter` path dep declared only in `crates/ucil-embeddings/Cargo.toml`.
- **AC28** — `lib.rs` re-exports all 4 new public symbols (`EmbeddingChunk`, `EmbeddingChunker`, `EmbeddingChunkerError`, `MAX_CHUNK_TOKENS`) on a single `pub use chunker::{...}` line. Cumulative re-export-discipline streak: 14 consecutive WOs cleared.
- **AC29** — `tests/fixtures/**` NOT modified. The new fixture lives at `crates/ucil-embeddings/tests/data/sample.rs` (per-crate test data, NOT repo-root fixtures).
- **AC30** — `env -u RUSTC_WRAPPER bash scripts/verify/coverage-gate.sh ucil-embeddings 85 75` exits 0 with line coverage **90%** (`ucil-embeddings` total — chunker.rs at 93.06%, models.rs at 85.42%, onnx_inference.rs at 87.80%; total 89.85% lines, 89.64% regions). Required `cargo clean -p ucil-embeddings` first to drop stale non-instrumented binaries from earlier test runs (the gate script's `cargo llvm-cov clean` does NOT touch `target/debug/`); after the clean, the gate passes idempotently.
- **AC31** — `ucil-build/feature-list.json` and `ucil-build/feature-list.schema.json` NOT modified.
- **AC32** — `ucil-master-plan-v2.1-final.md` NOT modified.
- **AC33** — Commit cadence: 6 commits per `.claude/rules/commit-style.md` and DEC-0005 module-coherence carve-out. Every commit subject ≤70 chars; every commit body cites `Phase: 2 / Feature: P2-W8-F05 / Work-order: WO-0060`.
- **AC34** — Branch up-to-date with origin (`git rev-parse HEAD == git rev-parse @{u}` modulo this RFR commit); working tree clean.
- **AC35** — `bash scripts/verify/P2-W8-F05.sh` exits 0 with `[OK] P2-W8-F05`.

## Notable design decisions (vs WO scope_in)

- **Manual `Debug` impl for `EmbeddingChunker`** — `ucil_treesitter::Parser` does not derive `Debug`, so the `#[derive(Debug)]` on `EmbeddingChunker` was replaced with a hand-rolled formatter that elides the parser internals. WO scope_in §6 prescribed `#[derive(Debug)]` but the upstream surface forced a fit; this is an "inline divergence" per WO-0058 lessons line 543 (NOT an ADR-required change).

- **Synthetic tokenizer constructed via `tokenizer.json` deserialisation, not `WordLevelBuilder` + `AHashMap`** — the `WordLevelBuilder::vocab()` method takes `ahash::AHashMap<String, u32>`, but `ahash` is not exposed via the workspace deps. Instead of adding `ahash` to dev-dependencies, the synthetic tokenizer is built by parsing a hand-crafted `tokenizer.json` payload string (`Tokenizer::from_str` via `FromStr`) — same `WordLevel + WhitespaceSplit` shape, no new dep. Per DEC-0008 this is still NOT a critical-dep mock — every byte of the JSON is interpreted by the real `tokenizers` crate.

- **Source-order sort in `chunk()`** — `ucil_treesitter::Chunker` returns symbols in tree-sitter query-match order, which interleaves classes and their nested method symbols. The frozen test's SA5 (source-order monotonic) revealed that the output isn't sorted by `start_line` upstream. `EmbeddingChunker::chunk` now `sort_by_key(|c| c.start_line)` before returning — required by the `LanceDB` indexer (P2-W8-F04) for stable chunk ids. Documented inline.

- **Iterative byte-budget shrink in `collapse_to_signature`** — the WO scope_in prescribed a single hard-truncation step for oversize signatures, but the synthetic tokenizer's tight 2-bytes-per-token ratio (one space + one ASCII char per token) blew through the 4-bytes-per-token static budget. `collapse_to_signature` now iteratively halves the budget until the real-tokenizer count satisfies the cap. Documented inline.

- **`Cargo.lock` re-resolves** — adding `ucil-treesitter` as a path dep produced a 1-line additive entry in `Cargo.lock` (no version bumps in unrelated packages).

## Stop-hook compliance

- Working tree clean (`git status --porcelain` empty after RFR commit).
- Branch up-to-date with `origin/feat/WO-0060-ucil-embeddings-chunker`.
- No uncommitted changes; all 6 commits + this RFR commit are pushed.

## Verification protocol for the verifier

1. `git fetch origin feat/WO-0060-ucil-embeddings-chunker && git checkout feat/WO-0060-ucil-embeddings-chunker`
2. `bash scripts/devtools/install-coderankembed.sh` (download artefacts if absent — required for AC17)
3. `cargo clean -p ucil-embeddings && env -u RUSTC_WRAPPER cargo llvm-cov clean --workspace`
4. Run `bash scripts/verify/P2-W8-F05.sh` — must exit 0
5. Run `env -u RUSTC_WRAPPER bash scripts/verify/coverage-gate.sh ucil-embeddings 85 75` — must exit 0
6. Run `cargo test --workspace --no-fail-fast` — must exit 0
7. Optionally re-run mutations AC22-AC24 per the prescribed runtime-only variants
