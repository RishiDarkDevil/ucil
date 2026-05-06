# Root Cause Analysis: WO-0059 (P2-W8-F02 — CodeRankEmbed default model)

**Analyst session**: rca-WO-0059-r1
**Feature**: P2-W8-F02
**Work-order**: WO-0059
**Branch**: `feat/WO-0059-ucil-embeddings-coderankembed`
**HEAD reviewed**: `560f07f` (verifier reject commit) over `6cd94ab` (executor RFR HEAD)
**Attempts before RCA**: 1
**Generated**: 2026-05-07T01:00:00Z

## Failure pattern

Single-rejection. **All 30 explicit work-order acceptance criteria pass** —
the frozen acceptance test, the three pre-baked mutations (M1/M2/M3),
workspace build/clippy/fmt, and the prior P2-W8-F01 frozen test all
green. The reject trigger is the **per-WO Quality Gate**
`scripts/verify/coverage-gate.sh ucil-embeddings 85 75` exiting 1 with
line coverage at **80% (floor 85%, delta -5pp)**. Per
`.claude/agents/verifier.md:36`:

> If the gate exits non-zero → **reject** (same path as step 8 below).
> Do not flip the feature even if step 6 passed.

This is a procedurally-correct rejection; the verifier did not have a
choice to accept.

## Root cause (hypothesis, 95% confidence)

**The new `models.rs` ships only a single happy-path frozen test
(`test_coderankembed_inference`), leaving 23 reachable error-path lines
in `CodeRankEmbed::load` and `CodeRankEmbed::embed` uncovered.** The
file lands at 74.34% line coverage (84 / 113), pulling the crate
average from 87.80% (the unchanged `onnx_inference.rs`, 72 / 82) down
to 80.00% (156 / 195) — 5pp below the 85% line floor.

Evidence (cited from the rejection's `cargo llvm-cov --text` per-line
breakdown):

- `crates/ucil-embeddings/src/models.rs:261` — `Err(MissingModelFile)`
  for absent `model.onnx` (1 uncovered line).
- `crates/ucil-embeddings/src/models.rs:267-269` — `Err(MissingModelFile)`
  for absent `tokenizer.json` (3 uncovered lines).
- `crates/ucil-embeddings/src/models.rs:271-274` — corrupt-tokenizer
  `.map_err(|e| Tokenizer { message: ... })` branch (~3 uncovered lines).
- `crates/ucil-embeddings/src/models.rs:355-357` — `embed`'s
  `tokenizer.encode(...).map_err(...)?` branch (~2 uncovered lines).
- `crates/ucil-embeddings/src/models.rs:375` — `embed`'s
  `session.run(...)?` branch (1 uncovered line).
- `crates/ucil-embeddings/src/models.rs:377-382` —
  `outputs.get("sentence_embedding")` missing-output guard (5 uncovered
  lines).
- `crates/ucil-embeddings/src/models.rs:386-390` — post-extract
  `raw.len() != EMBEDDING_DIM` guard (4 uncovered lines).
- `crates/ucil-embeddings/src/models.rs:400-404` — final-invariant
  `pooled.len() != EMBEDDING_DIM` guard (4 uncovered lines).
- `crates/ucil-embeddings/src/models.rs:452-453,469` — assertion-failure
  format-string args in the test itself (cosmetic, only fire on assertion
  failure, ~3 lines).

The seven `CodeRankEmbedError` variants (lines 96-191) include
`MissingModelFile`, `DimensionMismatch`, `Tokenizer`, `Onnx`, `Io`,
`Ndarray` — six of which are unreachable from a single happy-path
test. `models.rs`'s 113-line size compounds the problem: even a small
absolute count of error-path lines drives a meaningful percentage drop.

The pre-existing `onnx_inference.rs` (WO-0058, 82 lines) squeaked
through at 87.80% with similar defensive branches uncovered — coverage
floor is met when the file is small enough that 10 uncovered lines
still leaves a respectable percentage. The new `models.rs` is 38% larger
with more error variants, so the same authoring pattern (one happy-path
test, defensive errors uncovered) drops the crate below the floor.

## Hypotheses considered

### H1 (95%, accepted): missing negative-path test coverage

Above. Falsifiable trivially — the rejection's `cargo llvm-cov` JSON
output names exact uncovered line ranges; reading those ranges against
`models.rs` confirms each is a defensive error-return path.

