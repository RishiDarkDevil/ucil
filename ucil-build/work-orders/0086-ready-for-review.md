# WO-0086 — ready for review

**Final commit sha**: `8d2ad95` (feat(plugins): add zoekt G2 search plugin manifest with trigram smoke)
**Branch**: `feat/WO-0086-deps-cruiser-and-zoekt-manifests`
**Phase / Week**: 3 / 10
**Features**: P3-W10-F14 (dependency-cruiser G4 architecture), P3-W10-F15 (Zoekt G2 search)

## Summary

Lands two CLI/external-service plugin manifests bundle:

* **F14 — dependency-cruiser** (master-plan §4.4 line 328, P0 JS/TS):
  manifest at `plugins/architecture/dependency-cruiser/plugin.toml`, install
  hint at `scripts/devtools/install-dependency-cruiser.sh`, parse-only
  manifest test `g4_plugin_manifests::dependency_cruiser_manifest_parses`,
  and replacement `scripts/verify/P3-W10-F14.sh` smoke that fabricates a
  circular cycle-a/cycle-b TS module pair in a tmpdir copy of
  `tests/fixtures/typescript-project` (fixture itself is read-only and
  verified at planner-time as a clean DAG) and asserts dep-cruiser
  surfaces a `no-circular` violation in its JSON output. Pinned version:
  `17.4.0` from npm `dist-tags.latest` 2026-05-09.

* **F15 — Zoekt** (master-plan §4.2 line 300, P1 trigram-indexed search):
  manifest at `plugins/search/zoekt/plugin.toml`, install hint at
  `scripts/devtools/install-zoekt.sh`, parse-only manifest test
  `plugin_manifests::zoekt_manifest_parses`, and replacement
  `scripts/verify/P3-W10-F15.sh` smoke that copies `rust-project` +
  `typescript-project` + `python-project` fixtures to a tmpdir, builds
  a trigram index via `zoekt-index`, runs the same `evaluate` query
  through Zoekt (warm) and ripgrep, asserts the matched-path set
  equivalence (Zoekt match-set MUST be a non-empty subset of ripgrep's;
  the actual run sees both tools return 12 identical paths) AND
  asserts the wall-clock guard `zoekt_ns ≤ rg_ns + 50ms` (master-plan
  §4.2 'faster than ripgrep on warm index' regression guard). Pinned
  ref: `2a1cee1ac057de4d43c0ce316b11fd3648c9ee25` <!-- gitleaks:allow --> from
  `sourcegraph/zoekt` `main` 2026-05-09 (no formal semver releases
  upstream — confirmed via `gh api repos/sourcegraph/zoekt/releases`
  returning empty).

Bundle-shape rationale: both features are CLI/external-service tools
that do NOT speak stdio JSON-RPC; both follow the WO-0051 ripgrep
parse-only-manifest precedent under DEC-0009. Mirrors WO-0044's
ast-grep+probe pair shape (two tools, two manifests, two install
scripts, two parse/health tests across two existing test files).

## Live tool capture (executor-side smoke)

* Capture timestamp: `2026-05-08T20:05:15Z`
* `depcruise --version` → `17.4.0`
* `zoekt --version` → `flag provided but not defined: -version` (Go
  binary; bare `--version` is non-recognised; tool nonetheless exits
  cleanly and is functional — verified via `zoekt-index -index … <corpus>`
  + `zoekt -index_dir … <query>` succeeding end-to-end)
* `zoekt-index --version` → emits `USAGE: zoekt-index [options] PATHS...`
  (also non-recognised; same pattern as `zoekt`)
* `rg --version` → `ripgrep 14.1.1`

The verify-script captures the version-emit's first line into the
`[INFO]` line for the verifier log; the upstream `zoekt --version`
unrecognised-flag behaviour is documented inline in the verify script
as expected (no false-FAIL surface).

## Acceptance smoke transcripts

```
$ bash scripts/verify/P3-W10-F14.sh
[INFO] depcruise version: 17.4.0
[INFO] depcruise reported 1 no-circular violation(s) on the fabricated cycle-a/cycle-b pair
[OK] P3-W10-F14
```

