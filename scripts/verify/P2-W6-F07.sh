#!/usr/bin/env bash
# Acceptance test for P2-W6-F07 — `ucil plugin` CLI subcommand tree
# (master-plan §16 line 1580: list | install | uninstall | enable |
# disable | reload).
#
# Behaviour:
#   1. cd to the repo root via `git rev-parse --show-toplevel`.
#   2. Build `ucil-cli` and the `mock-mcp-plugin` binary used by `reload`.
#   3. Stand up a TempDir-based plugins fixture with TWO `plugin.toml`
#      manifests (`alpha`, `beta`), both pointing at the real
#      `mock-mcp-plugin` binary so `reload` can probe a live subprocess.
#   4. End-to-end-drive each subcommand via
#      `target/debug/ucil plugin <subcommand> [name] --plugins-dir <tmp>
#      --format json` and assert the JSON shape via `jq -e`.
#   5. Verify the cumulative `<tmp>/.ucil-plugin-state.toml` reflects
#      every mutation.
#   6. Run `cargo test -p ucil-cli commands::plugin::` and require the
#      cargo-test / cargo-nextest summary line via the alternation
#      regex established in WO-0038/0039/0042/0043/0044.
#   7. On success print `[OK] P2-W6-F07` and exit 0; on any failure
#      print `[FAIL] P2-W6-F07: <reason>` and exit 1.
#
# This script never touches `tests/fixtures/**`. The fixture lives in a
# fresh TempDir cleaned up via a trap.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "${REPO_ROOT}"

TMPDIR_OWN="$(mktemp -d -t wo-0045-XXXXXXXX)"
PLUGINS_DIR="${TMPDIR_OWN}/plugins"
trap 'rm -rf "${TMPDIR_OWN}"' EXIT

LIST_LOG="/tmp/wo-0045-list.json"
RELOAD_LOG="/tmp/wo-0045-reload.json"
ENABLE_LOG="/tmp/wo-0045-enable.json"
DISABLE_LOG="/tmp/wo-0045-disable.json"
UNINSTALL_LOG="/tmp/wo-0045-uninstall.json"
BUILD_LOG="/tmp/wo-0045-build.log"
CARGO_LOG="/tmp/wo-0045-cargo.log"

# ── Prereq: jq on PATH ──────────────────────────────────────────────────
if ! command -v jq >/dev/null 2>&1; then
    echo "[FAIL] P2-W6-F07: jq not on PATH; install via 'apt install jq' or distro equivalent." >&2
    exit 1
fi

# ── Step 1: build ucil-cli + mock-mcp-plugin ───────────────────────────
echo "[INFO] P2-W6-F07: building ucil-cli + mock-mcp-plugin..."
if ! cargo build -p ucil-cli -p ucil-daemon --bin ucil --bin mock-mcp-plugin 2>&1 | tee "${BUILD_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: cargo build failed — see ${BUILD_LOG}" >&2
    tail -20 "${BUILD_LOG}" >&2 || true
    exit 1
fi

UCIL_BIN="${REPO_ROOT}/target/debug/ucil"
MOCK_BIN="${REPO_ROOT}/target/debug/mock-mcp-plugin"
if [[ ! -x "${UCIL_BIN}" ]]; then
    echo "[FAIL] P2-W6-F07: ucil binary not produced at ${UCIL_BIN}" >&2
    exit 1
fi
if [[ ! -x "${MOCK_BIN}" ]]; then
    echo "[FAIL] P2-W6-F07: mock-mcp-plugin binary not produced at ${MOCK_BIN}" >&2
    exit 1
fi

# ── Step 2: lay out plugins fixture (two manifests pointing at the mock) ──
mkdir -p "${PLUGINS_DIR}/category_a/alpha" "${PLUGINS_DIR}/category_b/beta"
for name in alpha beta; do
    case "${name}" in
        alpha) DIR="${PLUGINS_DIR}/category_a/alpha" ;;
        beta)  DIR="${PLUGINS_DIR}/category_b/beta"  ;;
    esac
    cat > "${DIR}/plugin.toml" <<EOF
[plugin]
name = "${name}"
version = "0.1.0"
description = "WO-0045 verify-script fixture"

[transport]
type = "stdio"
command = "${MOCK_BIN}"
args = []
EOF
done

# ── Step 3: drive `plugin list` (json) — expect 2 entries, both default-off ──
echo "[INFO] P2-W6-F07: running plugin list..."
if ! "${UCIL_BIN}" plugin list --plugins-dir "${PLUGINS_DIR}" --format json > "${LIST_LOG}"; then
    echo "[FAIL] P2-W6-F07: 'ucil plugin list' exited non-zero" >&2
    cat "${LIST_LOG}" >&2 || true
    exit 1
fi
if ! jq -e '.plugins | length == 2' < "${LIST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: list did not return exactly 2 plugins — see ${LIST_LOG}" >&2
    cat "${LIST_LOG}" >&2 || true
    exit 1
fi
if ! jq -e 'all(.plugins[]; .installed == false and .enabled == false)' < "${LIST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: list defaults must be installed=false enabled=false — see ${LIST_LOG}" >&2
    cat "${LIST_LOG}" >&2 || true
    exit 1
fi

# ── Step 4: drive `plugin reload alpha` — must spawn the mock and report tool_count >= 1 ──
echo "[INFO] P2-W6-F07: running plugin reload alpha (live subprocess probe)..."
if ! "${UCIL_BIN}" plugin reload alpha --plugins-dir "${PLUGINS_DIR}" --timeout-ms 10000 --format json > "${RELOAD_LOG}"; then
    echo "[FAIL] P2-W6-F07: 'ucil plugin reload alpha' exited non-zero" >&2
    cat "${RELOAD_LOG}" >&2 || true
    exit 1
