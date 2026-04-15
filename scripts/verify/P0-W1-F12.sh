#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(git rev-parse --show-toplevel)"
FIXTURE="$REPO_ROOT/tests/fixtures/python-project"
VALIDATOR="$REPO_ROOT/tests/fixtures/python_project/test_fixture_valid.py"

echo "=== P0-W1-F12: python-project fixture ==="

# Assert fixture directory exists
[ -d "$FIXTURE" ] || { echo "FAIL: $FIXTURE directory missing"; exit 1; }
[ -f "$FIXTURE/pyproject.toml" ] || { echo "FAIL: pyproject.toml missing"; exit 1; }
[ -d "$FIXTURE/src/python_project" ] || { echo "FAIL: src/python_project missing"; exit 1; }
[ -f "$VALIDATOR" ] || { echo "FAIL: test_fixture_valid.py missing"; exit 1; }

echo "All fixture files present. Running validator..."
cd "$REPO_ROOT"
# Use python3 (python may not be in PATH on all systems)
python3 -m pytest "$VALIDATOR" -v
echo "=== P0-W1-F12 PASS ==="
