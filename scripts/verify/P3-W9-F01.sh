#!/usr/bin/env bash
# Acceptance test for P3-W9-F01 — deterministic-fallback classifier
# (master-plan §6.2 lines 643-658 + §3.2 lines 211-237 + §7.1 lines
# 693-695).  Authored under WO-0067.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen selector + frozen public symbols live in
#      `crates/ucil-core/src/fusion.rs` (rename-drift guards).
#   3. Run the frozen-selector test
#      `cargo test -p ucil-core fusion::test_deterministic_classifier
#       --no-fail-fast` and tee the output through the cargo summary
#      regex (shape established in WO-0038/0039/0042/0043/0044/0045/
#      0046/0047/0048/0056/0066).
#   4. On success print `[OK] P3-W9-F01 deterministic classifier
#      wired and verified` and exit 0; on any failure print
#      `[FAIL] P3-W9-F01: <reason>` and exit 1.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0067-f01-test.log"
SELECTOR_FILE="crates/ucil-core/src/fusion.rs"

# ── Step 1: rename-drift guards on frozen public symbols ──────────────
if ! grep -qE 'pub enum QueryType' "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F01: frozen symbol \`pub enum QueryType\` not \
found in ${SELECTOR_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub fn classify_query' "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F01: frozen symbol \`pub fn classify_query\` \
not found in ${SELECTOR_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub (const )?fn group_weights_for' "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F01: frozen symbol \`pub fn group_weights_for\` \
not found in ${SELECTOR_FILE}" >&2
    exit 1
fi

# ── Step 2: frozen-selector grep — test_deterministic_classifier ──────
if ! grep -qE '^[[:space:]]*fn test_deterministic_classifier\(\)' \
        "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F01: frozen selector \`fn \
test_deterministic_classifier\` not found at module root of \
${SELECTOR_FILE}" >&2
    echo "[HINT] P3-W9-F01: per DEC-0007, the test must be at module \
root (NOT inside \`mod tests {}\`)" >&2
    exit 1
fi

# ── Step 3: run the frozen acceptance selector ────────────────────────
echo "[INFO] P3-W9-F01: running cargo test fusion::test_deterministic_classifier..."
if ! cargo test -p ucil-core fusion::test_deterministic_classifier \
        --no-fail-fast -- --nocapture 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P3-W9-F01: cargo test \
fusion::test_deterministic_classifier exited non-zero — see \
${TEST_LOG}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P3-W9-F01: see panic line above for the failing \
sub-assertion (SA1..SA6)" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. .* 0 failed|[0-9]+ tests? passed' \
        "${TEST_LOG}"; then
    echo "[FAIL] P3-W9-F01: cargo test summary line missing in \
${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself ────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P3-W9-F01.sh"; then
        echo "[FAIL] P3-W9-F01: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P3-W9-F01: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P3-W9-F01 deterministic classifier wired and verified"
exit 0
