#!/usr/bin/env bash
# Idempotent helper that documents how to install / warm the
# Semgrep MCP server pinned by `plugins/quality/semgrep/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. The optional warm-up tries
# `uvx semgrep-mcp@<pin> --help 2>&1 | head -1` so the pypi tarball
# lands in the uv cache before the integration test pays the cold-
# cache cost; if `--help` is unsupported by the upstream binary the
# warm-up exits gracefully — the uv fetch still primes the cache as
# a side-effect of uvx resolving the package spec.
#
# Unlike install-eslint-mcp.sh, this script checks for python3 + uv
# (or uvx) — Semgrep MCP is pypi-distributed, NOT npm-distributed.
# If `uv` / `uvx` is missing the script emits a `[MISSING]` notice
# and exits 0 gracefully (operator-state — does not auto-install uv).
#
# Semgrep MCP also REQUIRES the Semgrep CLI binary on PATH (or
# located via the `SEMGREP_PATH` env var) — the `semgrep_mcp.semgrep
# .mk_context` lifespan calls `semgrep --pro --version` BEFORE the
# MCP server reaches its initialize handshake. The script reports
# the Semgrep CLI's status and points the operator at install
# options when missing; it does NOT auto-install Semgrep.
#
# Disclosed Deviation: pinned version is 0.8.1 (NOT WO-0076 scope_in's
# 0.9.0). v0.9.0 ships only a `deprecation_notice` tool upstream
# (verified via live tools/list capture in the WO-0076 RFR);
# v0.8.1 is the last PyPI release with the canonical 8-tool scan
# surface. See plugins/quality/semgrep/plugin.toml top-of-file
# rustdoc for the full rationale.
#
# WO-0074 §executor #2 lesson applied: do NOT use `--mcp` as a
# warm-up flag — Semgrep MCP IS an MCP server by default and would
# block on stdin. `--help` is the canonical CLI warm-up.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_PYPI_VERSION="0.8.1"
PINNED_PKG_SPEC="semgrep-mcp@${PINNED_PYPI_VERSION}"

if ! command -v python3 >/dev/null 2>&1; then
    printf '[MISSING] python3 is not on PATH.\n'
    printf 'Install Python 3.11+ first (e.g. via your distro package\n'
    printf 'manager or pyenv: `pyenv install 3.13.0`), then re-run\n'
    printf 'this script.\n'
    exit 0
fi

if ! command -v uvx >/dev/null 2>&1; then
    printf '[MISSING] uvx is not on PATH (it ships with `uv`).\n'
    printf 'Install uv first via the official installer:\n'
    printf '  curl -LsSf https://astral.sh/uv/install.sh | sh\n'
    printf 'Or via pip: `pip install --user uv`\n'
    printf 'See https://docs.astral.sh/uv/getting-started/installation/\n'
    printf 'for additional install options. After install, re-run this\n'
    printf 'script to warm the uv cache for %s.\n' "$PINNED_PKG_SPEC"
    exit 0
fi

# Semgrep CLI dep — required by the upstream lifespan handler.
if command -v semgrep >/dev/null 2>&1; then
    found_semgrep="$(command -v semgrep)"
    found_version="$(semgrep --version 2>/dev/null | head -1 || echo unknown)"
    printf '[OK] semgrep CLI binary at %s reports version: %s\n' \
        "$found_semgrep" "$found_version"
elif [[ -n "${SEMGREP_PATH:-}" && -x "${SEMGREP_PATH:-}" ]]; then
    found_version="$("$SEMGREP_PATH" --version 2>/dev/null | head -1 || echo unknown)"
    printf '[OK] semgrep CLI binary at $SEMGREP_PATH (%s) reports version: %s\n' \
        "$SEMGREP_PATH" "$found_version"
else
    printf '[MISSING] semgrep CLI binary is not on PATH (and SEMGREP_PATH is\n'
    printf '          unset / not executable).\n'
    printf '          Semgrep MCP requires the Semgrep CLI to satisfy its\n'
    printf '          lifespan check (`semgrep --pro --version`). Install\n'
    printf '          via:\n'
    printf '            pip install --user semgrep   # PyPI\n'
    printf '            uv tool install semgrep      # uv-managed\n'
    printf '            brew install semgrep         # macOS Homebrew\n'
    printf '          See https://semgrep.dev/docs/getting-started/ for\n'
    printf '          additional install options. After install, re-run\n'
    printf '          this script to warm the uv cache for %s. The\n' "$PINNED_PKG_SPEC"
    printf '          integration test + verify script BOTH skip\n'
    printf '          gracefully when the Semgrep CLI is missing.\n'
fi

if command -v semgrep-mcp >/dev/null 2>&1; then
    found_version="$(semgrep-mcp --version 2>/dev/null | head -1 || echo unknown)"
    printf '[OK] semgrep-mcp binary at %s reports version: %s\n' \
        "$(command -v semgrep-mcp)" "$found_version"
    printf '     plugin.toml pins pypi package %s.\n' "$PINNED_PKG_SPEC"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       uv tool install %s\n' "$PINNED_PKG_SPEC"
fi

printf '[OK] uvx is on PATH (%s); plugin.toml launches the server via\n' \
    "$(command -v uvx)"
printf '     `uvx %s` — no global install required.\n' "$PINNED_PKG_SPEC"
printf '     First run downloads the package + transitive deps into the\n'
printf '     uv cache; subsequent runs hit the cache and complete in\n'
printf '     well under a second.\n'
printf '     For a global install (faster cold start, no uvx fetch):\n'
printf '       uv tool install %s\n' "$PINNED_PKG_SPEC"

# Best-effort warm-up via `--help` (NOT --mcp — the upstream binary
# IS an MCP server by default and would block on stdin). The goal is
# a side-effect uv fetch into the cache.
printf '[INFO] warming uvx cache for %s ...\n' "$PINNED_PKG_SPEC"
if uvx "$PINNED_PKG_SPEC" --help >/dev/null 2>&1; then
    printf '[INFO] warm-up emitted --help; cache primed.\n'
else
    printf '[INFO] --help unsupported by upstream binary; warm-up exited\n'
    printf '       gracefully — the uv fetch still primed the cache as a\n'
    printf '       side-effect of uvx resolving the package spec.\n'
fi

printf '\n'
printf '[NOTE] Semgrep MCP is the multi-language SAST half of the G7\n'
printf '       (Quality) source group. The basic OWASP Top-10 scan\n'
printf '       surface (`p/owasp-top-ten` ruleset, advertised via the\n'
printf '       `semgrep_scan` tool) requires NO API key and runs\n'
printf '       entirely locally against operator-supplied code\n'
printf '       contents. An OPTIONAL `SEMGREP_APP_TOKEN` env var\n'
printf '       enables the Semgrep AppSec Platform Pro engine for\n'
printf '       advanced cross-file taint tracking + supply-chain\n'
printf '       scans, but is NOT required by the WO-0076 F04 verify\n'
printf '       script smoke. The integration test + verify script\n'
printf '       BOTH operate without a token. The Semgrep CLI binary\n'
printf '       on PATH (or `SEMGREP_PATH` env var) IS required —\n'
printf '       operator-state per the manifest top-of-file rustdoc.\n'
exit 0
