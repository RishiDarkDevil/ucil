#!/usr/bin/env bash
# Acceptance test for P0-W1-F03 — ucil init basics
# Spins up a temp directory, runs `cargo run --bin ucil -- init`,
# asserts .ucil/ and ucil.toml exist and contain schema_version.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

echo "=== P0-W1-F03 acceptance test ==="

# Build the binary first.
echo ">>> Building ucil binary..."
cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --bin ucil --quiet

UCIL_BIN="$REPO_ROOT/target/debug/ucil"
if [[ ! -x "$UCIL_BIN" ]]; then
    echo "FAIL: binary not found at $UCIL_BIN" >&2
    exit 1
fi

# Create a temp project directory.
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

# Seed it with a Cargo.toml so language detection finds Rust.
cat > "$TMPDIR/Cargo.toml" <<'TOML'
[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
TOML
mkdir -p "$TMPDIR/src"
echo 'fn main() {}' > "$TMPDIR/src/main.rs"

echo ">>> Running: ucil init --dir $TMPDIR"
"$UCIL_BIN" init --dir "$TMPDIR"

# Assertions.
if [[ ! -d "$TMPDIR/.ucil" ]]; then
    echo "FAIL: .ucil/ directory was not created" >&2
    exit 1
fi
echo "PASS: .ucil/ directory exists"

if [[ ! -f "$TMPDIR/.ucil/ucil.toml" ]]; then
    echo "FAIL: .ucil/ucil.toml was not created" >&2
    exit 1
fi
echo "PASS: .ucil/ucil.toml exists"

if ! grep -q "schema_version" "$TMPDIR/.ucil/ucil.toml"; then
    echo "FAIL: ucil.toml does not contain schema_version" >&2
    cat "$TMPDIR/.ucil/ucil.toml"
    exit 1
fi
echo "PASS: ucil.toml contains schema_version"

if ! grep -q "rust" "$TMPDIR/.ucil/ucil.toml"; then
    echo "FAIL: ucil.toml does not contain detected language 'rust'" >&2
    cat "$TMPDIR/.ucil/ucil.toml"
    exit 1
fi
echo "PASS: ucil.toml contains detected language 'rust'"

# Idempotency: running init again should not fail.
echo ">>> Running init a second time (idempotency check)..."
"$UCIL_BIN" init --dir "$TMPDIR"
echo "PASS: second init run exited 0"

echo ""
echo "=== P0-W1-F03 PASSED ==="
