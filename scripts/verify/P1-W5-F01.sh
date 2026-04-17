#!/usr/bin/env bash
# Acceptance test for P1-W5-F01 — Serena plugin install + live tools/list probe.
#
# Prerequisites:
#   * `uvx`  — provided by Astral's `uv` (https://github.com/astral-sh/uv).
#              Install via `curl -LsSf https://astral.sh/uv/install.sh | sh`
#              or your distro's package manager.
#   * `jq`   — JSON CLI; install via `apt-get install jq` / `brew install jq`.
#
# A Docker-fallback harness (for CI images that cannot install uv) is
# explicitly out of scope per ucil-build/work-orders/0013-* scope_out
# — a follow-up WO will add it.
#
# Behaviour:
#   1. cd to the repo root (via `git rev-parse --show-toplevel`).
#   2. Assert uvx + jq are available; exit 1 with a clear message otherwise.
#   3. Pre-warm the Serena uvx cache (swallow output, 120 s cap).
#   4. Build the CLI in release mode (`cargo build --release -p ucil-cli --locked`).
#   5. Invoke `ucil plugin install serena --plugins-dir ./plugins
#      --timeout-ms 180000 --format json`.
#   6. Parse the JSON.  Require status == "ok" AND tool_count >= 10.
#   7. Exit 0 on success; non-zero with a one-line failure reason on any
#      other outcome.

set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# Serena git-ref must match plugins/structural/serena/plugin.toml.
SERENA_REF="v1.0.0"
SERENA_PKG="git+https://github.com/oraios/serena@${SERENA_REF}"

# ── Prereq: uvx ─────────────────────────────────────────────────────────
if ! command -v uvx >/dev/null 2>&1; then
    echo "P1-W5-F01: requires uvx (install from https://github.com/astral-sh/uv). Serena docker fallback is a future WO." >&2
    exit 1
fi

# ── Prereq: jq ─────────────────────────────────────────────────────────
if ! command -v jq >/dev/null 2>&1; then
    echo "P1-W5-F01: requires jq (install via 'apt-get install jq' or 'brew install jq')." >&2
    exit 1
fi

# ── Pre-warm Serena uvx cache ──────────────────────────────────────────
# First-run uvx can take 60-120s to download Serena + its Python deps.
# `--help` is a cheap way to force the download without starting the server.
echo "P1-W5-F01: pre-warming uvx cache for ${SERENA_PKG} (cap 120 s)..."
timeout 120 uvx --from "${SERENA_PKG}" serena-mcp-server --help >/dev/null 2>&1 || true

# ── Build the CLI ──────────────────────────────────────────────────────
echo "P1-W5-F01: building ucil-cli (release)..."
cargo build --release -p ucil-cli --locked >/dev/null 2>&1

CLI_BIN="${REPO_ROOT}/target/release/ucil"
if [[ ! -x "${CLI_BIN}" ]]; then
    echo "P1-W5-F01: FAIL — release binary not found at ${CLI_BIN}" >&2
    exit 1
fi

# ── Invoke plugin install ──────────────────────────────────────────────
echo "P1-W5-F01: invoking ucil plugin install serena..."
if ! OUT="$("${CLI_BIN}" plugin install serena \
        --plugins-dir ./plugins \
        --timeout-ms 180000 \
        --format json 2>&1)"; then
    echo "P1-W5-F01: FAIL — CLI exited non-zero. Output:" >&2
    echo "${OUT}" >&2
    exit 1
fi

# ── Parse the JSON ─────────────────────────────────────────────────────
STATUS="$(echo "${OUT}" | jq -r '.status // "missing"')"
TOOL_COUNT="$(echo "${OUT}" | jq -r '.tool_count // 0')"

if [[ "${STATUS}" != "ok" ]]; then
    echo "P1-W5-F01: FAIL — status=${STATUS} (expected 'ok'). Output:" >&2
    echo "${OUT}" >&2
    exit 1
fi

if [[ "${TOOL_COUNT}" -lt 10 ]]; then
    echo "P1-W5-F01: FAIL — tool_count=${TOOL_COUNT} (expected >= 10). Output:" >&2
    echo "${OUT}" >&2
    exit 1
fi

echo "P1-W5-F01 PASS: serena status=${STATUS} tools=${TOOL_COUNT}"
