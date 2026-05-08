#!/usr/bin/env bash
# Acceptance test for P3-W9-F05 — codebase-memory plugin manifest
# health-check + symbol-lookup smoke against the rust-project fixture.
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Assert `npx` is on PATH; otherwise exit 1 with a clear hint
#      pointing at `scripts/devtools/install-codebase-memory-mcp.sh`.
#   3. Print npx + the pinned codebase-memory-mcp version for the
#      verifier log.
#   4. Run `cargo test -p ucil-daemon --test g3_plugin_manifests
#      g3_plugin_manifests::codebase_memory_manifest_health_check` and
#      require the cargo-test summary line "1 passed; 0 failed" or
#      the cargo-nextest equivalent — alternation regex per
#      WO-0042 / WO-0043 / WO-0044.
#   5. Tool-level symbol-lookup smoke (gated by
#      ${UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E:-0}):
#        a. mktemp -d for the CBM_CACHE_DIR so we never pollute the
#           operator's real ~/.cache/codebase-memory-mcp.
#        b. Index tests/fixtures/rust-project via
#           `codebase-memory-mcp cli index_repository` (mode=fast for
#           a small fixture) and capture the auto-derived project
#           name from the JSON response.
#        c. Issue `codebase-memory-mcp cli search_graph` with
#           query=evaluate against the indexed project; assert the
#           response is JSON with a non-empty `results[]` array AND
#           that the first result's `name` field equals "evaluate"
#           (proves search_graph actually returned the symbol from
#           tests/fixtures/rust-project/src/util.rs:128).
#        d. Clean up the tmpdir.
#   6. On all-green prints `[OK] P3-W9-F05` and exits 0; on any
#      failure prints `[FAIL] P3-W9-F05: <reason>` and exits 1.
#
# This script is read-only against the fixture: it never modifies
# tests/fixtures/rust-project (forbidden_paths in WO-0069).
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

PINNED_NPM_VERSION="0.6.1"
PINNED_PKG_SPEC="codebase-memory-mcp@${PINNED_NPM_VERSION}"

# ── Prereq: npx on PATH ────────────────────────────────────────────────
if ! command -v npx >/dev/null 2>&1; then
    echo "[FAIL] P3-W9-F05: npx not on PATH." >&2
    echo "  See scripts/devtools/install-codebase-memory-mcp.sh for install hints." >&2
    exit 1
fi
echo "[INFO] P3-W9-F05: npx version: $(npx --version)"
echo "[INFO] P3-W9-F05: codebase-memory-mcp pinned: ${PINNED_PKG_SPEC}"

# ── Step 1: integration test (real subprocess, real JSON-RPC) ─────────
CARGO_LOG="/tmp/wo-0069-f05-cargo.log"
echo "[INFO] P3-W9-F05: running cargo test g3_plugin_manifests::codebase_memory_manifest_health_check..."
if ! cargo test -p ucil-daemon --test g3_plugin_manifests \
        g3_plugin_manifests::codebase_memory_manifest_health_check 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P3-W9-F05: cargo test exited non-zero — see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests run: 1 passed' "${CARGO_LOG}"; then
    echo "[FAIL] P3-W9-F05: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W9-F05: integration test PASS."

# ── Step 2: tool-level symbol-lookup smoke ────────────────────────────
if [[ "${UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E:-0}" == "1" ]]; then
    echo "[SKIP] P3-W9-F05: tool-level smoke (UCIL_SKIP_KNOWLEDGE_PLUGIN_E2E=1)."
    echo "[OK] P3-W9-F05"
    exit 0
fi

CBM_TMPDIR="$(mktemp -d -t wo-0069-f05-cbm-XXXXXX)"
trap 'rm -rf "${CBM_TMPDIR}"' EXIT
export CBM_CACHE_DIR="${CBM_TMPDIR}"

INDEX_LOG="/tmp/wo-0069-f05-index.log"
SEARCH_LOG="/tmp/wo-0069-f05-search.log"

echo "[INFO] P3-W9-F05: indexing tests/fixtures/rust-project into ephemeral cache ${CBM_TMPDIR}..."
INDEX_PAYLOAD="{\"repo_path\":\"${REPO_ROOT}/tests/fixtures/rust-project\",\"mode\":\"fast\"}"
if ! npx -y "${PINNED_PKG_SPEC}" cli index_repository "${INDEX_PAYLOAD}" >"${INDEX_LOG}" 2>&1; then
    echo "[FAIL] P3-W9-F05: index_repository exited non-zero - see ${INDEX_LOG}" >&2
    tail -20 "${INDEX_LOG}" >&2 || true
    exit 1
fi

# Last line of the index_repository CLI output is the JSON envelope;
# stderr (level=info ...) is folded in via 2>&1 above so we tail -n 1
# to extract the JSON. The auto-derived project name is determined by
# the absolute path of the fixture, so we extract it from the JSON
# rather than hard-coding it.
PROJECT_NAME="$(tail -n 1 "${INDEX_LOG}" \
    | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["project"])' 2>/dev/null \
    || true)"
if [[ -z "${PROJECT_NAME}" ]]; then
    echo "[FAIL] P3-W9-F05: could not extract project name from index_repository response - see ${INDEX_LOG}" >&2
    tail -20 "${INDEX_LOG}" >&2 || true
    exit 1
fi
echo "[INFO] P3-W9-F05: indexed project: ${PROJECT_NAME}"

echo "[INFO] P3-W9-F05: invoking search_graph for 'evaluate'..."
SEARCH_PAYLOAD="{\"project\":\"${PROJECT_NAME}\",\"query\":\"evaluate\",\"limit\":5}"
if ! npx -y "${PINNED_PKG_SPEC}" cli search_graph "${SEARCH_PAYLOAD}" >"${SEARCH_LOG}" 2>&1; then
    echo "[FAIL] P3-W9-F05: search_graph exited non-zero - see ${SEARCH_LOG}" >&2
    tail -20 "${SEARCH_LOG}" >&2 || true
    exit 1
fi

# Validate the JSON response shape: results[] must be non-empty AND
# the first result's name field must equal "evaluate" (proves the
# graph search actually returned the symbol from
# tests/fixtures/rust-project/src/util.rs:128 rather than a benign
# empty-results envelope).
if ! tail -n 1 "${SEARCH_LOG}" \
    | python3 -c '
import json, sys
d = json.load(sys.stdin)
results = d.get("results") or []
if not results:
    sys.stderr.write("results[] is empty\n")
    sys.exit(1)
first = results[0]
name = first.get("name")
if name != "evaluate":
    sys.stderr.write("first result name != evaluate: " + repr(name) + "\n")
    sys.exit(1)
print("OK: results=" + str(len(results)) + " first=" + str(name) + " file=" + str(first.get("file_path")) + ":" + str(first.get("start_line")))
'; then
    echo "[FAIL] P3-W9-F05: search_graph response did not carry the expected evaluate symbol - see ${SEARCH_LOG}" >&2
    tail -20 "${SEARCH_LOG}" >&2 || true
    exit 1
fi

SEARCH_BYTES="$(wc -c < "${SEARCH_LOG}")"
echo "[INFO] P3-W9-F05: search_graph returned ${SEARCH_BYTES} bytes containing the evaluate symbol."

echo "[OK] P3-W9-F05"
exit 0