### H2 (3%, rejected): toolchain mis-instrumentation

Hypothesis: `cargo llvm-cov` is mis-instrumenting `models.rs` (e.g., a
rust-toolchain version mismatch causing `llvm-tools-preview` to
under-count). Falsifiable: the verifier ran `cargo llvm-cov clean` +
fresh build under a clean session per `.claude/agents/verifier.md:21`;
the JSON output is consistent with the per-line text dump. Toolchain
artefact unlikely.

### H3 (1%, rejected): coverage floor not intended for per-WO gates

Hypothesis: the 85% floor was meant only for phase-gate scripts, not
per-WO verification. Falsifiable by reading `verifier.md:34-37`:

> 7. **Quality gates** — for every Rust crate touched by the WO's
> diff [...]. Coverage gate: `scripts/verify/coverage-gate.sh <crate>
> 85 75` — must exit 0. [...] If the gate exits non-zero → **reject**.

The floor is explicitly per-WO and explicitly mandatory. H3 rejected.

### H4 (1%, rejected): WO scope_out forbids the fix

Hypothesis: the WO's `forbidden_paths` block forbids adding the tests
needed to lift coverage. Falsifiable:
`/home/rishidarkdevil/Desktop/ucil/ucil-build/work-orders/0059-ucil-embeddings-coderankembed.json:139`
forbids `crates/ucil-embeddings/tests/**` (the integration-test
directory) but NOT `crates/ucil-embeddings/src/models.rs` itself —
adding `#[test]` functions inside `models.rs` is in-scope per
`scope_in[8]`. H4 rejected.

## Remediation

**Who**: executor

**What**: Add 5-6 negative-path / unit `#[test]` functions to
`crates/ucil-embeddings/src/models.rs` and (safely) extract one private
helper from `CodeRankEmbed::embed` to enable in-isolation unit testing
of the dimension-invariant + L2-normalisation logic. NO change to
production semantics; no change to the frozen acceptance test; no
change to load/embed's external contract.

### Step 1 — add `tempfile` dev-dep (1 LOC, 1 commit)

Edit `crates/ucil-embeddings/Cargo.toml`:

```toml
[dev-dependencies]
tempfile.workspace = true
```

`tempfile = "3"` is already declared at `Cargo.toml:52` (workspace
deps), so no workspace-deps change is needed. The 2-line addition
(`[dev-dependencies]` header + dep line) is in the WO's allow-list per
AC21 (`crates/ucil-embeddings/Cargo.toml` is already on it).

### Step 2 — refactor `embed()` to extract `pool_and_normalise` (~15 LOC, 1 commit)

Extract the dimension-check + L2-normalisation block (currently
`models.rs:386-405`) into a private helper:

```rust
fn pool_and_normalise(raw: &[f32]) -> Result<Vec<f32>, CodeRankEmbedError> {
    if raw.len() != EMBEDDING_DIM {
        return Err(CodeRankEmbedError::DimensionMismatch {
            expected: EMBEDDING_DIM,
            got: raw.len(),
        });
    }
    let mut pooled = raw.to_vec();
    let norm_sq: f32 = pooled.iter().map(|x| x * x).sum();
    let norm = norm_sq.sqrt().max(f32::EPSILON);
    for p in &mut pooled {
        *p /= norm;
    }
    Ok(pooled)
}
```

`embed()`'s call site collapses from ~20 lines to:

```rust
let raw = slice.to_vec();
pool_and_normalise(&raw)
```

The post-normalise `pooled.len() != EMBEDDING_DIM` guard at lines
400-404 is redundant with the pre-normalise check (the helper preserves
length); the refactor naturally elides it. This is in scope per
`scope_in[7]` (implementation freedom for `embed()` provided the
external contract holds). Add a 1-line rustdoc on the helper citing
`P2-W8-F02 / WO-0059 retry-1 coverage-driven extraction`.

### Step 3 — add unit tests (~80 LOC, 1 commit)

