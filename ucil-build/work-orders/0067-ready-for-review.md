# WO-0067 — Ready for Review

- **Features**: P3-W9-F01 (deterministic classifier), P3-W9-F02 (CEQP reason parser)
- **Branch**: `feat/WO-0067-classifier-and-reason-parser`
- **Final implementation commit sha**: `fc50ef0` (RFR landed at `<TBD>`; `f9fd29d`, `68e505f`, `4f968d8`, `d24a08c` are out-of-scope verification-report refreshes pushed by phase-1/phase-2 effectiveness-evaluator + gate-check child agents per scope_in #42 carve-out)
- **Frozen acceptance selectors**:
  - `cargo test -p ucil-core fusion::test_deterministic_classifier` (P3-W9-F01)
  - `cargo test -p ucil-core ceqp::test_reason_parser` (P3-W9-F02)
- **Verify scripts**:
  - `bash scripts/verify/P3-W9-F01.sh`
  - `bash scripts/verify/P3-W9-F02.sh`
- **Master plan citations**: §3.2 lines 211-237 (22-tool MCP surface), §6.2 lines 643-658 (10×8 query-type weight matrix), §7.1 lines 693-695 (deterministic-fallback path), §8.3 lines 772-774 (CEQP reason parser contract), §8.6 lines 817-822 (most-permissive bonus-context default), §18 Phase 3 Week 9 lines 1799-1806 (week-9 deliverable list).

## What landed

### F01 — `crates/ucil-core/src/fusion.rs`

- **`pub enum QueryType`** with the 10 §6.2-frozen variants in canonical declaration order (`UnderstandCode`, `FindDefinition`, `FindReferences`, `SearchCode`, `GetContextForEdit`, `TraceDependencies`, `BlastRadius`, `ReviewChanges`, `CheckQuality`, `Remember`).  Derives `Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize` with `#[serde(rename_all = "snake_case")]`.  `#[default]` is on `UnderstandCode` (most-permissive default per §8.6).
- **`pub struct ClassifierOutput`** with `query_type: QueryType, intent_hint: Option<String>, domain_tags: Vec<String>, group_weight_overrides: BTreeMap<G2Source, f32>`.  All metadata fields are reserved for future enrichment (LLM `QueryInterpreter` agent / cross-group RRF override surface) per scope_in #2.
- **`const TOOL_NAME_MAP: &[(&str, QueryType)]`** — 12 entries covering every §3.2 user-facing tool currently shipped or in-flight (lines 215-226).
- **`const KEYWORD_RULES: &[(&str, QueryType)]`** — 28 ordered patterns implementing the precedence ladder per scope_in #3.  `"refactor" | "rename" | "cleanup"` route to `GetContextForEdit` (no dedicated `Refactor` row in §6.2).
- **`const QUERY_WEIGHT_MATRIX: [[f32; 8]; 10]`** — the §6.2 lines 649-658 weight table verbatim with one row per `QueryType` variant.  Sentinel: row 9 (`Remember`) is `[0, 0, 3.0, 0, 0, 0, 0, 0]`.
- **`pub const fn group_weights_for(QueryType) -> [f32; 8]`** — row lookup keyed by `query_type as usize`.  `const fn` so future build-time consumers (`P3-W9-F04` cross-group RRF) can use it in const contexts.
- **`pub fn classify_query(tool_name: &str, reason_keywords: &[&str]) -> ClassifierOutput`** — implements the 3-step precedence ladder: (1) tool_name primary signal, (2) keyword fallback over the lower-cased + space-padded slice, (3) default `UnderstandCode`.  Pure: no IO, no async, no logging.
- **`fn test_deterministic_classifier`** at module root per `DEC-0007`, with sub-assertions SA1..SA6:
  - SA1 — 12 tool_name → QueryType mappings (load-bearing against M1).
  - SA2 — keyword fallback ("references" → FindReferences) + phrase fallback ("blast radius" → BlastRadius).
  - SA3 — default UnderstandCode + ClassifierOutput field defaults when both tool AND keywords are unknown.
  - SA4 — group_weights_for shape across UnderstandCode, FindDefinition, BlastRadius (load-bearing against M2 row swap).
  - SA5 — Remember sentinel row `[0, 0, 3.0, 0, 0, 0, 0, 0]` — canary for matrix-row-shift bugs.
  - SA6 — JSON round-trip on every QueryType variant + wire-format spot check (`FindDefinition → "find_definition"`).

### F02 — NEW `crates/ucil-core/src/ceqp.rs`

- Module-level rustdoc cites master-plan §8.3 lines 772-774 + §7.1 lines 693-695.
- **`pub enum Intent`** with the 5 §8.3-frozen variants (`AddFeature, FixBug, Refactor, Understand, Review`).  `#[default]` on `Understand`.  `#[serde(rename_all = "snake_case")]`.
- **`pub enum PlannedAction`** with `Edit, Read, Explain, Other`.  `#[default]` on `Read` (read-only is the safest default).
- **`pub struct ParsedReason`** with `intent, domains, planned_action, knowledge_gaps`.  Derives `Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize`.
- **`pub fn parse_reason(reason: &str) -> ParsedReason`** — implements the 5-step pipeline:
  1. Lowercase the input ONCE; pad with leading + trailing space.
  2. `classify_intent` precedence `Refactor > FixBug > AddFeature > Review > Understand` (load-bearing against M3 verifier mutation).
  3. `classify_action` precedence `Edit > Read > Explain` (default `Read`).
  4. `extract_domains` splits on whitespace + the punctuation set `, . ; : ! ? ( )` and matches against the 25-token canonical UCIL vocabulary; first-occurrence order preserved, no duplicates.
  5. `extract_gaps` scans for `"don't know "`, `"unsure about "`, `"need to learn "`, `"unfamiliar with "` and captures up to 40 ASCII bytes of `[a-z0-9_\- ]` until a sentence-end byte (`. , ; : ! ? \n`) or end-of-input — UTF-8 multibyte safe via the high-bit-set scan termination.
- **NO regex crate import**; **no new dependency** added to `crates/ucil-core/Cargo.toml`.  All scans use `str::contains` + manual byte-level scan per scope_in #34.
- **`fn test_reason_parser`** at module root per `DEC-0007`, sub-assertions SA1..SA8:
  - SA1 — intent classification across all 5 variants + the default-`Understand` baseline.
  - SA2 — intent precedence (`Refactor` MUST win over FixBug + AddFeature in the same sentence).  Load-bearing against M3.
  - SA3 — domain extraction in first-occurrence order (`["tokio", "http", "rust"]`) + empty-domain case.
  - SA4 — action classification across Edit, Explain, Read.
  - SA5 — knowledge_gap extraction with `"don't know exponential semantics yet"` and `"unsure about the lancedb schema"` plus the empty-gap case.
  - SA6 — JSON round-trip on `ParsedReason::default()`.
  - SA7 — empty input returns the all-default `ParsedReason`.
  - SA8 — case insensitivity (`"FIX the BUG"` classifies as `FixBug`).

### Wiring

- **`crates/ucil-core/src/lib.rs`** — added `pub mod ceqp;` directly above `pub mod fusion;` (alphabetical).
- **No re-exports added** — F01 / F02 surface lives at `ucil_core::fusion::*` and `ucil_core::ceqp::*`.  Re-exports are deferred to the consumer WO that wires them through the daemon orchestration layer (P3-W9-F03 / P3-W9-F04).

### Verify scripts

- **`scripts/verify/P3-W9-F01.sh`** — frozen-symbol grep guards on `pub enum QueryType`, `pub fn classify_query`, `pub fn group_weights_for`, plus the frozen selector `^[[:space:]]*fn test_deterministic_classifier\(\)` at module root, plus the cargo test runner.  Exits 0 with `[OK] P3-W9-F01 deterministic classifier wired and verified`.
- **`scripts/verify/P3-W9-F02.sh`** — frozen-symbol grep guards on `pub fn parse_reason`, `pub enum Intent`, `pub struct ParsedReason`, `pub mod ceqp` in `lib.rs`, plus the frozen selector `^[[:space:]]*fn test_reason_parser\(\)` at module root, plus the cargo test runner.  Exits 0 with `[OK] P3-W9-F02 CEQP reason parser wired and verified`.

## What I verified locally

| AC | Description | Result |
|----|-------------|--------|
| AC01 | `pub enum QueryType` exists with 10 variants in §6.2 declaration order + frozen derives | ✅ |
| AC02 | `pub struct ClassifierOutput` has all 4 fields with the right types | ✅ |
| AC03 | `pub fn classify_query(&str, &[&str]) -> ClassifierOutput` exists with the precedence ladder | ✅ |
| AC04 | `pub const fn group_weights_for(QueryType) -> [f32; 8]` exists, returns the row | ✅ |
| AC05 | `const QUERY_WEIGHT_MATRIX: [[f32; 8]; 10]` exists, rows in §6.2 line 649-658 order | ✅ (SA4 + SA5) |
| AC06 | `tool_name → QueryType` map covers ≥12 §3.2 tools | ✅ (SA1 covers all 12) |
| AC07 | `pub fn test_deterministic_classifier()` exists, `#[test]`, frozen selector resolves | ✅ |
| AC08 | Test exercises SA1..SA6 inclusive | ✅ |
| AC09 | NEW module `crates/ucil-core/src/ceqp.rs` exists, declared via `pub mod ceqp;` | ✅ |
| AC10 | `pub enum Intent` with 5 variants, default `Understand`, `snake_case` serde | ✅ |
| AC11 | `pub enum PlannedAction` with 4 variants, default `Read`, `snake_case` serde | ✅ |
| AC12 | `pub struct ParsedReason` with 4 fields + frozen derives | ✅ |
| AC13 | `pub fn parse_reason(&str) -> ParsedReason` implements the 5-step pipeline | ✅ |
| AC14 | `pub fn test_reason_parser()` exists, `#[test]`, frozen selector resolves | ✅ |
| AC15 | Test exercises SA1..SA8 inclusive | ✅ |
| AC16 | `cargo test -p ucil-core fusion::test_deterministic_classifier` passes from clean | ✅ |
| AC17 | `cargo test -p ucil-core ceqp::test_reason_parser` passes from clean | ✅ |
| AC18 | `cargo clippy --all-targets -p ucil-core -- -D warnings` clean | ✅ |
| AC19 | `cargo fmt --all -- --check` clean | ✅ |
| AC20 | `cargo test --workspace --no-fail-fast` green (after symlinking ml/models/coderankembed/ artefacts per WO-0058+ standing protocol) | ✅ |
| AC21 | `bash scripts/gate/phase-1.sh` exits 0 | ⚠️ exit=1 — only failure is `coverage gate: ucil-core` (KNOWN CARRY from WO-0058+ standing protocol per scope_out #14; see Disclosed Deviations) |
| AC22 | `bash scripts/gate/phase-2.sh` exits 0 | ⚠️ exit=1 — only failures are `coverage gate: ucil-core` AND `coverage gate: ucil-embeddings` (both SAME carry-over from WO-0058+) |
| AC23 | Coverage gate INFORMATIONAL: `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-core --summary-only --json | jq '.data[0].totals.lines.percent'` reports `>= 85.0` | ✅ **97.21%** (well above 85% floor; new modules + tests added coverage — `fusion.rs` 93.92%, `ceqp.rs` 97.06%) |
| AC24 | NEW script `scripts/verify/P3-W9-F01.sh` exists, executable, shebang `#!/usr/bin/env bash`, `set -euo pipefail`, `IFS=$'\n\t'` | ✅ |
| AC25 | Verify script runs frozen-selector test + 4 rename-drift greps | ✅ |
| AC26 | `bash scripts/verify/P3-W9-F01.sh` exits 0 | ✅ (`[OK] P3-W9-F01 deterministic classifier wired and verified`) |
| AC27 | NEW `scripts/verify/P3-W9-F02.sh` with the same shebang/IFS posture | ✅ |
| AC28 | Verify script runs frozen-selector test + 5 rename-drift greps (`pub fn parse_reason`, `pub enum Intent`, `pub struct ParsedReason`, `fn test_reason_parser`, `pub mod ceqp` in lib.rs) | ✅ |
| AC29 | `bash scripts/verify/P3-W9-F02.sh` exits 0 | ✅ (`[OK] P3-W9-F02 CEQP reason parser wired and verified`) |
| AC30 | Pre-flight word-ban grep: NO LITERAL `mock\|fake\|stub` in NEW non-`#[cfg(test)]` code in fusion.rs / ceqp.rs | ✅ (verified via `grep -iE 'mock\|fake\|stub'` returning zero matches in the production scope) |
| AC31 | NO new deps in `crates/ucil-core/Cargo.toml` | ✅ (`git diff main -- crates/ucil-core/Cargo.toml \| grep -E '^\+[a-z_-]+\s*='` returns zero matches) |
| AC32 | NO `regex` crate import anywhere in `crates/ucil-core/src/` | ✅ (`grep -rE '^use regex\|extern crate regex' crates/ucil-core/src/` returns zero matches) |
| AC33 | Conventional commits with required trailers `Phase: 3`, `Feature: P3-W9-F01\|F02`, `Work-order: WO-0067`, `Co-Authored-By: Claude Opus 4.6 (1M context)` | ✅ (4 implementation commits) |
| AC34 | All commits pushed to origin | ✅ |

## Gate detail (informational)

### Phase-1 gate (AC21) — non-coverage results

```
[OK]   cargo test --workspace
[OK]   clippy -D warnings
[OK]   MCP 22 tools registered
[OK]   Serena docker-live integration
[OK]   diagnostics bridge live
[OK]   effectiveness (phase 1 scenarios)
[OK]   multi-lang probes
[FAIL] coverage gate: ucil-core         ← carry-over (sccache / RUSTC_WRAPPER); real coverage 97.21%
[OK]   coverage gate: ucil-daemon       (line=88%)
[OK]   coverage gate: ucil-treesitter   (line=89%)
[OK]   coverage gate: ucil-lsp-diagnostics (line=94%)
```

### Phase-2 gate (AC22) — non-coverage results

```
[OK]   cargo test --workspace
[OK]   effectiveness (phase 2 scenarios)
[OK]   multi-lang probes
[OK]   real-repo smoke
[FAIL] coverage gate: ucil-core         ← same carry-over; real coverage 97.21%
[OK]   coverage gate: ucil-daemon       (line=88%)
[OK]   coverage gate: ucil-treesitter   (line=89%)
[OK]   coverage gate: ucil-lsp-diagnostics (line=94%)
[FAIL] coverage gate: ucil-embeddings   ← same carry-over (WO-0066 precedent); real coverage 89.46%
```

Phase-2 runs additional optional checks (`plugin-hot-cold.sh`, `bench-embed.sh`, `golden-fusion.sh`, `recall-at-10.sh`) that are silently skipped because their script files do not exist in this repo at HEAD — same posture as on `main` and as accepted by the WO-0066 verifier.

## Mutation contract (delegated to verifier per WO scope_in #36)

Mutations are NOT applied in-line per WO-0061 line 690 + WO-0066 precedent — `git checkout --` is the verifier's restoration mechanism.

- **M1** — classifier tool-name bypass: in `classify_query`, replace the tool_name match arm body with `_ => QueryType::UnderstandCode`.  Concrete patch: replace the entire `for &(name, qt) in TOOL_NAME_MAP { … }` loop with `let _ = tool_name;`.  Expected: SA1 fails on the first unmapped tool (e.g. `find_definition` → expected `FindDefinition`, got `UnderstandCode`).  Verifier restoration: `git checkout -- crates/ucil-core/src/fusion.rs`.
- **M2** — group-weight matrix row swap: swap rows 0 and 1 in `QUERY_WEIGHT_MATRIX` (the `understand_code` line and the `find_definition` line — visible at the comments `// §6.2 line 649` and `// §6.2 line 650`).  Expected: SA4 fails on the UnderstandCode row mismatch.  Verifier restoration: `git checkout --`.
- **M3** — parser intent precedence break: prepend `return Intent::Understand;` as the first statement of `classify_intent` in `crates/ucil-core/src/ceqp.rs` (so Understand "wins" before any other rule has a chance).  Expected: SA1 fails on the `"refactor the storage module"` case (got Understand, expected Refactor) AND SA2 fails on the precedence test.  Verifier restoration: `git checkout --`.

## Disclosed deviations (carry from WO-0058..WO-0066 standing protocol)

1. **Coverage workaround** (per scope_out #14, now 22nd consecutive WO under the same workaround):  `scripts/verify/coverage-gate.sh` does NOT use `env -u RUSTC_WRAPPER` and the sccache wrapper produces near-empty `.profraw` files, yielding `ucil-core line=5%` / `ucil-embeddings line=0%` from the gate.  The actual coverage when measured via the AC23 standing command — `env -u RUSTC_WRAPPER cargo llvm-cov --package <crate> --summary-only --json | jq '.data[0].totals.lines.percent'` — is:
   - `ucil-core` → **97.21%** (well above the 85% gate floor and the 85% per-crate floor)
   - `ucil-embeddings` → **89.46%** (well above 85%)
   Per-file new-code coverage on this WO: `fusion.rs` 93.92%, `ceqp.rs` 97.06% (both NEW reads + tests).  Follow-up trigger remains a `coverage-gate.sh` harness improvement (out of scope for WO-0067 per scope_out #14).

2. **CodeRankEmbed model artefacts** (`ml/models/coderankembed/{model.onnx, tokenizer.json}`) are gitignored.  The worktree starts without them and `models::test_coderankembed_inference` would fail under `cargo test --workspace`.  Verifier should `cp` (or `ln -sf`) the artefacts from the main repo's `ml/models/coderankembed/` into the worktree's `ml/models/coderankembed/`, OR run `bash scripts/devtools/install-coderankembed.sh`.  Same protocol carries from WO-0058+.

3. **Phase-1 effectiveness re-run flake** (cross-run swap on `caller_completeness` for the `nav-rust-symbol` scenario): a fresh phase-1 effectiveness-evaluator child agent ran during the AC21 gate execution and committed `f9fd29d docs(verification-reports): refresh phase-1 effectiveness report — FAIL` to the WO-0067 feat branch (per scope_in #42 carve-out — `verification-reports/**` is NOT in `forbidden_paths`).  The child agent also filed escalation `ucil-build/escalations/20260507T1930Z-effectiveness-nav-rust-symbol-doctest-caller-flake.md` (severity: harness-config, blocks_loop: false) — the swap is run-to-run agent stochasticity over whether to enumerate the doctest call site (`src/http_client.rs:26` inside the `///` rustdoc on `retry_with_backoff`); the underlying P3-W9-F01/F02 work is unaffected.  The phase-1 gate's `effectiveness (phase 1 scenarios)` sub-check still resolved `[OK]` at the gate level.  This same orthogonal flake ALSO applies to AC22 phase-2 effectiveness (resolved `[OK]` at the gate level for the latest run).

4. **Verification-report refreshes on the WO-0067 branch** (per scope_in #42 — explicitly NOT added to `forbidden_paths`): commits `f9fd29d`, `68e505f`, `4f968d8`, `d24a08c` are gate-side artefacts pushed by phase-1/phase-2 effectiveness-evaluator and gate-check child agents during AC21/AC22 execution.  Critic / verifier MAY flag as a Warning but accept as benign per the scope_in #42 carve-out.

## Files changed

```
crates/ucil-core/src/ceqp.rs          (NEW; 547 lines incl. frozen test)
crates/ucil-core/src/fusion.rs        (+322 lines; 1 new public enum, 1 new public struct,
                                       3 new public fns, 4 new const tables, 1 frozen test)
crates/ucil-core/src/lib.rs           (+1 line; pub mod ceqp;)
scripts/verify/P3-W9-F01.sh           (NEW, executable; 88 lines)
scripts/verify/P3-W9-F02.sh           (NEW, executable; 100 lines)
ucil-build/work-orders/0067-ready-for-review.md   (this file)
```

## Commit log (implementation-only — verification-report refreshes excluded)

```
fc50ef0  chore(scripts): add verify/P3-W9-F01.sh + verify/P3-W9-F02.sh
1521525  feat(core): add ceqp module with parse_reason + Intent/PlannedAction/ParsedReason
22a87c4  test(core): add fusion::test_deterministic_classifier (SA1..SA6)
43f0c93  feat(core): add QueryType enum + classify_query + QUERY_WEIGHT_MATRIX
```

All four implementation commits carry the required Conventional-Commits trailers (`Phase: 3`, `Feature: P3-W9-F01` and/or `P3-W9-F02`, `Work-order: WO-0067`, `Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>`).

## Verification recipe (for the verifier — fresh worktree)

```bash
# 1. set up worktree from main, then merge the feat branch
git worktree add ../ucil-verify-WO-0067 -b verify/WO-0067 main
cd ../ucil-verify-WO-0067
git merge --ff-only feat/WO-0067-classifier-and-reason-parser

# 2. ensure protoc is on PATH (per WO-0058+ standing protocol — lancedb dep)
export PROTOC=~/.local/bin/protoc PROTOC_INCLUDE=~/.local/include

# 3. symlink ml model artefacts (per WO-0058+ standing protocol)
ln -sf $REPO_MAIN/ml/models/coderankembed/model.onnx ml/models/coderankembed/model.onnx
ln -sf $REPO_MAIN/ml/models/coderankembed/tokenizer.json ml/models/coderankembed/tokenizer.json

# 4. run the frozen acceptance selectors
cargo test -p ucil-core fusion::test_deterministic_classifier
cargo test -p ucil-core ceqp::test_reason_parser

# 5. run the verify scripts
bash scripts/verify/P3-W9-F01.sh
bash scripts/verify/P3-W9-F02.sh

# 6. run the standing-protocol coverage check (AC23)
env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-core --summary-only --json | jq '.data[0].totals.lines.percent'
# expect >= 85.0  (actual: 97.21%)

# 7. apply each mutation, run the appropriate verify script, expect FAIL, restore
#    (M1) sed-out the for-loop body in classify_query                    ; expect SA1 fail
#    (M2) swap rows 0 and 1 in QUERY_WEIGHT_MATRIX                      ; expect SA4 fail
#    (M3) prepend `return Intent::Understand;` to classify_intent       ; expect SA2 fail
git checkout -- crates/ucil-core/src/fusion.rs crates/ucil-core/src/ceqp.rs
```
