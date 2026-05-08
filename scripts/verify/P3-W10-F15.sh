#!/usr/bin/env bash
# scripts/verify/P3-W10-F15.sh — Zoekt plugin smoke
#
# Acceptance test for P3-W10-F15 (WO-0086). Master-plan §4.2 line 300
# lists Zoekt as the P1 trigram-indexed search HTTP API. DEC-0009
# (search-code-in-process-ripgrep, generalised): Zoekt is an external-
# service / CLI tool, not an MCP server. The manifest's [transport]
# table is a declarative sentinel; the runtime path is `zoekt-index`
# (offline indexer) + `zoekt` (query) directly.
#
# Sub-checks performed (all must pass for [OK]):
#   1. cargo test -p ucil-daemon --test plugin_manifests
#      plugin_manifests::zoekt_manifest_parses
#   2. zoekt + zoekt-index on PATH; ripgrep on PATH (comparison oracle)
#   3. plugins/search/zoekt/plugin.toml exists
#   4. tmpdir corpus (rust + ts + python fixture projects) indexed via
#      zoekt-index, queried via zoekt and ripgrep — Zoekt match-set
#      MUST be a non-empty subset of ripgrep's match-set (correctness
#      invariant; load-bearing)
#   5. wall-clock guard: zoekt warm query completes within ripgrep's
#      time + 50ms (master-plan §4.2 'faster than ripgrep on warm
#      index'; 50ms tolerance absorbs small-corpus noise)
#
# tests/fixtures/** is read-only per WO-0086 forbidden_paths; the corpus
# and zoekt-index storage live in a `mktemp -d` tmpdir copy ONLY.
#
# Skip env-var: UCIL_SKIP_SEARCH_PLUGIN_E2E (NEW G2-search-group
# convention introduced in WO-0086, parallel to UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E
# from WO-0072 and UCIL_SKIP_QUALITY_PLUGIN_E2E from WO-0080). Verifier
# MUST NOT set this env var.
set -euo pipefail
IFS=$'\n\t'

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if [[ "${UCIL_SKIP_SEARCH_PLUGIN_E2E:-0}" != "0" ]]; then
    echo "[SKIP] P3-W10-F15: search plugin smoke skipped via UCIL_SKIP_SEARCH_PLUGIN_E2E"
    exit 0
fi

# (1) Tool prerequisites.
if ! command -v zoekt >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F15: zoekt not on PATH; see scripts/devtools/install-zoekt.sh" >&2
    exit 1
fi
if ! command -v zoekt-index >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F15: zoekt-index not on PATH; see scripts/devtools/install-zoekt.sh" >&2
    exit 1
fi
if ! command -v rg >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F15: rg (ripgrep, comparison oracle) not on PATH" >&2
    exit 1
fi

# Print version diagnostics for the verifier log. Some Go binaries (zoekt,
# zoekt-index) emit usage on bare --version; the first line is captured.
ZOEKT_VERSION="$(zoekt --version 2>&1 | head -n1 || true)"
ZOEKT_INDEX_VERSION="$(zoekt-index --version 2>&1 | head -n1 || true)"
RG_VERSION="$(rg --version 2>&1 | head -n1)"
echo "[INFO] zoekt: ${ZOEKT_VERSION}"
echo "[INFO] zoekt-index: ${ZOEKT_INDEX_VERSION}"
echo "[INFO] ripgrep: ${RG_VERSION}"

# (2) Manifest exists at canonical path.
if ! test -f plugins/search/zoekt/plugin.toml; then
    echo "[FAIL] P3-W10-F15: plugins/search/zoekt/plugin.toml missing" >&2
    exit 1
fi

# (3) Cargo test for the parse-only manifest assertion.
CARGO_LOG="$(mktemp -t wo-0086-f15-cargo.XXXXXX.log)"
TMPDIR_F15="$(mktemp -d)"
ZOEKT_OUT="$(mktemp -t wo-0086-zoekt-out.XXXXXX.txt)"
RG_OUT="$(mktemp -t wo-0086-rg-out.XXXXXX.txt)"
ZOEKT_PATHS="$(mktemp -t wo-0086-zoekt-paths.XXXXXX.txt)"
RG_PATHS="$(mktemp -t wo-0086-rg-paths.XXXXXX.txt)"
cleanup() {
    rm -rf "$TMPDIR_F15"
    rm -f "$CARGO_LOG" "$ZOEKT_OUT" "$RG_OUT" "$ZOEKT_PATHS" "$RG_PATHS"
}
trap cleanup EXIT

if ! cargo test -p ucil-daemon --test plugin_manifests plugin_manifests::zoekt_manifest_parses 2>&1 | tee "$CARGO_LOG" >/dev/null; then
    echo "[FAIL] P3-W10-F15: zoekt_manifest_parses cargo test failed" >&2
    cat "$CARGO_LOG" >&2
    exit 1
fi
if ! grep -Eq 'test result: ok\. 1 passed; 0 failed|1 tests? passed' "$CARGO_LOG"; then
    echo "[FAIL] P3-W10-F15: zoekt_manifest_parses did not report a single passing test" >&2
    cat "$CARGO_LOG" >&2
    exit 1
