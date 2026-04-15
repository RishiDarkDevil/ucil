#!/usr/bin/env bash
# Acceptance test for P0-W1-F08 — CI pipeline presence + ucil init smoke test.
#
# Checks:
#   1. .github/workflows/ci.yml exists in the repo root.
#   2. The workflow file is valid YAML (yamllint, or python3+pyyaml, or plain
#      syntax heuristics as last resort).
#   3. `ucil init --no-install-plugins` in a fresh temp dir produces
#      .ucil/init_report.json that is valid JSON.
#
# Exit 0 on success, non-zero on any failure.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
CI_YAML="${REPO_ROOT}/.github/workflows/ci.yml"

# ── 1. File existence ─────────────────────────────────────────────────────────
if [[ ! -f "${CI_YAML}" ]]; then
    echo "FAIL: .github/workflows/ci.yml not found at ${CI_YAML}" >&2
    exit 1
fi
echo "OK: .github/workflows/ci.yml exists"

# ── 2. YAML validation ────────────────────────────────────────────────────────
if command -v yamllint &>/dev/null; then
    yamllint -d relaxed "${CI_YAML}"
    echo "OK: yamllint passed"
elif python3 -c 'import yaml' 2>/dev/null; then
    python3 - "${CI_YAML}" <<'PYEOF'
import sys, yaml
try:
    doc = yaml.safe_load(open(sys.argv[1]))
    assert isinstance(doc, dict), "expected a mapping at root"
    assert "jobs" in doc, "missing 'jobs' key"
    print("OK: YAML is valid (python3 yaml.safe_load)")
except Exception as exc:
    print(f"FAIL: YAML validation error — {exc}", file=sys.stderr)
    sys.exit(1)
PYEOF
else
    echo "WARN: neither yamllint nor pyyaml available — checking for 'jobs:' key manually"
    if grep -q '^jobs:' "${CI_YAML}"; then
        echo "OK: 'jobs:' key found in ci.yml (shallow check)"
    else
        echo "FAIL: 'jobs:' key not found in ci.yml" >&2
        exit 1
    fi
fi

# ── 3. Smoke test: ucil init --no-install-plugins ─────────────────────────────
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "${WORK_DIR}"' EXIT

echo "Running: cargo run -p ucil-cli -- init --dir \"${WORK_DIR}\" --no-install-plugins"
(
    cd "${REPO_ROOT}"
    cargo run -q -p ucil-cli -- init --dir "${WORK_DIR}" --no-install-plugins
)

REPORT="${WORK_DIR}/.ucil/init_report.json"
if [[ ! -f "${REPORT}" ]]; then
    echo "FAIL: .ucil/init_report.json was not produced in ${WORK_DIR}" >&2
    exit 1
fi
echo "OK: .ucil/init_report.json produced"

# ── 4. Validate the report is well-formed JSON ────────────────────────────────
python3 - "${REPORT}" <<'PYEOF'
import sys, json
try:
    doc = json.load(open(sys.argv[1]))
    for field in ("schema_version", "project_name", "languages", "plugin_health", "llm_provider"):
        assert field in doc, f"missing field: {field}"
    print("OK: init_report.json is valid JSON with required fields")
except Exception as exc:
    print(f"FAIL: init_report.json validation error — {exc}", file=sys.stderr)
    sys.exit(1)
PYEOF

echo ""
echo "P0-W1-F08: all checks passed"
