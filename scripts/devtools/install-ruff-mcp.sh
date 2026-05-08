#!/usr/bin/env bash
# Idempotent helper that documents how to install / warm the
# Ruff MCP server pinned by `plugins/quality/ruff/plugin.toml`.
#
# This script does NOT auto-install — it reports the current state and
# prints the recommended install command. The optional warm-up tries
# `uvx mcp-server-analyzer@<pin> --help 2>&1 | head -1` so the pypi
# tarball lands in the uv cache before the integration test pays the
# cold-cache cost; if `--help` is unsupported by the upstream binary
# the warm-up exits gracefully — the uv fetch still primes the cache
# as a side-effect of uvx resolving the package spec.
#
# Unlike install-eslint-mcp.sh, this script checks for python3 + uv
# (or uvx) — Ruff MCP is pypi-distributed via Anselmoo's
# `mcp-server-analyzer` wrapper, NOT npm-distributed. If `uv` /
# `uvx` is missing the script emits a `[MISSING]` notice and exits 0
# gracefully (operator-state — does not auto-install uv).
#
# Self-resolving server (per plugins/quality/ruff/plugin.toml top-of-
# file rustdoc): the upstream `mcp-server-analyzer` PyPI package
# bundles Ruff + Vulture as transitive Python dependencies via its
# pyproject.toml. The uvx-managed venv self-resolves both binaries —
# NO equivalent of the WO-0076 Semgrep CLI dep (`semgrep --pro
# --version` lifespan call). The script therefore does NOT probe
# for a separate `ruff` CLI on PATH.
#
# WO-0074 §executor #2 lesson applied: do NOT use `--mcp` as a
# warm-up flag — the upstream binary IS an MCP server by default
# and would block on stdin. `--help` is the canonical CLI warm-up;
# if `--help` is not honored the uv fetch still primes the cache.
#
# Exit code is always 0; this is informational scaffolding.
set -euo pipefail

PINNED_PYPI_VERSION="0.1.2"
PINNED_PKG_SPEC="mcp-server-analyzer@${PINNED_PYPI_VERSION}"

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

printf '[OK] uvx is on PATH (%s); plugin.toml launches the server via\n' \
    "$(command -v uvx)"
printf '     `uvx %s` — no global install required.\n' "$PINNED_PKG_SPEC"
printf '     First run downloads the package + transitive deps (ruff,\n'
printf '     vulture, mcp Python SDK) into the uv cache; subsequent\n'
printf '     runs hit the cache and complete in well under a second.\n'
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
printf '[NOTE] Ruff MCP is the Python lint + format half of the G7\n'
printf '       (Quality) source group. It runs entirely locally — no\n'
printf '       network calls (after the initial uvx fetch), no auth\n'
printf '       required. The upstream binary advertises five tools\n'
printf '       (kebab-case, as emitted by tools/list):\n'
printf '         ruff-check     - Lint Python code via Ruff.\n'
printf '         ruff-format    - Format Python code via Ruff.\n'
printf '         ruff-check-ci  - CI/CD-shaped Ruff lint output.\n'
printf '         vulture-scan   - Dead-code detection via Vulture.\n'
printf '         analyze-code   - Combined Ruff + Vulture analysis.\n'
printf '       The integration test pins on `ruff-check` (canonical\n'
printf '       lint tool; maps verbatim to our `quality.ruff.lint`\n'
printf '       capability). Tools take inline `code:str` arguments —\n'
printf '       the verify script reads file contents into Python and\n'
printf '       passes them via the `code` argument; the spawned\n'
printf '       binary is cwd-independent. The integration test +\n'
printf '       verify script BOTH copy `tests/fixtures/python-project`\n'
printf '       into a `mktemp -d` tmpdir BEFORE fabricating any\n'
printf '       deliberate-violation file (per WO-0080 scope_in #35/#36)\n'
printf '       — the read-only fixture tree stays pristine.\n'
printf '       The PyPI package name `mcp-server-analyzer` differs\n'
printf '       from the runtime serverInfo.name `Python Analyzer`\n'
printf '       (FastMCP advertises the Python class name independently\n'
printf '       of the PyPI package version) — see the plugin.toml\n'
printf '       top-of-file rustdoc for the full asymmetry capture.\n'
exit 0
