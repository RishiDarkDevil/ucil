#!/usr/bin/env bash
# Acceptance test for P2-W8-F05 — embedding chunker
# (master-plan §12.2 line 1339 — "Chunking: AST-aware via tree-sitter.
# Each chunk is a complete function/method/class.  Never split
# mid-function.  Max 512 tokens.  Larger functions: signature +
# first-paragraph doc comment.").  Authored under WO-0060.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen selector lives at the module root of
#      `crates/ucil-embeddings/src/chunker.rs` per DEC-0007 —
#      `fn test_embedding_chunker_real_fixture` must be at top level
#      (NOT inside `mod tests {}`); the frozen
#      `feature-list.json:P2-W8-F05.acceptance_tests[0].selector`
#      is `-p ucil-embeddings chunker::`.
#   3. Run `cargo test -p ucil-embeddings chunker:: -- --nocapture`
#      and tee the output through the cargo-test / cargo-nextest
#      summary regex (shape established in WO-0042 onwards).
#   4. On success print `[OK] P2-W8-F05` and exit 0; on any failure
#      print `[FAIL] P2-W8-F05: <reason>` and exit 1, including the
#      specific assertion line on test failure.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0060-test.log"
SELECTOR_FILE="crates/ucil-embeddings/src/chunker.rs"
FIXTURE_FILE="crates/ucil-embeddings/tests/data/sample.rs"

# ── Step 1: confirm the bundled fixture is present ────────────────────
if [ ! -s "${FIXTURE_FILE}" ]; then
    echo "[FAIL] P2-W8-F05: bundled fixture missing at ${FIXTURE_FILE}" >&2
    echo "[HINT] P2-W8-F05: see WO-0060 scope_in step adding \
${FIXTURE_FILE} (a Rust source fixture with 3-5 functions). \
Master-plan §12.2 line 1339." >&2
    exit 1
fi

# ── Step 2: confirm the frozen selector lives at module root ──────────
if ! grep -nE '^fn test_embedding_chunker_real_fixture' \
        "${SELECTOR_FILE}" > /dev/null; then
    echo "[FAIL] P2-W8-F05: frozen selector \`fn test_embedding_chunker_real_fixture\` \
not found at module root of ${SELECTOR_FILE}" >&2
    echo "[HINT] P2-W8-F05: per DEC-0007, the test must be at module root \
(NOT inside \`mod tests {}\`).  See WO-0060 + master-plan §12.2 line 1339." >&2
    exit 1
fi

# ── Step 3: run the frozen acceptance selector ────────────────────────
echo "[INFO] P2-W8-F05: running cargo test -p ucil-embeddings chunker::..."
if ! cargo test -p ucil-embeddings \
        chunker:: \
        -- --nocapture 2>&1 | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W8-F05: cargo test chunker:: exited non-zero — see ${TEST_LOG}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P2-W8-F05: see panic line above for the failing \
assertion (check ${FIXTURE_FILE} integrity + master-plan §12.2 line \
1339 + WO-0060 acceptance criteria)" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. .* 0 failed|[0-9]+ tests? passed' \
        "${TEST_LOG}"; then
    echo "[FAIL] P2-W8-F05: cargo test summary line missing in ${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Step 4: optional shellcheck self-lint ─────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W8-F05.sh"; then
        echo "[FAIL] P2-W8-F05: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W8-F05: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W8-F05"
exit 0