Add a `#[cfg(test)] mod tests { ... }` block at the **end** of
`models.rs` (after the frozen `test_coderankembed_inference` at module
root — DEC-0007 keeps the frozen selector
`models::test_coderankembed_inference` in place; the additional tests
live at `models::tests::*` and don't collide with the frozen selector).

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_returns_missing_model_file_for_empty_dir() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        match CodeRankEmbed::load(tmp.path()) {
            Err(CodeRankEmbedError::MissingModelFile { path }) => {
                assert!(
                    path.ends_with("model.onnx"),
                    "expected model.onnx in MissingModelFile; got {path:?}"
                );
            }
            other => panic!(
                "expected Err(MissingModelFile {{ model.onnx }}); got {other:?}"
            ),
        }
    }

    #[test]
    fn pool_and_normalise_returns_dim_mismatch_when_too_short() {
        let too_short = vec![0.5_f32; 100];
        match pool_and_normalise(&too_short) {
            Err(CodeRankEmbedError::DimensionMismatch { expected, got }) => {
                assert_eq!(expected, EMBEDDING_DIM);
                assert_eq!(got, 100);
            }
            other => panic!("expected DimensionMismatch; got {other:?}"),
        }
    }

    #[test]
    fn pool_and_normalise_returns_dim_mismatch_when_too_long() {
        let too_long = vec![0.5_f32; 1024];
        match pool_and_normalise(&too_long) {
            Err(CodeRankEmbedError::DimensionMismatch { expected, got }) => {
                assert_eq!(expected, EMBEDDING_DIM);
                assert_eq!(got, 1024);
            }
            other => panic!("expected DimensionMismatch; got {other:?}"),
        }
    }

    #[test]
    fn pool_and_normalise_l2_normalises_correct_length_input() {
        let raw = vec![3.0_f32; EMBEDDING_DIM];
        let pooled = pool_and_normalise(&raw).expect("happy path");
        assert_eq!(pooled.len(), EMBEDDING_DIM);
        let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "expected unit vector after L2-normalise; got norm={norm}"
        );
    }

    #[test]
    fn pool_and_normalise_clamps_zero_input_to_epsilon() {
        let zero = vec![0.0_f32; EMBEDDING_DIM];
        let pooled = pool_and_normalise(&zero).expect("zero-vector path");
        assert_eq!(pooled.len(), EMBEDDING_DIM);
        assert!(
            pooled.iter().all(|x| x.is_finite()),
            "EPSILON clamp must keep all floats finite; got {} finite of {}",
            pooled.iter().filter(|x| x.is_finite()).count(),
            pooled.len(),
        );
    }

    #[test]
    fn coderankembed_error_display_renders_expected_format() {
        let e = CodeRankEmbedError::MissingModelFile {
            path: std::path::PathBuf::from("/no/such/path/model.onnx"),
        };
        let s = format!("{e}");
        assert!(
            s.contains("required model file missing"),
            "MissingModelFile Display must contain canonical text; got {s:?}"
        );

        let e = CodeRankEmbedError::DimensionMismatch {
            expected: EMBEDDING_DIM,
            got: 0,
        };
        let s = format!("{e}");
        assert!(
            s.contains("unexpected embedding dimension"),
            "DimensionMismatch Display must contain canonical text; got {s:?}"
        );

        let e = CodeRankEmbedError::Tokenizer {
            message: "bad json".into(),
        };
        let s = format!("{e}");
        assert!(
            s.contains("tokenizer error"),
            "Tokenizer Display must contain canonical text; got {s:?}"
        );
    }
}
```

These six tests cover:

- `load`'s line 261 (MissingModelFile for missing model.onnx) — 1 line.
- `pool_and_normalise`'s DimensionMismatch return (replacing former
  386-390 + 400-404) — 4-8 lines depending on collapse.
- `pool_and_normalise`'s L2-normalise success path — 4 lines (already
  covered by happy-path; redundancy keeps the test small).
- `pool_and_normalise`'s EPSILON-clamp branch — exercises the
  `.max(f32::EPSILON)` fall-through.
- `CodeRankEmbedError`'s `#[error("...")]` Display impls — exercises
  the thiserror-generated formatting code paths (3 variants).

Net new line coverage estimate: **~14 lines** (file → ~98/113 = 86.7%;
crate → ~170/195 = 87.2%). Comfortably clears 85%.

### Acceptance for retry

- `bash scripts/verify/coverage-gate.sh ucil-embeddings 85 75` exits 0
  with line ≥ 85% (target 87% — gives 2pp headroom).