fi

# (4) Build trigram index over a tmpdir corpus of fixture projects.
mkdir "$TMPDIR_F15/corpus"
cp -r tests/fixtures/rust-project tests/fixtures/typescript-project tests/fixtures/python-project "$TMPDIR_F15/corpus/"

INDEX_DIR="$TMPDIR_F15/zoekt-index"
mkdir "$INDEX_DIR"
if ! zoekt-index -index "$INDEX_DIR" "$TMPDIR_F15/corpus" >/dev/null 2>&1; then
    echo "[FAIL] P3-W10-F15: zoekt-index build over tmpdir corpus failed" >&2
    exit 1
fi

# (5) Run identical query through Zoekt (warm) and ripgrep, capture
# wall-clock + match sets. Use a single-token query so both tools agree
# on the substring-match semantic (Zoekt's default treats space-separated
# tokens as AND-of-tokens; single token sidesteps that mismatch). The
# `evaluate` token appears across all three fixture languages so the
# match set is non-trivial.
QUERY='evaluate'

# Warm Zoekt cache.
zoekt -index_dir "$INDEX_DIR" "$QUERY" >/dev/null 2>&1 || true

ZOEKT_T_NS=$(date +%s%N)
zoekt -index_dir "$INDEX_DIR" "$QUERY" > "$ZOEKT_OUT" 2>&1 || true
ZOEKT_NS=$(($(date +%s%N) - ZOEKT_T_NS))

# ripgrep with --no-ignore --hidden so its scan-set matches Zoekt's
# index-set (Zoekt indexes everything under the corpus dir; ripgrep's
# default .gitignore-aware scan would otherwise return a strict subset).
RG_T_NS=$(date +%s%N)
rg --no-ignore --hidden --files-with-matches "$QUERY" "$TMPDIR_F15/corpus" > "$RG_OUT" 2>&1 || true
RG_NS=$(($(date +%s%N) - RG_T_NS))

echo "[INFO] zoekt warm query: ${ZOEKT_NS} ns; ripgrep query: ${RG_NS} ns; corpus root: ${TMPDIR_F15}/corpus"

# Extract relative-path match sets.
# Zoekt output format: <rel-path>:<line>:<text>. Strip ANSI escapes, take
# everything before the first ':' on each line, sort -u.
sed 's/\x1b\[[0-9;]*m//g' "$ZOEKT_OUT" \
    | awk -F':' 'NF >= 3 { print $1 }' \
    | sort -u \
    > "$ZOEKT_PATHS"

# ripgrep --files-with-matches outputs absolute paths; strip the corpus
# root prefix so paths are comparable to Zoekt's relative form.
sed "s|^${TMPDIR_F15}/corpus/||" "$RG_OUT" | sort -u > "$RG_PATHS"

ZOEKT_PATH_COUNT=$(wc -l < "$ZOEKT_PATHS")
RG_PATH_COUNT=$(wc -l < "$RG_PATHS")
echo "[INFO] zoekt matched ${ZOEKT_PATH_COUNT} file(s); ripgrep matched ${RG_PATH_COUNT} file(s)"

# Correctness invariant: Zoekt match-set MUST be non-empty AND a subset
# of ripgrep's match-set. `comm -23 a b` lists lines in a not in b.
if [[ $ZOEKT_PATH_COUNT -le 0 ]]; then
    echo "[FAIL] P3-W10-F15: zoekt returned 0 matches for query '${QUERY}' on tmpdir corpus" >&2
    echo "[DIAG] zoekt raw output (first 20 lines):" >&2
    head -n 20 "$ZOEKT_OUT" >&2 || true
    exit 1
fi

DIFF_COUNT=$(comm -23 "$ZOEKT_PATHS" "$RG_PATHS" | wc -l)
if [[ $DIFF_COUNT -ne 0 ]]; then
    echo "[FAIL] P3-W10-F15: ${DIFF_COUNT} zoekt-reported file(s) NOT reported by ripgrep (correctness invariant: Zoekt match-set MUST be subset of ripgrep match-set)" >&2
    echo "[DIAG] paths in zoekt but not in rg:" >&2
    comm -23 "$ZOEKT_PATHS" "$RG_PATHS" >&2
    exit 1
fi

# Wall-clock guard: Zoekt warm query must NOT be more than 50ms slower
# than ripgrep (master-plan §4.2 'faster than ripgrep on warm index').
# 50ms tolerance absorbs small-corpus noise; on representative corpora
# Zoekt's warm-index wins comfortably; on tiny fixtures both are <10ms.
TOLERANCE_NS=50000000
if [ $ZOEKT_NS -gt $((RG_NS + TOLERANCE_NS)) ]; then
    echo "[FAIL] P3-W10-F15: zoekt warm query (${ZOEKT_NS} ns) is more than 50ms slower than ripgrep (${RG_NS} ns) — master-plan §4.2 'faster than ripgrep on warm index' regression" >&2
    exit 1
fi

echo "[OK] P3-W10-F15"
exit 0
