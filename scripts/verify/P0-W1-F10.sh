#!/usr/bin/env bash
# Acceptance test for P0-W1-F10: full §17 directory skeleton present.
#
# Exits 0 if every required directory exists.
# Exits 1 with a clear message naming the first missing directory.
#
# Usage: bash scripts/verify/P0-W1-F10.sh
#   Run from the repository root.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

check_dir() {
  local dir="$1"
  if [[ ! -d "$REPO_ROOT/$dir" ]]; then
    echo "FAIL: required directory missing: $dir" >&2
    exit 1
  fi
}

# §17 required directories
check_dir "crates"
check_dir "crates/ucil-core"
check_dir "crates/ucil-daemon"
check_dir "crates/ucil-cli"
check_dir "crates/ucil-treesitter"
check_dir "crates/ucil-lsp-diagnostics"
check_dir "crates/ucil-agents"
check_dir "crates/ucil-embeddings"

check_dir "adapters"
check_dir "adapters/templates"

check_dir "plugins"
check_dir "plugins/structural"
check_dir "plugins/search"
check_dir "plugins/knowledge"
check_dir "plugins/architecture"
check_dir "plugins/context"
check_dir "plugins/platform"
check_dir "plugins/quality"
check_dir "plugins/testing"

check_dir "ml"
check_dir "ml/models"

check_dir "tests"
check_dir "tests/fixtures"
check_dir "tests/integration"
check_dir "tests/benchmarks"

check_dir "scripts"

check_dir "docs"

check_dir "plugin"
check_dir "plugin/.claude-plugin"
check_dir "plugin/agents"
check_dir "plugin/skills"
check_dir "plugin/hooks"
check_dir "plugin/.claude/rules"

echo "OK: all §17 required directories are present"
