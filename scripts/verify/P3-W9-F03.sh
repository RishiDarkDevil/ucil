#!/usr/bin/env bash
# Acceptance test for P3-W9-F03 — cross-group parallel-execution
# orchestrator (master-plan §6.1 lines 585-641 + line 606).  Authored
# under WO-0068.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen public symbols + frozen selector live at
#      module root of `crates/ucil-core/src/cross_group.rs`
#      (rename-drift guards per scope_in #35).
#   3. Run the frozen-selector test
#      `cargo test -p ucil-core cross_group::test_cross_group_parallel_execution
#       --no-fail-fast -- --nocapture` and tee the output through the
#      cargo summary regex (shape established in
#      WO-0038/0039/0042/0043/0044/0045/0046/0047/0048/0056/0066/0067).
#   4. On success print `[OK] P3-W9-F03 cross-group parallel executor
#      wired and verified` and exit 0; on any failure print
#      `[FAIL] P3-W9-F03: <reason>` and exit 1.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/wo-0068-f03-test.log"
SELECTOR_FILE="crates/ucil-core/src/cross_group.rs"

# ── Step 1: rename-drift guards on frozen public symbols ──────────────
if ! grep -qE 'pub async fn execute_cross_group' "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F03: frozen symbol \`pub async fn execute_cross_group\` \
not found in ${SELECTOR_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub trait GroupExecutor' "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F03: frozen symbol \`pub trait GroupExecutor\` \
not found in ${SELECTOR_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub enum Group' "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F03: frozen symbol \`pub enum Group\` not found \
in ${SELECTOR_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub struct CrossGroupExecution' "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F03: frozen symbol \`pub struct CrossGroupExecution\` \
not found in ${SELECTOR_FILE}" >&2
    exit 1
fi
if ! grep -qE 'CROSS_GROUP_MASTER_DEADLINE.*Duration::from_millis\(5_000\)' \
        "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F03: frozen constant \
\`CROSS_GROUP_MASTER_DEADLINE: Duration = Duration::from_millis(5_000)\` \
not found in ${SELECTOR_FILE}" >&2
    exit 1
fi

# ── Step 2: frozen-selector grep — test_cross_group_parallel_execution ──
if ! grep -qE '^[[:space:]]*(pub )?(async )?fn test_cross_group_parallel_execution\(\)' \
        "${SELECTOR_FILE}"; then
    echo "[FAIL] P3-W9-F03: frozen selector \
\`fn test_cross_group_parallel_execution\` not found at module root \
of ${SELECTOR_FILE}" >&2
    echo "[HINT] P3-W9-F03: per DEC-0007, the test must be at module \
root (NOT inside \`mod tests {}\`)" >&2
    exit 1
fi

# ── Step 3: run the frozen acceptance selector ────────────────────────
echo "[INFO] P3-W9-F03: running cargo test \
cross_group::test_cross_group_parallel_execution..."
if ! cargo test -p ucil-core cross_group::test_cross_group_parallel_execution \
        --no-fail-fast -- --nocapture 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P3-W9-F03: cargo test \
cross_group::test_cross_group_parallel_execution exited non-zero — \
see ${TEST_LOG}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P3-W9-F03: see panic line above for the failing \
sub-assertion (SA1..SA7)" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. .* 0 failed|[0-9]+ tests? passed' \
        "${TEST_LOG}"; then
    echo "[FAIL] P3-W9-F03: cargo test summary line missing in \
${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself ────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P3-W9-F03.sh"; then
        echo "[FAIL] P3-W9-F03: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P3-W9-F03: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P3-W9-F03 cross-group parallel executor wired and verified"
exit 0
