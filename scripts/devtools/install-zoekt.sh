#!/usr/bin/env bash
# Idempotent helper that documents how to install the zoekt + zoekt-index
# binaries pinned by `plugins/search/zoekt/plugin.toml` (WO-0086 /
# P3-W10-F15; master-plan §4.2 line 300 — trigram-indexed search, P1).
#
# This script does NOT auto-install — it reports the current state and
# prints recommended install commands. Operators install manually.
#
# Tier-1 install (canonical) requires Go >= 1.21 on PATH. Sourcegraph/zoekt
# does not publish standalone pre-built binaries (verified 2026-05-09 via
# `gh api repos/sourcegraph/zoekt/releases` returning empty); Tier-1 via
# `go install` is the only operator-facing install path. If/when upstream
# starts publishing pre-built binaries, an ADR may add a Tier-2 fallback.
#
# Exit code is always 0; this is informational scaffolding matching the
# `install-ripgrep.sh` / `install-ast-grep.sh` / `install-dependency-cruiser.sh`
# shape.
set -euo pipefail

PINNED_ZOEKT_REF="2a1cee1ac057de4d43c0ce316b11fd3648c9ee25" # gitleaks:allow — git commit-SHA, not a token

zoekt_present=0
zoekt_index_present=0
if command -v zoekt >/dev/null 2>&1; then
    zoekt_present=1
fi
if command -v zoekt-index >/dev/null 2>&1; then
    zoekt_index_present=1
fi

if [[ $zoekt_present -eq 1 && $zoekt_index_present -eq 1 ]]; then
    printf '[OK] zoekt binary at %s\n' "$(command -v zoekt)"
    printf '     zoekt-index binary at %s\n' "$(command -v zoekt-index)"
    printf '     plugin.toml pins sourcegraph/zoekt commit %s.\n' "$PINNED_ZOEKT_REF"
    exit 0
fi

printf '[MISSING] zoekt and/or zoekt-index are not on PATH (zoekt=%d zoekt-index=%d).\n' \
    "$zoekt_present" "$zoekt_index_present"
printf 'To install Zoekt pinned by plugins/search/zoekt/plugin.toml (commit %s):\n' \
    "$PINNED_ZOEKT_REF"
if ! command -v go >/dev/null 2>&1; then
    printf '  [PREREQ] Go >= 1.21 must be on PATH first. See https://go.dev/dl/\n'
fi
printf '  go install github.com/sourcegraph/zoekt/cmd/zoekt-index@%s   # Tier-1\n' "$PINNED_ZOEKT_REF"
printf '  go install github.com/sourcegraph/zoekt/cmd/zoekt@%s         # Tier-1\n' "$PINNED_ZOEKT_REF"
printf '\n'
printf 'After install, verify both binaries are on $GOPATH/bin (or $HOME/go/bin):\n'
printf '  zoekt --help        # query frontend\n'
printf '  zoekt-index --help  # offline indexer\n'
exit 0
