#!/usr/bin/env bash
# Acceptance test for P2-W8-F01 — `ucil-embeddings` ONNX Runtime session
# (master-plan §18 Phase 2 Week 8 line 1786 — "ucil-embeddings crate:
# ONNX Runtime (ort crate) inference").  Authored under WO-0058
# (supersedes the abandoned WO-0054 — module name corrected from
# `ort_session.rs` to `onnx_inference.rs` to match the frozen
# feature-list selector).
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen selector lives at the module root of
#      `crates/ucil-embeddings/src/onnx_inference.rs` per DEC-0007 —
#      `fn test_onnx_session_loads_minimal_model` must be at top
#      level (not inside `mod tests {}`).
#   3. Verify the bundled `minimal.onnx` fixture exists and is non-empty.
#   4. Run `cargo test -p ucil-embeddings
#      onnx_inference::test_onnx_session_loads_minimal_model --
#      --nocapture` and tee the output through the cargo-test /
#      cargo-nextest summary regex (shape established in WO-0042
#      onwards).
#   5. On success print `[OK] P2-W8-F01` and exit 0; on any failure
#      print `[FAIL] P2-W8-F01: <reason>` and exit 1, including the
#      specific assertion line on test failure so operators can jump
#      straight to the panic without re-running.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0058-test.log"

# ── Step 1: confirm the frozen selector lives at module root ──────────
SELECTOR_FILE="crates/ucil-embeddings/src/onnx_inference.rs"
if ! grep -nE '^fn test_onnx_session_loads_minimal_model|^pub fn test_onnx_session_loads_minimal_model' \
        "${SELECTOR_FILE}" > /dev/null; then
    echo "[FAIL] P2-W8-F01: frozen selector \`fn \
test_onnx_session_loads_minimal_model\` not found at module root of \
${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W8-F01: per DEC-0007, the test must be at module \
root (NOT inside \`mod tests {}\`).  See WO-0058 + master-plan §18 \
Phase 2 Week 8 line 1786." >&2
    exit 1
fi

# ── Step 2: confirm the minimal.onnx fixture is present ────────────────
FIXTURE_FILE="crates/ucil-embeddings/tests/data/minimal.onnx"
if [ ! -s "${FIXTURE_FILE}" ]; then
    echo "[FAIL] P2-W8-F01: required fixture ${FIXTURE_FILE} is \
missing or empty" >&2
    echo "[HINT] P2-W8-F01: regenerate via \`uv pip install onnx && \
python3 crates/ucil-embeddings/tests/data/build_minimal_onnx.py\`" >&2
    exit 1
fi

# ── Step 3: run the frozen acceptance selector ────────────────────────
echo "[INFO] P2-W8-F01: running cargo test \
onnx_inference::test_onnx_session_loads_minimal_model..."
if ! cargo test -p ucil-embeddings \
        onnx_inference::test_onnx_session_loads_minimal_model \
        -- --nocapture 2>&1 | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W8-F01: cargo test \
onnx_inference::test_onnx_session_loads_minimal_model exited \
non-zero — see ${TEST_LOG}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P2-W8-F01: see panic line above for the failing \
assertion (check ${FIXTURE_FILE} integrity + master-plan §18 Phase 2 \
Week 8 line 1786)" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. .* 0 failed|[0-9]+ tests? passed' \
        "${TEST_LOG}"; then
    echo "[FAIL] P2-W8-F01: cargo test summary line missing in \
${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself ─────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W8-F01.sh"; then
        echo "[FAIL] P2-W8-F01: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W8-F01: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W8-F01"
exit 0