- All AC01-AC30 from the original WO continue to pass (reality-check
  M1/M2/M3 still trip — they target `load`, `embed`, and
  `EMBEDDING_DIM`; the helper extraction does not change M1/M2/M3 sed
  targets because `load`'s body and `EMBEDDING_DIM` are unchanged, and
  M2's `Ok(Vec::new())` short-circuit still fires SA4 on len mismatch).
- No production-semantics change: `CodeRankEmbed::load` and
  `CodeRankEmbed::embed` external contracts unchanged.

### Risks & mitigations

**R1 — extracted helper changes M2 mutation behaviour.** M2 stashes
`embed`'s body to early `Ok(Vec::new())`; the empty Vec propagates to
SA4's `assert_eq!(embedding.len(), 768, ...)` which still panics
correctly. The helper extraction does not touch M2's sed shape (the
`return Ok(Vec::new())` still lands at the top of `embed`'s body
before any helper call). **Mitigation**: the executor MUST re-run
M1/M2/M3 locally before pushing the retry to confirm the mutations
still trip the frozen test. Re-cite the existing AC18/AC19/AC20 sed
patches in the RFR.

**R2 — commit count climbs above AC24 ≤ 8 ceiling.** WO-0059's first
attempt is at 10 commits (already above the soft ceiling, accepted in
the prior rejection per the WO-0058 line 568 precedent). The retry
adds 3 more (dev-dep + refactor + tests) → 13 total. **Mitigation**:
document the constraint chain (refactor + new tests + coverage-driven
retry) in the RFR per the WO-0058 line 568 precedent. The verifier
already accepts AC24 SOFT-FAILs when justified.

**R3 — `tempfile` adds ~10kB to the build.** Negligible; `tempfile`
is already a workspace dep used by other crates.

**R4 — refactoring `embed` could regress the frozen test wall-time.**
The helper is a single function call; LLVM inlines it. No measurable
change.

**R5 — pinhole in commits #1-2 if the executor doesn't re-run
`scripts/verify/P2-W8-F02.sh` after the refactor.** The verify script
only checks the test summary line; the refactor's correctness is
covered by the existing happy-path test. **Mitigation**: standing rule
per scope_in[15] — `cargo build --workspace --tests` + the verify
script before RFR.

## If hypothesis is wrong

If after applying the above remediation the coverage gate STILL fails
(unlikely — the math gives ≥86.7% for `models.rs`):

**Fallback 1**: add 2 more tests targeting the `load`'s
tokenizer-existence-check branch (lines 267-269) by hard-linking the
real `ml/models/coderankembed/model.onnx` into a tempdir without a
`tokenizer.json` (test gates on `model.onnx` being present, panics
helpfully if not — same shape as the frozen test's pre-flight). This
hits an additional 3 lines and ~1pp.

**Fallback 2**: if the verifier finds the helper extraction
unsatisfying (too far from scope_in[7]'s prescription), inline the
helper back into `embed` and instead add tests that exercise
`CodeRankEmbed::embed` on the real model with degenerate inputs (empty
string, single character, very long snippet). The real-model tests are
slower (~0.5s each) but always-correct; they hit the encode/decode
paths organically.

**Fallback 3**: if neither fallback works (suggests our coverage math
is off), open `ucil-build/verification-reports/coverage-ucil-embeddings.md`
written by `coverage-gate.sh` and re-run `cargo llvm-cov --package
ucil-embeddings --text` to identify the actual remaining uncovered
ranges; pick targeted tests from there.

## Citations

- Rejection: `ucil-build/rejections/WO-0059.md` (read-only).
- Coverage gate script: `scripts/verify/coverage-gate.sh:24-237`.
- Verifier procedure: `.claude/agents/verifier.md:34-37` (step 7
  Quality gates — coverage-floor enforcement).
- Source under review: `crates/ucil-embeddings/src/models.rs:88-491`
  (worktree at `/home/rishidarkdevil/Desktop/ucil-wt/WO-0059`).
- WO scope: `ucil-build/work-orders/0059-ucil-embeddings-coderankembed.json`
  (`scope_in[7-8]`, `forbidden_paths:139` — `tests/**` forbidden but
  in-file `#[test]` functions in `src/models.rs` are in-scope).
- WO-0058 precedent for soft-fail commit-count + RFR constraint chain
  documentation: `ucil-build/rejections/WO-0059.md:158-160` (cites the
  precedent verbatim).
- DEC-0007 frozen-selector module-root placement (the additional
  `#[cfg(test)] mod tests` does NOT compete with the frozen
  `models::test_coderankembed_inference` selector at module root).
- `tempfile = "3"` workspace pin: `Cargo.toml:52`.
