# WO-0048 — Ready for Review

**Work-order**: `WO-0048` (`g1-result-fusion`)
**Feature**: `P2-W7-F02` (G1 result-fusion algorithm, master-plan §5.1 lines 430-442)
**Branch**: `feat/WO-0048-g1-result-fusion`
**Final commit**: `4fc1684f95908929dadd75cc0b4498b5eabadff0`
**Commit count**: 4 (types / `fuse_g1` / test+lib.rs re-exports / verify script) — meets AC26 ≥3 floor.

## Allow-list diff (`git diff --name-only main...HEAD`)

```
crates/ucil-daemon/src/executor.rs
crates/ucil-daemon/src/lib.rs
scripts/verify/P2-W7-F02.sh
```

`Cargo.toml`, `Cargo.lock`, `tests/fixtures/**`, `feature-list.json`, `ucil-master-plan-v2.1-final.md`, every `forbidden_paths` entry: untouched (verified by `git diff --name-only main...HEAD -- '<path>'` returning empty for each).

## What I verified locally

### Build + lint
- **AC01**: `cargo build -p ucil-daemon` → exit 0.
- **AC02**: `cargo clippy -p ucil-daemon --all-targets -- -D warnings` → exit 0. New rustdoc on `fuse_g1` / fusion types passes `clippy::doc_markdown` (every uppercase identifier is backticked); pre-flight grep proved by clippy passing.

### Frozen-selector test
- **AC03**: `cargo test -p ucil-daemon executor::test_g1_result_fusion -- --nocapture` → `1 passed; 0 failed`.
- **AC04**: `grep -nE '^pub async fn test_g1_result_fusion' crates/ucil-daemon/src/executor.rs` → line 2391 (module root, NOT inside `mod tests {}`).

### Sub-assertion coverage (one extra over the WO checklist)
- **AC05** location merge: `fused.entries.len() == 2` ✓
- **AC06** field union: `[ast_kind, hover_doc, pattern, signature]` keys; `contributing_sources == [Serena, TreeSitter, AstGrep]` ✓
- **AC07** authority resolution: `fields["signature"] == "fn foo() -> i32"`; one `G1Conflict { field: "signature", winner: Serena, losers: [(AstGrep, "fn foo()")] }` ✓; `ast_kind` records NO conflict ✓
- **AC08** disposition pass-through: `len() == 4`, all `Available`, input order `[TreeSitter, Serena, AstGrep, Diagnostics]`, `master_timed_out == false` ✓
- **Extra (sub-assertion 5)**: the `(util.rs, 30, 35)` Diagnostics-only entry has `contributing_sources == [Diagnostics]`, carries the `diagnostic` field verbatim, `conflicts == []`. Not strictly required by the WO; landed for symmetric coverage.

### Regression suite
- **AC09**: `cargo test -p ucil-daemon executor::test_g1_parallel_execution` → `1 passed`.
- **AC10**: every Phase-2 W6 module-root test individually (`test_hot_cold_lifecycle` / `test_manifest_parser` / `test_lifecycle_state_machine` / `test_hot_reload` / `test_circuit_breaker`) → `1 passed` each. Note: `cargo test` only accepts ONE positional arg per invocation, so I ran each separately. Verifier: same applies.
- **AC11**: `cargo test -p ucil-daemon --test plugin_manager` → `3 passed`. `UCIL_SKIP_EXTERNAL_PLUGIN_TESTS=1 cargo test -p ucil-daemon --test plugin_manifests` → `2 passed`.
- **AC12**: `cargo test -p ucil-daemon --test e2e_mcp_stdio --test e2e_mcp_with_kg` → `1 passed` each.
- **AC13**: `cargo test --test test_plugin_lifecycle` → `3 passed`.
- **AC14**: `cargo test --test test_lsp_bridge` → `5 passed`.
- **AC15**: `cargo test --workspace --no-fail-fast` → 35 `test result:` lines, all `0 failed` (Rust unit + integration + doc-tests, including the new `executor::fuse_g1` doctest).

### Coverage (AC16) — known broken script, manual proof
- **AC16**: `bash scripts/verify/coverage-gate.sh ucil-daemon 85 75` → fails per the standing `RUSTC_WRAPPER` + corrupt-header profraw issue (escalation `20260419-0152-monitor-phase1-gate-red-integration-gaps.md` still open; this is the **11th consecutive WO** to use the workaround).
- **Manual proof** via `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --summary-only --json`:
  - `ucil-daemon` totals: `lines.percent = 89.7637 %` (above 85% floor by +4.76 pp).
  - `executor.rs` alone: `lines.percent = 93.6842 %` (above floor by +8.68 pp).
- Verifier should apply the same `env -u RUSTC_WRAPPER cargo llvm-cov ...` workaround as in WO-0042 / 0043 / 0044 / 0045 / 0046 / 0047 / cumulative.

