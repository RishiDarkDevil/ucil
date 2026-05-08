#!/usr/bin/env bash
# Acceptance test for P3-W9-F07 — G3 (Knowledge) parallel-execution
# orchestrator + entity-keyed temporal-priority merger (master-plan
# §5.3 lines 469-479).  Authored under WO-0070.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen public symbols + frozen selector live at
#      module root of `crates/ucil-daemon/src/g3.rs` and the test at
#      `crates/ucil-daemon/src/executor.rs` (rename-drift guards).
#   3. Run the frozen-selector test
#      `cargo test -p ucil-daemon executor::test_g3_parallel_merge
#       --no-fail-fast 2>&1 | tee /tmp/p3-w9-f07-test.log` and grep
#      the cargo summary regex.
#   4. On success print
#      `[PASS] P3-W9-F07: G3 parallel-merge frozen test green` and
#      exit 0; on any failure print
#      `[FAIL] P3-W9-F07: <reason>` and exit 1.
#
# No mocks.  No env-gated short-circuits — the test is fully synthetic
# (`TestG3Source` local impls per `DEC-0008` §4; no subprocess,
# no API key).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/p3-w9-f07-test.log"
G3_FILE="crates/ucil-daemon/src/g3.rs"
EXECUTOR_FILE="crates/ucil-daemon/src/executor.rs"

# ── Step 1: rename-drift guards on frozen public symbols in g3.rs ─────
if ! grep -qE 'pub trait G3Source' "${G3_FILE}"; then
    echo "[FAIL] P3-W9-F07: frozen symbol \`pub trait G3Source\` \
not found in ${G3_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub async fn execute_g3' "${G3_FILE}"; then
    echo "[FAIL] P3-W9-F07: frozen symbol \`pub async fn execute_g3\` \
not found in ${G3_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub fn merge_g3_by_entity' "${G3_FILE}"; then
    echo "[FAIL] P3-W9-F07: frozen symbol \`pub fn merge_g3_by_entity\` \
not found in ${G3_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub const G3_MASTER_DEADLINE.*Duration::from_millis\(5_000\)' \
        "${G3_FILE}"; then
    echo "[FAIL] P3-W9-F07: frozen constant \
\`G3_MASTER_DEADLINE: Duration = Duration::from_millis(5_000)\` \
not found in ${G3_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub const G3_PER_SOURCE_DEADLINE.*Duration::from_millis\(4_500\)' \
        "${G3_FILE}"; then
    echo "[FAIL] P3-W9-F07: frozen constant \
\`G3_PER_SOURCE_DEADLINE: Duration = Duration::from_millis(4_500)\` \
not found in ${G3_FILE}" >&2
    exit 1
fi

# ── Step 2: frozen-selector grep — test_g3_parallel_merge ─────────────
#
# Per `DEC-0007`, the test must live at module root (NOT inside `mod
# tests {}`) so the substring selector
# `executor::test_g3_parallel_merge` resolves cleanly without a
# `tests::` intermediate.  Pattern tolerates `pub`/`async` modifiers
# (per WO-0068 lessons 'For planner' #2).
if ! grep -qE '^[[:space:]]*(pub )?(async )?fn test_g3_parallel_merge\(\)' \
        "${EXECUTOR_FILE}"; then
    echo "[FAIL] P3-W9-F07: frozen selector \
\`fn test_g3_parallel_merge\` not found at module root of \
${EXECUTOR_FILE}" >&2
    echo "[HINT] P3-W9-F07: per DEC-0007, the test must be at module \
root (NOT inside \`mod tests {}\`)" >&2
    exit 1
fi

# ── Step 3: run the frozen acceptance selector ────────────────────────
echo "[INFO] P3-W9-F07: running cargo test \
executor::test_g3_parallel_merge..."
if ! cargo test -p ucil-daemon executor::test_g3_parallel_merge \
        --no-fail-fast 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P3-W9-F07: cargo test \
executor::test_g3_parallel_merge exited non-zero — \
see ${TEST_LOG}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P3-W9-F07: see panic line above for the failing \
sub-assertion (SA1..SA8)" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -qE '^test executor::test_g3_parallel_merge \.\.\. ok$' \
        "${TEST_LOG}"; then
    echo "[FAIL] P3-W9-F07: expected line \
\`test executor::test_g3_parallel_merge ... ok\` missing in \
${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself ────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P3-W9-F07.sh"; then
        echo "[FAIL] P3-W9-F07: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P3-W9-F07: shellcheck not on PATH; skipping lint."
fi

echo "[PASS] P3-W9-F07: G3 parallel-merge frozen test green"
exit 0
