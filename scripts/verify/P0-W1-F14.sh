#!/usr/bin/env bash
# Acceptance test for P0-W1-F14: mixed-project fixture
# Verifies:
#   1. All expected source files exist.
#   2. `cargo clippy` on the fixture produces at least one warning.
# Exits 0 on success, non-zero on any failure.

set -euo pipefail
REPO_ROOT="$(git rev-parse --show-toplevel)"
FIXTURE="$REPO_ROOT/tests/fixtures/mixed-project"

echo "=== P0-W1-F14: mixed-project fixture ==="

# ── 1. Assert expected source files exist ────────────────────────────────────
assert_file() {
    local path="$1"
    if [ ! -f "$path" ]; then
        echo "FAIL: expected file missing: $path"
        exit 1
    fi
    echo "  OK: $path"
}

assert_file "$FIXTURE/Cargo.toml"
assert_file "$FIXTURE/src/main.rs"
assert_file "$FIXTURE/tests/integration_test.rs"
assert_file "$FIXTURE/package.json"
assert_file "$FIXTURE/src/index.ts"
assert_file "$FIXTURE/tests/index.test.ts"
assert_file "$FIXTURE/pyproject.toml"
assert_file "$FIXTURE/src/main.py"
assert_file "$FIXTURE/tests/test_main.py"

echo "All expected files present."

# ── 2. Run cargo clippy and assert at least one warning ──────────────────────
echo ""
echo "Running cargo clippy on mixed-project Rust component..."

clippy_output=$(cargo clippy \
    --manifest-path "$FIXTURE/Cargo.toml" \
    2>&1 || true)

echo "$clippy_output"

if echo "$clippy_output" | grep -q "^warning:"; then
    echo ""
    echo "OK: cargo clippy produced at least one warning (expected for defect fixture)."
else
    echo ""
    echo "FAIL: expected at least one 'warning:' line from cargo clippy on mixed-project."
    echo "      The fixture is supposed to contain code with Rust lint defects."
    exit 1
fi

echo ""
echo "=== P0-W1-F14 PASS ==="