### Stub-scan + forbidden-paths
- **AC17**: `grep -cE 'todo!\(\)|unimplemented!\(\)|panic!\(".*not yet|TODO|FIXME' crates/ucil-daemon/src/executor.rs` → `0` (current) vs `0` (main) → zero new hits. The `panic!` calls inside the test (`make_entry`'s "fields argument must be a JSON object" arm) do NOT match the `panic!\(".*not yet` pattern by design.
- **AC18**: `git diff --name-only main...HEAD` matches the WO's exact 3-file allow-list (no `Cargo.lock` because no new dep).
- **AC19**: `git diff --name-only main...HEAD -- '*.toml'` returns empty.
- **AC20**: `grep -nE 'fuse_g1|G1Conflict|G1FusedEntry|G1FusedLocation|G1FusedOutcome|G1FusionEntry' crates/ucil-daemon/src/lib.rs` → all 6 symbols present on the single `pub use executor::{...}` line; alphabetical ordering preserved within the executor:: block.
- **AC23-AC25**: `tests/fixtures/**`, `feature-list.json`, `feature-list.schema.json`, `ucil-master-plan-v2.1-final.md` all return empty diffs.

### Pre-baked mutations (the authoritative anti-laziness layer per DEC-0007)
- **AC21 — fusion body neutered**: literal sed produced the predicted `#![deny(warnings)] / unreachable_code` cascade (per WO-0046 lessons line 245). I applied the **runtime-only variant** as documented in the WO scope_in line 25 — added `#[allow(unreachable_code)]` on the function and prepended `let _ = outcome; return G1FusedOutcome::default();`. Result: panic at AC05 location-merge assertion (`expected 2 fused entries; got 0: entries=[]`). `git checkout -- crates/ucil-daemon/src/executor.rs` restored. Re-run: green.
- **AC22 — authority resolution disabled**: literal sed `'s|G1ToolKind::Serena => 0,|G1ToolKind::Serena => 3,|'` worked first try (no formatting fragility). Result: panic at sub-assertion 2 (`contributing_sources must be authority-ordered [Serena, TreeSitter, AstGrep]; got [TreeSitter, AstGrep, Serena]`). The WO predicted the panic would land at the AC07 signature value, but sub-assertion 2 fires first (it asserts on the authority-ordered `contributing_sources` list, which now reorders Serena last). Either way, the test catches the mutation — both panics prove `authority_rank` is load-bearing. `git checkout -- crates/ucil-daemon/src/executor.rs` restored. Re-run: green.

### Branch state
- **AC26**: 4 commits on the feature branch (1 types / 2 `fuse_g1` / 3 test+lib.rs / 4 verify script). Above the WO's ≥3 minimum.
- **AC27**: `git rev-parse HEAD` (`4fc1684f9...`) matches `git rev-parse @{u}`. `git status --porcelain` empty after restoring the auto-generated `ucil-build/verification-reports/coverage-ucil-daemon.md` (touched as a side-effect of the failing coverage-gate.sh, NOT part of WO).

### Verify script
- `bash scripts/verify/P2-W7-F02.sh` → `[OK] P2-W7-F02` exit 0. Mirrors the P2-W7-F01.sh template: confirms module-root selector, runs the test with the cargo-test/nextest summary regex (`'test result: ok\. .* 0 failed|[0-9]+ tests? passed'`), prints panic line on failure, optionally shellchecks itself.

## Notes for verifier / critic

1. **`G1FusedLocation` carries an EXTRA `Default` derive** beyond what `scope_in` line 14 enumerates. WO `scope_in` line 17 requires `G1FusedEntry: Default`, which transitively requires `G1FusedLocation: Default` because `G1FusedEntry.location: G1FusedLocation`. `PathBuf::default()` (empty path) + `u32::default()` (0) make this trivially derivable. Documented in the rustdoc.
2. **`#[allow(clippy::type_complexity)]` on the `groups` let binding** inside `fuse_g1` (line ~1402). Justified by the inline comment: a top-level type alias with a `'_` lifetime would noise up the public surface for a single-use intermediate. Same pattern as previous WOs.
3. **`authority_rank` is `const fn`** to satisfy `clippy::missing_const_for_fn` (clippy::nursery surfaces this when the function body is a pure `match`).
4. **Sub-assertion 5** (Diagnostics-only entry at `(util.rs, 30, 35)`) is one extra assertion beyond the WO's 4-sub-assertion checklist. It tightens the location-merge contract by asserting on the OTHER fused entry too — without it, AC05 only counts to 2 without observing the second entry's contents. Cheap insurance against a regression that would emit only the (10, 20) entry.
5. **`per_source` accumulator** uses `Vec<(G1ToolKind, Vec<G1FusionEntry>)>` (owned) rather than borrowing into `outcome.results` directly. The owned shape lets us defer borrowing the per-entry `&Map` until the BTreeMap-grouping pass, avoiding a self-referential borrow when entries from the same source span multiple locations.
6. **Critic-readiness** — every public type ships with rustdoc citing master-plan section/lines, every doc-comment-mention of an uppercase identifier (`G1`, `Serena`, `TreeSitter`, `AstGrep`, `Diagnostics`, `MUST`, `SCIP`, etc.) is either inside backticks or an English word that pedantic clippy already cleared. Manual eyeballed pass on the changed range.

## Out of scope for this WO

Per `scope_out`: no production wiring of real Serena MCP / ast-grep MCP / `ucil_treesitter::Parser` / `ucil-lsp-diagnostics::bridge` clients (deferred to **P2-W7-F05 `find_references`**). No `find_references` MCP tool wiring. No SCIP/Joern. No retry/backoff inside `fuse_g1` (it's a pure CPU-bound transform). No agent-narrative synthesis. No new dependencies. No new ADR.

The lib.rs preamble paragraph for WO-0048 explicitly cites the F05 deferral so the next reader does not double-take.
