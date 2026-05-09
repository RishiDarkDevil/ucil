#!/usr/bin/env bash
# Acceptance test for P3-W10-F04 — G5 (Context) parallel-execution
# orchestrator + PageRank-ranked, session-deduped assembler
# (master-plan §5.5 lines 502-522).  Authored under WO-0091.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Verify the frozen public symbols + frozen selector live at
#      module root of `crates/ucil-daemon/src/g5.rs` and the test at
#      `crates/ucil-daemon/src/executor.rs` (rename-drift guards).
#   3. Run the frozen-selector test
#      `cargo test -p ucil-daemon executor::test_g5_context_assembly
#       --no-fail-fast 2>&1 | tee /tmp/p3-w10-f04-test.log` and grep
#      the cargo summary regex.
#   4. On success print
#      `[PASS] P3-W10-F04: G5 context-assembly frozen test green` and
#      exit 0; on any failure print
#      `[FAIL] P3-W10-F04: <reason>` and exit 1.
#
# No substitute impls.  No env-gated short-circuits — the test is
# fully synthetic (`TestG5Source` local impls per `DEC-0008` §4; no
# subprocess, no API key).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TEST_LOG="/tmp/p3-w10-f04-test.log"
G5_FILE="crates/ucil-daemon/src/g5.rs"
EXECUTOR_FILE="crates/ucil-daemon/src/executor.rs"

# ── Step 1: rename-drift guards on frozen public symbols in g5.rs ─────
if ! grep -qE 'pub trait G5Source' "${G5_FILE}"; then
    echo "[FAIL] P3-W10-F04: frozen symbol \`pub trait G5Source\` \
not found in ${G5_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub async fn execute_g5' "${G5_FILE}"; then
    echo "[FAIL] P3-W10-F04: frozen symbol \`pub async fn execute_g5\` \
not found in ${G5_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub fn assemble_g5_context' "${G5_FILE}"; then
    echo "[FAIL] P3-W10-F04: frozen symbol \`pub fn assemble_g5_context\` \
not found in ${G5_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub const G5_MASTER_DEADLINE.*Duration::from_millis\(5_000\)' \
        "${G5_FILE}"; then
    echo "[FAIL] P3-W10-F04: frozen constant \
\`G5_MASTER_DEADLINE: Duration = Duration::from_millis(5_000)\` \
not found in ${G5_FILE}" >&2
    exit 1
fi
if ! grep -qE 'pub const G5_PER_SOURCE_DEADLINE.*Duration::from_millis\(4_500\)' \
        "${G5_FILE}"; then
    echo "[FAIL] P3-W10-F04: frozen constant \
\`G5_PER_SOURCE_DEADLINE: Duration = Duration::from_millis(4_500)\` \
not found in ${G5_FILE}" >&2
    exit 1
fi
if ! grep -qE '#\[tracing::instrument\(name = "ucil\.group\.context"' \
        "${G5_FILE}"; then
    echo "[FAIL] P3-W10-F04: frozen tracing span \
\`ucil.group.context\` not found in ${G5_FILE}" >&2
    exit 1
fi

# ── Step 2: frozen-selector grep — test_g5_context_assembly ───────────
#
# Per `DEC-0007`, the test must live at module root (NOT inside `mod
# tests {}`) so the substring selector
# `executor::test_g5_context_assembly` resolves cleanly without a
# `tests::` intermediate.  Pattern tolerates `pub`/`async` modifiers
# (per WO-0068 lessons 'For planner' #2).
if ! grep -qE '^[[:space:]]*(pub )?(async )?fn test_g5_context_assembly\(\)' \
        "${EXECUTOR_FILE}"; then
    echo "[FAIL] P3-W10-F04: frozen selector \
\`fn test_g5_context_assembly\` not found at module root of \
${EXECUTOR_FILE}" >&2
    echo "[HINT] P3-W10-F04: per DEC-0007, the test must be at module \
root (NOT inside \`mod tests {}\`)" >&2
    exit 1
fi

# ── Step 3: production-side word-ban scrub on g5.rs ───────────────────
if grep -niE 'mock|fake|stub' "${G5_FILE}"; then
    echo "[FAIL] P3-W10-F04: production-side word-ban grep \
(mock|fake|stub) returned matches in ${G5_FILE}" >&2
    exit 1
fi

# ── Step 4: run the frozen acceptance selector ────────────────────────
echo "[INFO] P3-W10-F04: running cargo test \
executor::test_g5_context_assembly..."
if ! cargo test -p ucil-daemon executor::test_g5_context_assembly \
        --no-fail-fast 2>&1 \
        | tee "${TEST_LOG}" >/dev/null; then
    echo "[FAIL] P3-W10-F04: cargo test \
executor::test_g5_context_assembly exited non-zero — \
see ${TEST_LOG}" >&2
    if grep -nE 'panicked at|assertion[ ]+failed' "${TEST_LOG}" >&2; then
        echo "[HINT] P3-W10-F04: see panic line above for the failing \
sub-assertion (SA1..SA8)" >&2
    fi
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi
if ! grep -qE '^test executor::test_g5_context_assembly \.\.\. ok$' \
        "${TEST_LOG}"; then
    echo "[FAIL] P3-W10-F04: expected line \
\`test executor::test_g5_context_assembly ... ok\` missing in \
${TEST_LOG}" >&2
    tail -40 "${TEST_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck the script itself ────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P3-W10-F04.sh"; then
        echo "[FAIL] P3-W10-F04: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P3-W10-F04: shellcheck not on PATH; skipping lint."
fi

echo "[PASS] P3-W10-F04: G5 context-assembly frozen test green"
exit 0
