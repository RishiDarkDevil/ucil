#!/usr/bin/env bash
# scripts/verify/P3-W10-F14.sh — dependency-cruiser plugin smoke
#
# Acceptance test for P3-W10-F14 (WO-0086). Master-plan §4.4 line 328
# lists dependency-cruiser as the P0 JS/TS architecture/dependency tool.
# DEC-0009 (search-code-in-process-ripgrep, generalised): dep-cruiser is
# a CLI invocation, not an MCP server. The manifest's [transport] table
# is a declarative sentinel; the runtime path is `depcruise` invocation
# directly against the target tree.
#
# Sub-checks performed (all must pass for [OK]):
#   1. cargo test -p ucil-daemon --test g4_plugin_manifests
#      g4_plugin_manifests::dependency_cruiser_manifest_parses
#   2. depcruise on PATH, version printed for verifier log
#   3. plugins/architecture/dependency-cruiser/plugin.toml exists
#   4. tmpdir-fabricated cycle-a/cycle-b TS module pair surfaces a
#      `no-circular` violation in dep-cruiser's JSON output (load-bearing)
#
# tests/fixtures/typescript-project/** is read-only per WO-0086
# forbidden_paths; the cycle fabrication lives in a `mktemp -d` tmpdir
# copy ONLY (the fixture itself is a clean DAG without a cycle).
#
# Skip env-var: UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E (extends WO-0072 verbatim).
# Verifier MUST NOT set this env var.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if [[ "${UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E:-0}" != "0" ]]; then
    echo "[SKIP] P3-W10-F14: architecture plugin smoke skipped via UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E"
    exit 0
fi

# (1) Tool prerequisite: depcruise on PATH.
if ! command -v depcruise >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F14: depcruise not on PATH; see scripts/devtools/install-dependency-cruiser.sh" >&2
    exit 1
fi
DEPCRUISE_VERSION="$(depcruise --version 2>/dev/null | head -n1)"
echo "[INFO] depcruise version: ${DEPCRUISE_VERSION}"

if ! command -v jq >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F14: jq not on PATH (required to parse depcruise JSON)" >&2
    exit 1
fi

# (2) Manifest exists at canonical path.
if ! test -f plugins/architecture/dependency-cruiser/plugin.toml; then
    echo "[FAIL] P3-W10-F14: plugins/architecture/dependency-cruiser/plugin.toml missing" >&2
    exit 1
fi

# (3) Cargo test for the parse-only manifest assertion.
CARGO_LOG="$(mktemp -t wo-0086-f14-cargo.XXXXXX.log)"
TMPDIR_F14="$(mktemp -d)"
DEPCRUISE_OUT="$(mktemp -t wo-0086-depcruise.XXXXXX.json)"
cleanup() {
    rm -rf "$TMPDIR_F14"
    rm -f "$CARGO_LOG" "$DEPCRUISE_OUT"
}
trap cleanup EXIT

if ! cargo test -p ucil-daemon --test g4_plugin_manifests g4_plugin_manifests::dependency_cruiser_manifest_parses 2>&1 | tee "$CARGO_LOG" >/dev/null; then
    echo "[FAIL] P3-W10-F14: dependency_cruiser_manifest_parses cargo test failed" >&2
    cat "$CARGO_LOG" >&2
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests? passed' "$CARGO_LOG"; then
    echo "[FAIL] P3-W10-F14: dependency_cruiser_manifest_parses did not report a single passing test" >&2
    cat "$CARGO_LOG" >&2
    exit 1
fi

# (4) Tmpdir-fabricated circular-dep smoke.
cp -r tests/fixtures/typescript-project "$TMPDIR_F14/ts-fixture"

# Fabricate the cycle in the tmpdir copy ONLY (fixture itself stays clean).
cat > "$TMPDIR_F14/ts-fixture/src/cycle-a.ts" <<'TSA'
import { fromB } from "./cycle-b.js";
export const fromA = (): number => fromB();
TSA
cat > "$TMPDIR_F14/ts-fixture/src/cycle-b.ts" <<'TSB'
import { fromA } from "./cycle-a.js";
export const fromB = (): number => fromA();
TSB

# Minimal dep-cruiser config that elevates circular cycles to violations.
# Lives in the tmpdir; never in tests/fixtures/**.
cat > "$TMPDIR_F14/ts-fixture/.dependency-cruiser.cjs" <<'DCC'
module.exports = {
  forbidden: [
    {
      name: 'no-circular',
      severity: 'warn',
      from: {},
      to: { circular: true },
    },
  ],
};
DCC

if ! ( cd "$TMPDIR_F14/ts-fixture" && depcruise --output-type json --config .dependency-cruiser.cjs --include-only '^src/' src ) > "$DEPCRUISE_OUT" 2>/dev/null; then
    echo "[FAIL] P3-W10-F14: depcruise invocation against tmpdir cycle-a/cycle-b pair failed" >&2
    exit 1
fi

# Assert at least one no-circular violation surfaces in the JSON output.
if ! jq -e '[.summary.violations[] | select(.rule.name == "no-circular")] | length > 0' "$DEPCRUISE_OUT" >/dev/null; then
    echo "[FAIL] P3-W10-F14: depcruise did not surface a 'no-circular' violation against the tmpdir-fabricated cycle-a/cycle-b pair" >&2
    jq '.summary' "$DEPCRUISE_OUT" >&2 || cat "$DEPCRUISE_OUT" >&2
    exit 1
fi
CYCLE_COUNT="$(jq '[.summary.violations[] | select(.rule.name == "no-circular")] | length' "$DEPCRUISE_OUT")"
echo "[INFO] depcruise reported ${CYCLE_COUNT} no-circular violation(s) on the fabricated cycle-a/cycle-b pair"

echo "[OK] P3-W10-F14"
exit 0
