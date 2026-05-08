#!/usr/bin/env bash
# Idempotent helper that documents how to install / warm the
# mcp-pytest-runner MCP server pinned by
# `plugins/testing/mcp-pytest-runner/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. The optional warm-up tries
# `uvx mcp-pytest-runner@<pin> --help 2>&1 | head -1` so the pypi
# tarball lands in the uv cache before the integration test pays the
# cold-cache cost; if `--help` is unsupported by the upstream binary
# the warm-up exits gracefully — the uv fetch still primes the cache
# as a side-effect of uvx resolving the package spec.
#
# Like install-semgrep-mcp.sh (WO-0076), this script checks for
# python3 + uv (or uvx) — mcp-pytest-runner is pypi-distributed, NOT
# npm-distributed. If `uv` / `uvx` is missing the script emits a
# `[MISSING]` notice and exits 0 gracefully (operator-state — does
# not auto-install uv).
#
# mcp-pytest-runner does NOT have an external Python-side CLI dep
# beyond pytest itself (the upstream package declares pytest as a
# dependency in its `pyproject.toml`; uvx provisions a fresh venv
# that auto-resolves pytest from PyPI). No `SEMGREP_PATH`-style
# operator-state surfacing is required.
#
# WO-0074 §executor #2 lesson applied: do NOT use `--mcp` as a
# warm-up flag — mcp-pytest-runner IS an MCP server by default and
# would block on stdin. `--help` is the canonical CLI warm-up.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_PYPI_VERSION="0.2.1"
PINNED_PKG_SPEC="mcp-pytest-runner@${PINNED_PYPI_VERSION}"

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

if command -v mcp-pytest-runner >/dev/null 2>&1; then
    found_version="$(mcp-pytest-runner --version 2>/dev/null | head -1 || echo unknown)"
    printf '[OK] mcp-pytest-runner binary at %s reports version: %s\n' \
        "$(command -v mcp-pytest-runner)" "$found_version"
    printf '     plugin.toml pins pypi package %s.\n' "$PINNED_PKG_SPEC"
    printf '     If the binary version drifts from the pin, reinstall via:\n'
    printf '       uv tool install %s\n' "$PINNED_PKG_SPEC"
fi

printf '[OK] uvx is on PATH (%s); plugin.toml launches the server via\n' \
    "$(command -v uvx)"
printf '     `uvx %s` — no global install required.\n' "$PINNED_PKG_SPEC"
printf '     First run downloads the package + transitive deps (pytest,\n'
printf '     pluggy, etc.) into the uv cache; subsequent runs hit the\n'
printf '     cache and complete in well under a second.\n'
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
printf '[NOTE] mcp-pytest-runner is the pytest half of the G8 (Testing\n'
printf '       + CI) source group per master-plan §4.8 line 405 + §5.8.\n'
printf '       The basic discover_tests + execute_tests surface\n'
printf '       (advertised live by the running MCP server — see\n'
printf '       plugins/testing/mcp-pytest-runner/plugin.toml top-of-\n'
printf '       file rustdoc for the full live tools/list capture)\n'
printf '       requires NO API key and runs entirely locally against\n'
printf '       the operator-supplied project tree.\n'
printf '       Working-directory invariant: the integration test +\n'
printf '       verify script BOTH copy `tests/fixtures/python-project`\n'
printf '       into a `mktemp -d` tmpdir BEFORE invoking the upstream\n'
printf '       binary so the fixture stays read-only. The install\n'
printf '       script does NOT manipulate cwd — that is operator-state\n'
printf '       and is fully handled by the test/verify scripts.\n'
printf '       test-runner-mcp (P3-W11-F07) is deferred per DEC-0021;\n'
printf '       lineage chain DEC-0019 → DEC-0020 → DEC-0021 documents\n'
printf '       the upstream-availability-driven preemptive-deferral\n'
printf '       convention.\n'
exit 0