```
$ bash scripts/verify/P3-W10-F15.sh
[INFO] zoekt: flag provided but not defined: -version
[INFO] zoekt-index: USAGE: zoekt-index [options] PATHS...
[INFO] ripgrep: ripgrep 14.1.1
[INFO] zoekt warm query: 19487920 ns; ripgrep query: 6461912 ns; corpus root: /tmp/tmp.wySCqJ8vfk/corpus
[INFO] zoekt matched 12 file(s); ripgrep matched 12 file(s)
[OK] P3-W10-F15
```

```
$ cargo test -p ucil-daemon --test g4_plugin_manifests g4_plugin_manifests::dependency_cruiser_manifest_parses
test g4_plugin_manifests::dependency_cruiser_manifest_parses ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out

$ cargo test -p ucil-daemon --test plugin_manifests plugin_manifests::zoekt_manifest_parses
test plugin_manifests::zoekt_manifest_parses ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out

$ cargo clippy -p ucil-daemon -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 47.55s   # exit 0

$ cargo fmt --check -p ucil-daemon
   # exit 0 (no diff)
```

Regression sentinels (with `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1
UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E=1` to neutralise the
network-touching peer tests, since these envs ARE allowed for the
executor's local smoke per WO-0086 scope_in #23 carry — verifier MUST
NOT set these envs):

```
$ UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1 UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E=1 \
    cargo test -p ucil-daemon --test g4_plugin_manifests
test g4_plugin_manifests::codegraphcontext_manifest_health_check ... ok
test g4_plugin_manifests::dependency_cruiser_manifest_parses ... ok
test result: ok. 2 passed; 0 failed

$ UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1 cargo test -p ucil-daemon --test plugin_manifests
test plugin_manifests::ast_grep_manifest_health_check ... ok
test plugin_manifests::probe_manifest_health_check ... ok
test plugin_manifests::ripgrep_manifest_parses ... ok
test plugin_manifests::zoekt_manifest_parses ... ok
test result: ok. 4 passed; 0 failed
```

## Mutation contract (M1, M2, M3, M4)

All four pre-baked mutations were applied in-place, the targeted
selector run, the SA-tagged panic / regression-FAIL observed, and
the file restored via `git checkout --` with md5sum verification.
Pre-mutation snapshots:

```
9bab542f18a7c75ebf99a9196940482b  plugins/architecture/dependency-cruiser/plugin.toml  (/tmp/wo-0086-depcruiser-orig.md5)
56bb724e094789ce0e73d1cfd3fce743  plugins/search/zoekt/plugin.toml                     (/tmp/wo-0086-zoekt-orig.md5)
1ec44e3cd05f61fa42929bae9081ab46  scripts/verify/P3-W10-F15.sh                         (/tmp/wo-0086-zoekt-verify-orig.md5)
```

### M1 — F14 manifest provides corruption

* **File**: `plugins/architecture/dependency-cruiser/plugin.toml`
* **Patch**: `sed -i 's|provides = \["architecture.dependency"\]|provides = []|' plugins/architecture/dependency-cruiser/plugin.toml`
* **Selector**: `cargo test -p ucil-daemon --test g4_plugin_manifests g4_plugin_manifests::dependency_cruiser_manifest_parses`
* **Expected fail**: `(SA2) capabilities.provides must include architecture.dependency; observed []`
* **Observed fail**: ✅ panic line matches verbatim at
  `crates/ucil-daemon/tests/g4_plugin_manifests.rs:183:9`
* **Restore**: `git checkout -- plugins/architecture/dependency-cruiser/plugin.toml`
* **md5sum verify after restore**: `md5sum -c /tmp/wo-0086-depcruiser-orig.md5` → `OK`

### M2 — F14 plugin name corruption

* **File**: `plugins/architecture/dependency-cruiser/plugin.toml`
* **Patch**: `sed -i 's|^name = "dependency-cruiser"|name = "x"|' plugins/architecture/dependency-cruiser/plugin.toml`
* **Selector**: `cargo test -p ucil-daemon --test g4_plugin_manifests g4_plugin_manifests::dependency_cruiser_manifest_parses`
* **Expected fail**: `(SA1) plugin.name must equal dependency-cruiser; observed "x"`
* **Observed fail**: ✅ panic line matches verbatim at
  `crates/ucil-daemon/tests/g4_plugin_manifests.rs:178:9` with `left: "x"  right: "dependency-cruiser"`
* **Restore**: `git checkout -- plugins/architecture/dependency-cruiser/plugin.toml`
* **md5sum verify after restore**: `md5sum -c /tmp/wo-0086-depcruiser-orig.md5` → `OK`

