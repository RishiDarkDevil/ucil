#!/usr/bin/env bash
# P2-W8-F03 verify — Qwen3-Embedding GPU upgrade config gate.
set -euo pipefail
# Master-plan §4.2 line 303 + §18 Phase 2 Week 8 line 1787; WO-0062.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen selector lives at the module root of
#      `crates/ucil-embeddings/src/models.rs` per DEC-0007 —
#      `fn test_qwen3_config_gate` must be at top level (NOT inside
#      `mod tests {}` or `mod qwen3_tests {}`); the frozen
#      `feature-list.json:P2-W8-F03.acceptance_tests[0].selector`
#      is `-p ucil-embeddings models::test_qwen3_config_gate`.
#   3. Run `cargo test -p ucil-embeddings
#      models::test_qwen3_config_gate -- --nocapture` and tee the
#      output through the cargo-test / cargo-nextest summary regex
#      (shape established in WO-0042 onwards).
#   4. Run the supporting negative-path coverage block
#      (`models::qwen3_tests::*` + `config::config_tests::*`) and
#      confirm `0 failed`.
#   5. On success print `[OK] P2-W8-F03 qwen3 config gate verified`
#      and exit 0; on any failure print
#      `[FAIL] P2-W8-F03: <reason>` and exit 1.
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0062-test.log"
COVERAGE_LOG="/tmp/wo-0062-coverage.log"

# ── Step 1: confirm the frozen selector lives at module root ──────────
SELECTOR_FILE="crates/ucil-embeddings/src/models.rs"
if ! grep -nE '^(pub )?fn test_qwen3_config_gate' "${SELECTOR_FILE}" \
        > /dev/null; then
    echo "[FAIL] P2-W8-F03: frozen selector \`fn test_qwen3_config_gate\` \
not found at module root of ${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W8-F03: per DEC-0007, the test must be at module root \
(NOT inside \`mod tests {}\` or \`mod qwen3_tests {}\`).  See WO-0062 + \
master-plan §4.2 line 303 + §18 Phase 2 Week 8 line 1787." >&2
    exit 1
fi

# ── Step 2: run the frozen acceptance selector ────────────────────────
echo "[INFO] P2-W8-F03: running cargo test models::test_qwen3_config_gate..."
if ! cargo test -p ucil-embeddings \
        models::test_qwen3_config_gate \
        -- --nocapture 2>&1 | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W8-F03: cargo test models::test_qwen3_config_gate \
exited non-zero — see ${TEST_LOG}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P2-W8-F03: see panic line above for the failing \
sub-assertion (SA1-SA12); check the workspace ort dep + Matryoshka \
bounds (32-7168 inclusive) + master-plan §4.2 line 303 + §17.6 lines \
2026-2030." >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. .* 0 failed|[0-9]+ tests? passed' \
        "${TEST_LOG}"; then
    echo "[FAIL] P2-W8-F03: cargo test summary line missing in \
${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Step 3: run the negative-path coverage block (qwen3_tests) ────────
echo "[INFO] P2-W8-F03: running cargo test models::qwen3_tests..."
if ! cargo test -p ucil-embeddings models::qwen3_tests \
        -- --nocapture 2>&1 | tee "${COVERAGE_LOG}" >/dev/null; then
    echo "[FAIL] P2-W8-F03: cargo test for qwen3_tests exited non-zero \
— see ${COVERAGE_LOG}" >&2
    tail -40 "${COVERAGE_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq '0 failed' "${COVERAGE_LOG}"; then
    echo "[FAIL] P2-W8-F03: '0 failed' marker missing in qwen3_tests run \
— see ${COVERAGE_LOG}" >&2
    tail -40 "${COVERAGE_LOG}" >&2 || true
    exit 1
fi

# ── Step 3b: run the negative-path coverage block (config_tests) ──────
echo "[INFO] P2-W8-F03: running cargo test config::config_tests..."
if ! cargo test -p ucil-embeddings config::config_tests \
        -- --nocapture 2>&1 | tee -a "${COVERAGE_LOG}" >/dev/null; then
    echo "[FAIL] P2-W8-F03: cargo test for config_tests exited non-zero \
— see ${COVERAGE_LOG}" >&2
    tail -40 "${COVERAGE_LOG}" >&2 || true
    exit 1
fi
if ! grep -cE '0 failed' "${COVERAGE_LOG}" | grep -qE '^[2-9]'; then
    echo "[FAIL] P2-W8-F03: '0 failed' marker missing in config_tests \
run — see ${COVERAGE_LOG}" >&2
    tail -40 "${COVERAGE_LOG}" >&2 || true
    exit 1
fi

# ── Step 4: optional shellcheck self-lint ─────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W8-F03.sh"; then
        echo "[FAIL] P2-W8-F03: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W8-F03: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W8-F03 qwen3 config gate verified"
exit 0
