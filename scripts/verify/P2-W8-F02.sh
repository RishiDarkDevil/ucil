#!/usr/bin/env bash
# Acceptance test for P2-W8-F02 — CodeRankEmbed default model
# (master-plan §18 Phase 2 Week 8 line 1787 — "CodeRankEmbed
# (137M, CPU) as default, Qwen3-Embedding (8B, GPU optional) as
# upgrade").  Authored under WO-0059.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Run `scripts/devtools/install-coderankembed.sh` (idempotent;
#      downloads ~138 MB on first run, instant thereafter) so the
#      model + tokenizer are present at `ml/models/coderankembed/`.
#   3. Verify the frozen selector lives at the module root of
#      `crates/ucil-embeddings/src/models.rs` per DEC-0007 —
#      `fn test_coderankembed_inference` must be at top level (NOT
#      inside `mod tests {}`); the frozen
#      `feature-list.json:P2-W8-F02.acceptance_tests[0].selector`
#      is `-p ucil-embeddings models::test_coderankembed_inference`.
#   4. Run `cargo test -p ucil-embeddings
#      models::test_coderankembed_inference -- --nocapture` and tee
#      the output through the cargo-test / cargo-nextest summary
#      regex (shape established in WO-0042 onwards).
#   5. On success print `[OK] P2-W8-F02` and exit 0; on any failure
#      print `[FAIL] P2-W8-F02: <reason>` and exit 1, including the
#      specific assertion line on test failure.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0059-test.log"
MODEL_DIR="ml/models/coderankembed"

# ── Step 1: ensure the real model + tokenizer are installed ───────────
if [ ! -s "${MODEL_DIR}/model.onnx" ] || [ ! -s "${MODEL_DIR}/tokenizer.json" ]; then
    echo "[INFO] P2-W8-F02: model artefacts missing; running install-coderankembed.sh..."
    if ! bash scripts/devtools/install-coderankembed.sh; then
        rc=$?
        echo "[FAIL] P2-W8-F02: install-coderankembed.sh exited ${rc}" >&2
        echo "[HINT] P2-W8-F02: re-run with verbose output via \`bash -x \
scripts/devtools/install-coderankembed.sh\` to inspect the curl/sha256 step \
that failed (master-plan §18 Phase 2 Week 8 line 1787)." >&2
        exit 1
    fi
fi

# ── Step 2: confirm the frozen selector lives at module root ──────────
SELECTOR_FILE="crates/ucil-embeddings/src/models.rs"
if ! grep -nE '^fn test_coderankembed_inference|^pub fn test_coderankembed_inference' \
        "${SELECTOR_FILE}" > /dev/null; then
    echo "[FAIL] P2-W8-F02: frozen selector \`fn test_coderankembed_inference\` \
not found at module root of ${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W8-F02: per DEC-0007, the test must be at module root \
(NOT inside \`mod tests {}\`).  See WO-0059 + master-plan §18 Phase 2 Week 8 \
line 1787." >&2
    exit 1
fi

# ── Step 3: run the frozen acceptance selector ────────────────────────
echo "[INFO] P2-W8-F02: running cargo test models::test_coderankembed_inference..."
if ! cargo test -p ucil-embeddings \
        models::test_coderankembed_inference \
        -- --nocapture 2>&1 | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W8-F02: cargo test models::test_coderankembed_inference \
exited non-zero — see ${TEST_LOG}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P2-W8-F02: see panic line above for the failing \
assertion (check ${MODEL_DIR}/{model.onnx,tokenizer.json} integrity + \
master-plan §18 Phase 2 Week 8 line 1787)" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. .* 0 failed|[0-9]+ tests? passed' \
        "${TEST_LOG}"; then
    echo "[FAIL] P2-W8-F02: cargo test summary line missing in ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Step 4: optional shellcheck self-lint ─────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W8-F02.sh"; then
        echo "[FAIL] P2-W8-F02: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W8-F02: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W8-F02"
exit 0