### M3 — F15 manifest languages corruption

* **File**: `plugins/search/zoekt/plugin.toml`
* **Patch**: `sed -i 's|languages = \[\]|languages = ["only-rust"]|' plugins/search/zoekt/plugin.toml`
* **Selector**: `cargo test -p ucil-daemon --test plugin_manifests plugin_manifests::zoekt_manifest_parses`
* **Expected fail**: `(SA3) capabilities.languages must be empty (zoekt is language-agnostic trigram); observed ["only-rust"]`
* **Observed fail**: ✅ panic line matches verbatim at
  `crates/ucil-daemon/tests/plugin_manifests.rs:226:9`
* **Restore**: `git checkout -- plugins/search/zoekt/plugin.toml`
* **md5sum verify after restore**: `md5sum -c /tmp/wo-0086-zoekt-orig.md5` → `OK`

### M4 — F15 verify-script wall-clock guard erasure

* **File**: `scripts/verify/P3-W10-F15.sh`
* **Patch (alternate form)**: `sed -i 's|^TOLERANCE_NS=50000000$|TOLERANCE_NS=-100000000000|' scripts/verify/P3-W10-F15.sh`

  The work-order proposed M4 form was `s|if \[ \$ZOEKT_NS -gt|# DELETED if [ $ZOEKT_NS -gt|` (commenting the guard
  out). That mutation form leaves the `then`/`fi` block orphaned and
  breaks the bash parser (script aborts on syntax error before reaching
  any check). The selected alternate form preserves bash syntactic
  validity AND demonstrates the guard's semantic load-bearing nature:
  with `TOLERANCE_NS = -100s` the guard fires for ANY positive Zoekt
  duration. Documented per WO-0086 scope_in #11's permissive clause
  ("if executor selects an alternate M4 form that achieves the same
  regression detection, document the form in the RFR's Mutation contract
  section") and AC20 same.
* **Selector**: `bash scripts/verify/P3-W10-F15.sh`
* **Expected fail**: `[FAIL] P3-W10-F15: zoekt warm query (<ns>) is more than 50ms slower than ripgrep (<ns>) — master-plan §4.2 'faster than ripgrep on warm index' regression`
* **Observed fail**: ✅ exit-1 with the explicit master-plan §4.2 message,
  e.g. `[FAIL] P3-W10-F15: zoekt warm query (18036969 ns) is more than 50ms slower than ripgrep (5403435 ns) ...`
* **Restore**: `git checkout -- scripts/verify/P3-W10-F15.sh`
* **md5sum verify after restore**: `md5sum -c /tmp/wo-0086-zoekt-verify-orig.md5` → `OK`

## UCIL_SKIP_SEARCH_PLUGIN_E2E (NEW env-var rationale)

WO-0086 introduces the search-group skip env-var
`UCIL_SKIP_SEARCH_PLUGIN_E2E` for the FIRST time, parallel to
`UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E` (WO-0072) and
`UCIL_SKIP_QUALITY_PLUGIN_E2E` (WO-0080). Pre-existence check at
WO-execution time:
`grep -rE 'UCIL_SKIP_SEARCH_PLUGIN_E2E' . --exclude-dir=target --exclude-dir=node_modules`
returned empty (introduced fresh by this WO). Documented in:

* `scripts/verify/P3-W10-F15.sh` header comment block.
* `plugins/search/zoekt/plugin.toml` header comment block.

The `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1` global skip and the architecture-
group `UCIL_SKIP_ARCHITECTURE_PLUGIN_E2E=1` env-var pre-exist and are
honoured by the F14 cargo-test for free (the test never spawns a
subprocess so the skip-via-env code path inside
`tests/g4_plugin_manifests.rs` is exercised only on the existing
codegraphcontext_manifest_health_check). F15's parse-only test
similarly does not consult any env-var. The verify scripts
(`scripts/verify/P3-W10-F14.sh`, `scripts/verify/P3-W10-F15.sh`) are
the entry points where the new search-group env-var matters.

## Disclosed deviations (per WO-0070 / 0083 / 0084 spirit-over-literal)

1. **AC `grep -qE '#\[test\][[:space:]]*fn ...'` regex form** —
   acceptance regex `#\[test\][[:space:]]*fn dependency_cruiser_manifest_parses`
   is line-oriented under `grep -qE`, but the test uses the canonical
   Rust style with `#[test]` and `fn ...` on separate lines (mirrors
   `ripgrep_manifest_parses` precedent at `tests/plugin_manifests.rs:157`,
   which has been verified by the same AC pattern in WO-0051 and
   subsequent regressions). A multiline-aware grep (`grep -Pz` or
   ripgrep's `-U --multiline-dotall`) matches the regex; the standard
   single-line form does not. This deviation is consistent with the
   pre-existing `ripgrep_manifest_parses` test and with WO-0070
   §planner #4 spirit-over-literal precedent (`feat..main = 0` →
   `git log feat ^main --merges` returning empty). Verifier should
   accept the structural placement.

2. **`zoekt --version` is upstream-non-recognised** — sourcegraph/zoekt
   does not implement a standard `--version` flag (the binary emits
   `flag provided but not defined: -version` and exits 2). The verify
   script captures the first line via `zoekt --version 2>&1 | head -n1`
   and prints it to the verifier log unconditionally (the `|| true`
   suffix on the version-capture suppresses the non-zero exit). This
   is documented inline in the verify script's INFO-line comment block
   and is identical behaviour for `zoekt-index`. Functional smoke
   (zoekt-index build + zoekt query against the tmpdir corpus) verifies
   the binaries are truly present and operational.

3. **M4 alternate form** (per Mutation contract M4 above): the WO-proposed
   M4 form leaves bash syntax orphaned; the alternate form
   (`TOLERANCE_NS = -100s`) preserves syntactic validity AND demonstrates
   the load-bearing nature of the guard expression itself. Documented
   per WO-0086 scope_in #11's permissive "alternate M4 form" clause.

4. **Coverage-gate sccache RUSTC_WRAPPER carry-forward** (scope_in #21):
   per the 41+ WO-deep standing protocol, the line-coverage gate-script
   reports `line=0%` for ucil-daemon under sccache; authoritative
   measurement is via `env -u RUSTC_WRAPPER cargo llvm-cov ... | jq
   '.data[0].totals.lines.percent'`. Out of WO-0086 scope.

5. **AC30/AC31 phase-1/phase-2 effectiveness-gate flake** (scope_in #22):
   carry-forward standing item; not addressed by WO-0086 deliverables.

## Files added / modified

* `plugins/architecture/dependency-cruiser/plugin.toml` (NEW, 49 lines)
* `plugins/search/zoekt/plugin.toml` (NEW, 63 lines)
* `scripts/devtools/install-dependency-cruiser.sh` (NEW, 35 lines, executable)
* `scripts/devtools/install-zoekt.sh` (NEW, 51 lines, executable)
* `scripts/verify/P3-W10-F14.sh` (REPLACED — was TODO-stub; new 109 lines, executable)
* `scripts/verify/P3-W10-F15.sh` (REPLACED — was TODO-stub; new 174 lines, executable)
* `crates/ucil-daemon/tests/g4_plugin_manifests.rs` (modified — `dependency_cruiser_manifest_parses` test added inside existing `mod g4_plugin_manifests { ... }` block; leading rustdoc updated to mention the new parse-only test; existing `codegraphcontext_manifest_health_check` untouched)
* `crates/ucil-daemon/tests/plugin_manifests.rs` (modified — `zoekt_manifest_parses` test added inside existing `mod plugin_manifests { ... }` block; leading rustdoc updated; existing ast-grep / probe / ripgrep tests untouched)

`tests/fixtures/**`, `ucil-build/feature-list.json`, master plan, and
`scripts/gate/**` are untouched. Forbidden-paths confirmed clean via
`git diff --name-only main...HEAD -- tests/fixtures/** ucil-build/feature-list.json
ucil-master-plan-v2.1-final.md scripts/gate/** scripts/flip-feature.sh`
returning empty.

## Commit cadence (DEC-0005 module-coherence per-feature commits)

```
8d2ad95 feat(plugins): add zoekt G2 search plugin manifest with trigram smoke
90f61ef feat(plugins): add dependency-cruiser G4 architecture plugin manifest
```

Two feat commits, one per feature, each cohesive (manifest + install
script + parse-only test + verify-script smoke landing as one unit per
DEC-0005). Plus this RFR commit. Branch carries zero merge commits
(`git log feat/WO-0086-deps-cruiser-and-zoekt-manifests ^main --merges`
returns empty per AC23).
