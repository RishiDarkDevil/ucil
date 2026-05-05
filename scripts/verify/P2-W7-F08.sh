#!/usr/bin/env bash
# Acceptance verify script for P2-W7-F08 (WO-0055 / DEC-0014):
# SCIP P1 install — scip-rust + scip CLI produce a cross-repo symbol
# index for the fixture rust-project; index loaded into SQLite and
# queried via G1.
#
# Master-plan §3 line 284 (CLI → SQLite frozen classification),
# §18 Phase 2 Week 7 line 1782 (verbatim feature description),
# §28 phase-log external-deps line (scip-rust + scip binary install
# prerequisite).
#
# Mirrors the WO-0051 P2-W7-F07 ripgrep verify-script shape:
#   1. detect external binaries on PATH (operator-actionable hints)
#   2. print binary versions
#   3. run the cargo test with the alternation regex check
#   4. run a forensic `scip print --json` against a fresh fixture
#      index and assert the JSON parses
#   5. exit 0 with [OK] P2-W7-F08
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# ── Step 1: detect scip-rust + scip on PATH ──────────────────────────────────
if ! command -v scip-rust >/dev/null 2>&1; then
    echo "[FAIL] P2-W7-F08: scip-rust not on PATH; run scripts/devtools/install-scip-rust.sh"
    exit 1
fi
if ! command -v scip >/dev/null 2>&1; then
    echo "[FAIL] P2-W7-F08: scip not on PATH; run scripts/devtools/install-scip.sh"
    exit 1
fi

# ── Step 2: print binary versions ────────────────────────────────────────────
scip-rust --version
scip --version

# ── Step 3: cargo test with alternation regex ────────────────────────────────
# The `--lib` filter restricts to the unit-test binary so the selector
# resolves to `ucil_daemon::scip::test_scip_p1_install` directly. The
# alternation regex covers both classic `cargo test` summary lines and
# `cargo nextest` summary lines per the WO-0038/0039/0042/0043/0051
# convention.
cargo test -p ucil-daemon --lib scip::test_scip_p1_install 2>&1 | tee /tmp/wo-0055-cargo.log
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' /tmp/wo-0055-cargo.log; then
    echo "[FAIL] P2-W7-F08: cargo test scip::test_scip_p1_install"
    exit 1
fi

# ── Step 4: forensic scip print --json smoke ─────────────────────────────────
# Produces a fresh index against the fixture rust-project and confirms
# the `scip` forensic CLI can decode and emit JSON over the binary
# protobuf payload. This catches regressions where the indexer +
# decoder happen to disagree (e.g. mismatched scip protobuf schema
# versions).
SCIP_OUT_DIR="/tmp/wo-0055-fixture-scip"
SCIP_OUT_FILE="${SCIP_OUT_DIR}/index.scip"
SCIP_JSON_FILE="/tmp/wo-0055-print.json"
rm -rf "${SCIP_OUT_DIR}"
mkdir -p "${SCIP_OUT_DIR}"
(cd tests/fixtures/rust-project && scip-rust index --output "${SCIP_OUT_FILE}") >/dev/null 2>&1 || {
    echo "[FAIL] P2-W7-F08: scip-rust index against fixture rust-project failed"
    exit 1
}
if [[ ! -s "${SCIP_OUT_FILE}" ]]; then
    echo "[FAIL] P2-W7-F08: scip-rust did not produce a non-empty ${SCIP_OUT_FILE}"
    exit 1
fi
scip print --json "${SCIP_OUT_FILE}" >"${SCIP_JSON_FILE}" 2>/dev/null || {
    echo "[FAIL] P2-W7-F08: scip print --json failed"
    exit 1
}
if [[ ! -s "${SCIP_JSON_FILE}" ]]; then
    echo "[FAIL] P2-W7-F08: scip print --json produced empty output"
    exit 1
fi
python3 -c 'import json,sys; json.load(open("'"${SCIP_JSON_FILE}"'"))' || {
    echo "[FAIL] P2-W7-F08: scip print --json output is not valid JSON"
    exit 1
}

# ── Step 5: success ──────────────────────────────────────────────────────────
echo "[OK] P2-W7-F08"
exit 0