fi
if ! jq -e '.tool_count >= 1' < "${RELOAD_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: reload tool_count must be >=1 — see ${RELOAD_LOG}" >&2
    cat "${RELOAD_LOG}" >&2 || true
    exit 1
fi
if ! jq -e '.status == "reloaded"' < "${RELOAD_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: reload status must be 'reloaded' — see ${RELOAD_LOG}" >&2
    cat "${RELOAD_LOG}" >&2 || true
    exit 1
fi
if ! jq -e '.installed == true' < "${RELOAD_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: reload must persist installed=true — see ${RELOAD_LOG}" >&2
    cat "${RELOAD_LOG}" >&2 || true
    exit 1
fi

# ── Step 5: drive `plugin enable alpha` ────────────────────────────────
echo "[INFO] P2-W6-F07: running plugin enable alpha..."
if ! "${UCIL_BIN}" plugin enable alpha --plugins-dir "${PLUGINS_DIR}" --format json > "${ENABLE_LOG}"; then
    echo "[FAIL] P2-W6-F07: 'ucil plugin enable alpha' exited non-zero" >&2
    cat "${ENABLE_LOG}" >&2 || true
    exit 1
fi
if ! jq -e '.enabled == true and .status == "enabled"' < "${ENABLE_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: enable must produce enabled=true status='enabled' — see ${ENABLE_LOG}" >&2
    cat "${ENABLE_LOG}" >&2 || true
    exit 1
fi

# ── Step 6: drive `plugin disable alpha` ───────────────────────────────
echo "[INFO] P2-W6-F07: running plugin disable alpha..."
if ! "${UCIL_BIN}" plugin disable alpha --plugins-dir "${PLUGINS_DIR}" --format json > "${DISABLE_LOG}"; then
    echo "[FAIL] P2-W6-F07: 'ucil plugin disable alpha' exited non-zero" >&2
    cat "${DISABLE_LOG}" >&2 || true
    exit 1
fi
if ! jq -e '.enabled == false and .status == "disabled"' < "${DISABLE_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: disable must produce enabled=false status='disabled' — see ${DISABLE_LOG}" >&2
    cat "${DISABLE_LOG}" >&2 || true
    exit 1
fi

# ── Step 7: drive `plugin uninstall alpha` ─────────────────────────────
echo "[INFO] P2-W6-F07: running plugin uninstall alpha..."
if ! "${UCIL_BIN}" plugin uninstall alpha --plugins-dir "${PLUGINS_DIR}" --format json > "${UNINSTALL_LOG}"; then
    echo "[FAIL] P2-W6-F07: 'ucil plugin uninstall alpha' exited non-zero" >&2
    cat "${UNINSTALL_LOG}" >&2 || true
    exit 1
fi
if ! jq -e '.installed == false and .status == "uninstalled"' < "${UNINSTALL_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: uninstall must produce installed=false status='uninstalled' — see ${UNINSTALL_LOG}" >&2
    cat "${UNINSTALL_LOG}" >&2 || true
    exit 1
fi

# ── Step 8: state file reflects cumulative mutations ────────────────────
STATE_FILE="${PLUGINS_DIR}/.ucil-plugin-state.toml"
if [[ ! -f "${STATE_FILE}" ]]; then
    echo "[FAIL] P2-W6-F07: state file ${STATE_FILE} not produced after mutations" >&2
    exit 1
fi
# After: reload (installed=true) → enable → disable → uninstall, the
# cumulative state is installed=false enabled=false for alpha.
if ! grep -E '^name = "alpha"' "${STATE_FILE}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: state file missing alpha row — contents:" >&2
    cat "${STATE_FILE}" >&2 || true
    exit 1
fi
# Re-run list and assert the state is reflected: alpha now installed=false enabled=false (after final uninstall).
"${UCIL_BIN}" plugin list --plugins-dir "${PLUGINS_DIR}" --format json > "${LIST_LOG}"
if ! jq -e '.plugins | map(select(.name == "alpha")) | length == 1' < "${LIST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: post-mutation list missing alpha row — see ${LIST_LOG}" >&2
    cat "${LIST_LOG}" >&2 || true
    exit 1
fi
if ! jq -e '.plugins | map(select(.name == "alpha")) | .[0].installed == false' < "${LIST_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: post-mutation alpha must show installed=false — see ${LIST_LOG}" >&2
    cat "${LIST_LOG}" >&2 || true
    exit 1
fi

# ── Step 9: cargo test commands::plugin:: ──────────────────────────────
echo "[INFO] P2-W6-F07: running cargo test -p ucil-cli commands::plugin::..."
if ! cargo test -p ucil-cli commands::plugin:: 2>&1 | tee "${CARGO_LOG}" >/dev/null; then
    echo "[FAIL] P2-W6-F07: cargo test exited non-zero — see ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi
if ! grep -Eq 'test result: ok\. [0-9]+ passed; 0 failed|[0-9]+ tests run: [0-9]+ passed' "${CARGO_LOG}"; then
    echo "[FAIL] P2-W6-F07: cargo test summary line missing in ${CARGO_LOG}" >&2
    tail -20 "${CARGO_LOG}" >&2 || true
    exit 1
fi

# ── Optional: shellcheck ───────────────────────────────────────────────
if command -v shellcheck >/dev/null 2>&1; then
    if ! shellcheck "${REPO_ROOT}/scripts/verify/P2-W6-F07.sh"; then
        echo "[FAIL] P2-W6-F07: shellcheck flagged the verify script" >&2
        exit 1
    fi
else
    echo "[INFO] P2-W6-F07: shellcheck not on PATH; skipping lint."
fi

echo "[OK] P2-W6-F07"
exit 0
