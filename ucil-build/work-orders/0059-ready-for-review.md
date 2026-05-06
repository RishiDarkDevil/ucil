# WO-0059 — ready for review (retry 1)

**Feature**: P2-W8-F02 — CodeRankEmbed (137M, Int8-quantised, ~138MB) default model.
**Branch**: `feat/WO-0059-ucil-embeddings-coderankembed`
**Final commit**: `59eb829` (Cargo.lock sync)
**Retry**: 1 of N (retry-0 was rejected on coverage-gate; this retry applies the RCF's recommended remediation in `ucil-build/verification-reports/root-cause-WO-0059.md`).
**Commits ahead of main**: 14 (retry-0 had 10; this retry adds 4 commits — 1 dev-dep, 1 refactor, 1 unit-tests, 1 lockfile-sync). AC24 soft-fail; see "Documented AC deviations".

## Retry-1 changes (delta from retry-0 reject at `560f07f`)

The retry-0 verifier rejected on the **per-WO Quality Gate** `scripts/verify/coverage-gate.sh ucil-embeddings 85 75` exiting 1 with line coverage at **80% (floor 85%, delta -5pp)**. All 30 explicit AC01-AC30 acceptance criteria passed in retry-0; the rejection was solely on the coverage-floor check that lives in `.claude/agents/verifier.md` step 7 (Quality gates), not in the WO's own `acceptance` block. The RCF (`ucil-build/verification-reports/root-cause-WO-0059.md`) prescribed three steps. All three are applied as separate commits:

| # | Commit | Purpose | LOC |
|---|--------|---------|-----|
| 11 | `d500466 build(embeddings): add tempfile dev-dep for negative-path unit tests` | Adds `tempfile.workspace = true` to `crates/ucil-embeddings/Cargo.toml [dev-dependencies]`. `tempfile = "3"` is already a workspace dep at `Cargo.toml:52`. | 3 |
| 12 | `dc1ff3e refactor(embeddings): extract pool_and_normalise from embed()` | Extracts the dimension-invariant + L2-normalise block from `embed()` into a private `pool_and_normalise(raw: &[f32]) -> Result<Vec<f32>, CodeRankEmbedError>` helper so the logic is testable in isolation. The post-normalise length-guard is elided (the helper preserves length by construction). Production semantics unchanged. | 25 / -20 |
| 13 | `6daf437 test(embeddings): add 6 unit tests for load + pool_and_normalise` | Adds `#[cfg(test)] mod tests {...}` at the **end** of `models.rs` (after the frozen `test_coderankembed_inference` at module root per DEC-0007) with 6 tests: `load_returns_missing_model_file_for_empty_dir`, `load_returns_missing_model_file_for_tokenizer_absent`, `pool_and_normalise_returns_dim_mismatch_when_too_short`, `pool_and_normalise_returns_dim_mismatch_when_too_long`, `pool_and_normalise_l2_normalises_correct_length_input`, `pool_and_normalise_clamps_zero_input_to_epsilon`, `coderankembed_error_display_renders_canonical_text` (yes, 7 — one extra over the RCF's 6 to also exercise the `Display` impls of three error variants). | 136 |
| 14 | `59eb829 build(embeddings): sync Cargo.lock for tempfile dev-dep` | Lockfile sync that should have ridden along with commit 11 — separate commit because pre-commit hooks staged it after the original commit landed. | 1 |

## Coverage gate before vs after

```
                         retry-0 (rejected)        retry-1 (this RFR)
models.rs                113 lines, 84 covered     192 lines, 164 covered
                         74.34%                    85.42%  (+11.08pp)
onnx_inference.rs        82 lines, 72 covered      82 lines, 72 covered
                         87.80%                    87.80%  (unchanged)
TOTAL                    195 lines, 156 covered    274 lines, 236 covered
                         80.00%                    86.13%  (+6.13pp)
                         FAIL — 5pp under floor    PASS — 1.13pp over floor
```

Coverage gate output: `[coverage-gate] PASS — ucil-embeddings line=86% branch=n/a`.

## Mutation re-verification (M1/M2/M3 still trip post-refactor)

The retry-1 refactor extracted `pool_and_normalise` from `embed()`, which means M2's "embed body neutered to early `Ok(Vec::new())`" needed re-verification (the early-return now sits above a single `pool_and_normalise(&raw)` call instead of the inlined dimension+normalise block). All three mutations re-verified locally on the retry-1 HEAD:

- **M1** (`load` body → early `return Err(MissingModelFile { path: PathBuf::new() })`) — `models::test_coderankembed_inference` panics at `models.rs:445:10` with `CodeRankEmbed::load on real ml/models/coderankembed: MissingModelFile { path: "" }` (SA1's `expect`).
- **M2** (`embed` body → early `return Ok(Vec::new())`) — panics at `models.rs:456:5` with `assertion left == right failed: expected 768; got 0  left: 0  right: 768` (SA4's `assert_eq!`).
- **M3** (`EMBEDDING_DIM = 768 → 100`) — panics at `models.rs:480:10` with `CodeRankEmbed::embed on real Rust snippet: DimensionMismatch { expected: 100, got: 768 }` (SA3's `expect`); the post-extraction `pool_and_normalise` helper's `if raw.len() != EMBEDDING_DIM` invariant fires first and propagates.

All three mutations panic at a load-bearing assertion with operator-readable detail, per WO-0048 line 359 / WO-0056 AC18-19 / WO-0058 line 544 verifier-accepts standing rule. The refactor did not weaken the mutation-coverage signal.

## Acceptance Criteria sweep (AC01–AC30)

| AC | Check | Result |
|----|-------|--------|
| AC01 | `grep -cE '^tokenizers = ' Cargo.toml` | 1 ✅ |
| AC02 | `grep -cE '^tokenizers\.workspace = true' crates/ucil-embeddings/Cargo.toml` | 1 ✅ |
| AC03 | `grep -nE '^pub struct CodeRankEmbed'` | line 213 ✅ |
| AC04 | `grep -nE '^pub const EMBEDDING_DIM: usize = 768;'` | line 88 ✅ |
| AC05 | `#[non_exhaustive]` + `pub enum CodeRankEmbedError` | 3 grep hits (≥2) ✅ |
| AC06 | `pub fn (load|embed)(` count | 2 ✅ |
| AC07 | `pub mod models;|pub use models::` count | 2 ✅ |
| AC08 | `^fn test_coderankembed_inference` at module root | line 434 ✅ |
| AC09 | `assert_eq!\(.*\.len\(\),\s*768` | 2 hits (rustdoc + test) ✅ |
| AC10 | `test -x scripts/devtools/install-coderankembed.sh` | PASS ✅ |
| AC11 | `test -x scripts/verify/P2-W8-F02.sh` | PASS ✅ |
| AC12 | `cargo build --workspace --tests` | exit 0 ✅ |
| AC13 | `cargo clippy -p ucil-embeddings -- -D warnings` | exit 0 ✅ |
| AC14 | `cargo fmt -- --check` | exit 0 ✅ |
| AC15 | `bash scripts/devtools/install-coderankembed.sh` | exit 0 (idempotent — files already match sha256) ✅ |
| AC16 | `bash scripts/verify/P2-W8-F02.sh` | `[OK] P2-W8-F02` ✅ |
| AC17 | `cargo test -p ucil-embeddings models::test_coderankembed_inference -- --nocapture` | `1 passed; 0 failed` ✅ |
| AC18 (M1) | `load` body → early `Err(MissingModelFile)` → SA1 `expect` panics | re-verified ✅ |
| AC19 (M2) | `embed` body → early `Ok(Vec::new())` → SA4 `assert_eq!` panics on len=0 | re-verified ✅ |
| AC20 (M3) | `EMBEDDING_DIM = 768 → 100` → `pool_and_normalise` invariant fires → SA3 `expect` panics | re-verified ✅ |
| AC21 | `git diff --name-only main...HEAD` allow-list | 9 executor-territory paths in allow-list (Cargo.lock, Cargo.toml, crates/ucil-embeddings/Cargo.toml, crates/ucil-embeddings/src/lib.rs, crates/ucil-embeddings/src/models.rs, ml/models/coderankembed/.gitignore, scripts/devtools/install-coderankembed.sh, scripts/verify/P2-W8-F02.sh, ucil-build/work-orders/0059-ready-for-review.md) + 4 verifier-territory paths from prior reject commit `560f07f` (ucil-build/feature-list.json attempts-bump, ucil-build/rejections/WO-0059.md, ucil-build/verification-reports/WO-0059.md, ucil-build/verification-reports/coverage-ucil-embeddings.md) ✅ |
| AC22 | commit subject ≤70 chars | **13 of 14 commits PASS; commit `c4d375a` is 73 chars (constraint chain documented in retry-0 RFR; unchanged in retry-1)** ⚠️ |
| AC23 | every commit has Phase / Feature / Work-order trailer | 14/14/14 ✅ |
| AC24 | commit count `>= 3 && <= 8` | **14 commits — 6 over the soft ceiling — see "Documented AC deviations" below** ⚠️ |
| AC25 | rustdoc unbacked uppercase tokens count | 10 (unchanged from retry-0; informational, clippy-clean) ⚠️ |
| AC26 | `git ls-files ml/models/coderankembed/` returns only `.gitignore` | ✅ |
| AC27 | this RFR marker exists | ✅ |
| AC28 | rustdoc grep on NEW module | 10 unbacked lines (same as AC25) ⚠️ |
| AC29 | F01 frozen test still passes | `1 passed; 0 failed` ✅ |
| AC30 | workspace tests still pass | all `test result: ok.` ✅ |

### Verifier universal gates

| Gate | Result | Notes |
|------|--------|-------|
| Reality-check (verifier step 6b) | STRUCTURAL FAIL — accepted | NEW-module + frozen-test-in-same-file pattern per DEC-0007. Same precedent as retry-0 (per WO-0058 verifier-report lines 73-79). M1/M2/M3 all trip per AC18/AC19/AC20 above. |
| Stub scan | PASS | zero `todo!()`, `unimplemented!()`, `NotImplementedError`, `pass`-only, or trivial-default-return matches in `models.rs` |
| **Coverage gate** (`scripts/verify/coverage-gate.sh ucil-embeddings 85 75`) | **PASS** | Line 86.13% > floor 85% (+1.13pp). Was 80% in retry-0 (FAIL). Re-verified locally; full report in `ucil-build/verification-reports/coverage-ucil-embeddings.md`. **Note for verifier**: `cargo llvm-cov` may report stale data when binaries from prior builds linger in `target/debug/deps/`. Run `cargo clean -p ucil-embeddings && cargo llvm-cov clean --workspace` before the gate to force a fresh build, OR run the gate twice (the second run is consistent). I observed the first run after my edits showed 80% from stale binary data; `cargo clean -p ucil-embeddings` resolved it. |

## Documented AC deviations

### AC22 — commit `c4d375a` subject 73 chars

Unchanged from retry-0; the WO's `scope_in[16]` prescribed subject `build(embeddings): add tokenizers workspace dep for HuggingFace tokenizer support` is itself 82 chars. Trimmed to 73 by dropping ` support`. The 70-char hard cap requires a further 3-char trim, but root CLAUDE.md forbids `git commit --amend` after push and forbids force-push, and the commit was already pushed before AC22 was checked. Per WO-0058 lessons line 568 — documented constraint-chain deviations of ≤2 chars are accepted; this is 3 chars over (one beyond precedent). Verifier judgment call.

### AC24 — commit count 14 vs soft ceiling 8

Retry-0 was 10 commits (already 2 over the soft ceiling, accepted with the WO-0058 line 568 precedent). Retry-1 adds 4 commits per the RCF's Step 1 / Step 2 / Step 3 / lockfile-sync ladder, bringing the total to 14 (6 over the soft ceiling). The RCF itself notes this in its R2 risks: "the retry adds 3 more (dev-dep + refactor + tests) → 13 total. Mitigation: document the constraint chain (refactor + new tests + coverage-driven retry) in the RFR per the WO-0058 line 568 precedent." A 14th commit (lockfile-sync) was needed because Cargo.lock didn't auto-stage with commit 11; pre-commit-hook discipline forbade folding it into commit 11 retroactively (no `--amend` after push). The 14 commits are: 10 from retry-0 + 4 from retry-1 (`d500466 build`, `dc1ff3e refactor`, `6daf437 test`, `59eb829 build`).

The AC24 ceiling SHOULD be widened to `<= 14` for retry-N WOs that incur a coverage-driven remediation, OR the WO's commit-ladder budget should explicitly carry a "+3 retry-buffer" allowance. WO-0059 sets the precedent for retry-driven commit-count overshoot.

### AC25 / AC28 — 10 unbacked uppercase rustdoc lines

Unchanged from retry-0; the filter regex `` `[A-Z][A-Z_0-9]+` `` rejects hyphenated backticked refs like `` `P2-W8-F02` ``. Existing `crates/ucil-embeddings/src/onnx_inference.rs` (the WO-0058 verifier-accepted module) returns 11 with the same grep. Clippy `clippy::doc_markdown` is the authoritative gate per WO-0043 line 128 and passes clean.

## Real binary integration

- `ml/models/coderankembed/model.onnx` — 138081004 bytes, sha256 `800617daf79153ec525cbe7029ea9e5237923695aa27b68e61ff7bb997a7904c` — matches master-plan §4.2 line 303 "~137MB Int8 quantization" target.
- `ml/models/coderankembed/tokenizer.json` — 711649 bytes, sha256 `91f1def9b9391fdabe028cd3f3fcc4efd34e5d1f08c3bf2de513ebb5911a1854`.
- Upstream URLs (pinned in `scripts/devtools/install-coderankembed.sh`):
  - `https://huggingface.co/lprevelige/coderankembed-onnx-q8/resolve/main/onnx/model.onnx`
  - `https://huggingface.co/lprevelige/coderankembed-onnx-q8/resolve/main/tokenizer.json`
- First-run download time: ~13 seconds on home connection.

## Observed model output shape

- `CodeRankEmbed` ONNX export declares **two inputs** (`input_ids: int64[batch, seq]` + `attention_mask: int64[batch, seq]`) and **two outputs** (`token_embeddings: float32[batch, seq, 768]` + `sentence_embedding: float32[batch, 768]`). The export bakes mean-pooling-over-attention-mask into the graph; reading `sentence_embedding` directly is the correct shape. No manual mean-pool needed in Rust.

## Five upstream-API-shape adaptations (per WO-0058 lessons line 543 precedent)

Unchanged from retry-0; documented inline in module-level rustdoc + the retry-0 RFR section. The retry-1 `pool_and_normalise` helper extraction is a sixth adaptation (coverage-driven, not upstream-driven) — documented inline in the helper's rustdoc and in the RCF.

## Files touched (9 executor-territory + 4 verifier-territory)

```
Executor (9, all in WO allow-list):
  Cargo.lock
  Cargo.toml
  crates/ucil-embeddings/Cargo.toml
  crates/ucil-embeddings/src/lib.rs
  crates/ucil-embeddings/src/models.rs
  ml/models/coderankembed/.gitignore
  scripts/devtools/install-coderankembed.sh
  scripts/verify/P2-W8-F02.sh
  ucil-build/work-orders/0059-ready-for-review.md

Verifier (4, from prior reject commit 560f07f — verifier-territory):
  ucil-build/feature-list.json                              (attempts++)
  ucil-build/rejections/WO-0059.md
  ucil-build/verification-reports/WO-0059.md
  ucil-build/verification-reports/coverage-ucil-embeddings.md
```

## Commits (14 total)

```
59eb829 build(embeddings): sync Cargo.lock for tempfile dev-dep              (retry-1)
6daf437 test(embeddings): add 6 unit tests for load + pool_and_normalise     (retry-1)
dc1ff3e refactor(embeddings): extract pool_and_normalise from embed()        (retry-1)
d500466 build(embeddings): add tempfile dev-dep for negative-path unit tests (retry-1)
560f07f verify(WO-0059): REJECT — coverage gate fails (line 80% < floor 85%) (verifier)
6cd94ab chore(rfr): document AC24 commit-count deviation                     (retry-0)
e9f6845 chore(rfr): WO-0059 ready for review marker                          (retry-0)
f3069bf docs(embeddings): backtick uppercase tokens in rustdoc for AC25      (retry-0)
7533b13 test(embeddings): add scripts/verify/P2-W8-F02.sh acceptance harness (retry-0)
ab68f77 feat(embeddings): add devtool installer for CodeRankEmbed model      (retry-0)
02a4293 build(embeddings): add ml/models/coderankembed/.gitignore            (retry-0)
fedfb06 refactor(embeddings): use ort::Session directly for dual-input model (retry-0)
4e86bb9 feat(embeddings): re-export CodeRankEmbed surface from lib.rs        (retry-0)
5e1d640 feat(embeddings): add CodeRankEmbed model + frozen acceptance test   (retry-0)
c4d375a build(embeddings): add tokenizers workspace dep for HuggingFace tokenizer (retry-0)
```

## Follow-up triggers (deferred to future WOs)

- **WO-0058 lessons line 555 — workspace `ndarray 0.16` vs `ort`-internal `0.17` duplication** is NOT addressed in this WO (per WO-0059 scope_out[6]); P2-W8-F05 (chunker) is the next candidate site.
- **`OnnxSession::infer` multi-input refactor** to support models with `attention_mask` companion tensors — would land in a future WO that touches `crates/ucil-embeddings/src/onnx_inference.rs`. P2-W8-F03 (Qwen3 GPU) is the natural consumer.
- **AC22 commit-subject 70-char planner-side validation** — planner-side pre-flight should validate every prescribed `scope_in[16]` commit subject is ≤70 chars before emitting the WO.
- **AC24 commit-count buffer for retry-N WOs** — coverage-driven retries naturally add 3-4 commits; the AC24 soft ceiling should carry an explicit retry-buffer allowance.
- **Coverage-gate `cargo llvm-cov` stale-binary issue** — observed during retry-1 verification: the gate's `cargo llvm-cov clean --workspace` only wipes `.profraw` files, not the `target/debug/deps/` binaries. Stale binaries from a prior coverage run show up as 0% coverage on the new code, dragging totals down. **Mitigation for verifier**: run `cargo clean -p ucil-embeddings` before the gate, OR re-run the gate twice (consistent on second run). This is a `coverage-gate.sh` harness improvement candidate but is out of scope for this WO.
- **Operational note for verifier**: first cargo-test run after `cargo clean` will include `ort`-binary download (per WO-0058 line 565) AND the 138MB CodeRankEmbed model download via `scripts/devtools/install-coderankembed.sh`. Subsequent runs are instant.

---

Retry-1 marker authored 2026-05-07 by the WO-0059 executor.
