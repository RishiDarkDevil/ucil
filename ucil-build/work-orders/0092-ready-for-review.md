---
work_order: WO-0092
slug: review-changes-mcp-tool
feature_ids: [P3-W11-F11]
phase: 3
week: 11
final_commit: 56dcf5d06a2a60c9b77c0e642a19dac2f0a57d92
branch: feat/WO-0092-review-changes-mcp-tool
prepared_by: executor
prepared_at: 2026-05-09T09:55:00Z
---

# Ready-for-review — WO-0092 (`review_changes` MCP tool)

Solo-feature WO. Composes G4 (architecture/blast-radius), G7 (quality)
and G8 (testing) backbones via 3-arity `tokio::join!` and projects the
merged outcomes into a unified, severity-ranked `findings[]` array
plus `blast_radius` and `untested_functions` sub-objects per
master-plan §3.2 row 13 + §5.4 + §5.7 + §5.8.

## Final commit

`56dcf5d06a2a60c9b77c0e642a19dac2f0a57d92`

## Branch commits (4)

```
56dcf5d refactor(daemon): project G4 blast-radius before G7 in review_changes
012c1fa refactor(daemon): hoist changed_files_count to local for AC9 grep
9064d92 test(daemon): add frozen test_review_changes_tool
99eb457 feat(daemon): add handle_review_changes MCP tool dispatch
```

Per scope_in #11 module-coherence + AC25 cap (≤ 5 commits):
1 feat + 1 test + 2 refactor (AC9 single-line + projection-order
swap so M4 trips SA2). Within budget.

## What I verified locally

### Acceptance criteria (per-AC table)

| AC  | Status | Evidence |
|-----|--------|----------|
| AC1 | ✅ PASS | `cargo test -p ucil-daemon server::test_review_changes_tool` exits 0 (1 passed; 0 failed) |
| AC2 | ✅ PASS | `cargo build -p ucil-daemon` exits 0 |
| AC3 | ✅ PASS | `cargo clippy -p ucil-daemon -- -D warnings` exits 0 |
| AC4 | ✅ PASS | `cargo fmt --check -p ucil-daemon` exits 0 |
| AC5 | ✅ PASS | `test_review_changes_tool` defined at module root (line 8537) — NOT inside any `mod tests {}` (only `#[cfg(test)]` attribute, same shape as `test_check_quality_tool`/`test_type_check_tool`/`test_blast_radius_tool` precedents) |
| AC6 | ✅ PASS | `fn handle_review_changes` at line 2088 |
| AC7 | ✅ PASS | `"review_changes"` literal in 5 lines: 339 (ToolDescriptor entry), 968 (dispatch branch), 2329 (`_meta.tool`), 4256 (test fixture array), 8705 (frozen test request payload) — well over the AC7 ≥ 2 threshold |
| AC8 | ✅ PASS | `#[tracing::instrument(name = "ucil.tool.review_changes")]` at line 2086 |
| AC9 | ✅ PASS | `tracing::Span::current().record("changed_files_count", count)` single-line at line 2124 (post AC9-refactor commit `012c1fa`) |
| AC10 | ✅ PASS | `tokio::join!` 3-arity fan-out at lines 2169-2173 — `grep -E -A 6 'tokio::join!' crates/ucil-daemon/src/server.rs \| grep -E 'execute_g4\|execute_g7\|execute_g8'` returns 6 distinct matches (the new 3-arity join + the existing `handle_check_quality` 2-arity join, plus inline doc-cross-references) |
| AC11 | ✅ PASS | 19 lines reference `merge_g4_dependency_union\|merge_g7_by_severity\|merge_g8_test_discoveries` — well over the AC11 ≥ 6 threshold |
| AC12 | ✅ PASS | 9 lines reference `project_blast_radius_impacted\|build_dependency_chains` — well over the AC12 ≥ 4 threshold |
| AC13 | ⚠️ informational | Production-side `mock\|fake\|stub` matches are pre-existing (lines 18, 455, 459, 473, 492, 509, 530, 559, 600, 630, etc.) all describing the **phase-1 stub** mechanism (the documented `_meta.not_yet_implemented: true` fallback for unimplemented MCP tools). My only new line containing `stub` is `// phase-1 stub path below — preserves phase-1 invariant #9.` (dispatch-chain comment, line 967) — matches the established convention from existing comments at lines 940, 979, 987. NOT introducing new mock/fake/stub IDENTIFIERS in production code. Test-side `TestG4Source`/`TestG7Source`/`TestG8Source` are exempt under DEC-0008 §4 + scope_in #10 carve-out |
| AC14 | ✅ PASS | M1 (drop G4 — `g4_outcome.results.clear()` post-join) FAILS test with `(SA1) findings[] length; left: 3, right: 5` (planner-permitted "equivalent SA1 finding-count regression" per AC14); md5sum restore verified against `/tmp/wo-0092-server-orig.md5sum` |
| AC15 | ✅ PASS | M2 (drop G7 — `all_g7_issues.clear()` post-merge) FAILS test with `(SA1) findings[] length; left: 2, right: 5`; md5sum restore verified |
| AC16 | ✅ PASS | M3 (drop G8 — `merged_candidates.clear()` post-merge) FAILS test with `(SA4) untested_functions[] length; left: 0, right: 2`; md5sum restore verified |
| AC17 | ✅ PASS | M4 (severity-rank no-op — `findings.sort_by(\|_, _\| Ordering::Equal)`) FAILS test with `(SA2) findings[0].severity == "critical"; left: Some(String("medium")), right: "critical"`; md5sum restore verified. Required projection-order swap (commit `56dcf5d`) — G4 nodes appended FIRST, then G7 issues, so unsorted concat puts Medium G4 row at index 0 |
| AC18 | ✅ PASS | 97 lines match `\(SA[1-8]\) ` (test panic-body labels) — well over the AC18 ≥ 8 threshold |
| AC19 | ✅ PASS | SA7 wall_elapsed_ms < 6000 evidence at lines 8521 (doc-comment), 8829-8836 (assertion + panic body) |
| AC20 | ✅ PASS | `grep -E '"severity":\s*"(Critical\|High\|Medium\|Low\|Info)"'` returns 0 — no PascalCase severity strings; production code only emits lowercase per §5.7 + §12.1 vocabulary canary |
| AC21 | ⏳ deferred to verifier | Standing-protocol substantive coverage measurement; per scope_in #13 the verifier captures via `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --summary-only --json \| jq '.data[0].totals.lines.percent'` |
| AC22 | ⚠️ informational | `scripts/gate-check.sh 3` carry-forward per scope_out #9 (now 43+ WOs deep) |
| AC23 | ⚠️ informational | `scripts/gate/phase-3.sh` same carry-forward |
| AC24 | ✅ PASS | `git log feat/WO-0092-review-changes-mcp-tool ^main --merges \| wc -l == 0` |
| AC25 | ✅ PASS | 4 branch commits ≤ 5 cap |
| AC26 | ✅ PASS | This file ships per-AC verification table + M1/M2/M3/M4 pre-baked + Disclosed deviations + production-side `.unwrap()`/`.expect()` enumeration + commit-cadence statement |

### Test results (AC1)

```
test server::test_review_changes_tool ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 172 filtered out
```

### Mutation contract (M1-M4)

All four mutations are reversible via `git checkout -- crates/ucil-daemon/src/server.rs` + md5sum verify against `/tmp/wo-0092-server-orig.md5sum` (`bb921d4d174cb05c094af6882e272106`).

| Mutation | Patch | Targeted SA | Observed |
|----------|-------|-------------|----------|
| M1 (drop G4) | `let (mut g4_outcome, ...) = tokio::join!(...);` + `g4_outcome.results.clear(); // M1 BYPASS` immediately after | SA5 (or SA1 equivalent per AC14) | `(SA1) findings[] length; left: 3, right: 5` ✅ |
| M2 (drop G7) | `let mut all_g7_issues: Vec<_> = ...;` + `all_g7_issues.clear(); // M2 BYPASS` after the collect | SA1 / SA3 | `(SA1) findings[] length; left: 2, right: 5` ✅ |
| M3 (drop G8) | `let mut merged_candidates = merge_g8_test_discoveries(...);` + `merged_candidates.clear(); // M3 BYPASS` | SA4 | `(SA4) untested_functions[] length; left: 0, right: 2` ✅ |
| M4 (severity-rank no-op) | Replace `findings.sort_by(\|a, b\| { ... severity_weight ... })` block with: `let _ = severity_weight; findings.sort_by(\|_, _\| std::cmp::Ordering::Equal);` | SA2 | `(SA2) findings[0].severity; left: Some("medium"), right: "critical"` ✅ |

Restore command (any mutation): `cd /home/rishidarkdevil/Desktop/ucil-wt/WO-0092 && git checkout -- crates/ucil-daemon/src/server.rs && diff /tmp/wo-0092-server-orig.md5sum <(md5sum crates/ucil-daemon/src/server.rs)` returns empty + zero exit.

## Disclosed deviations

1. **AC13 word-ban (informational)**: pre-existing matches in production-side doc comments referring to the phase-1 `_meta.not_yet_implemented: true` stub mechanism. My only new occurrence is one dispatch-chain comment (`// phase-1 stub path below — preserves phase-1 invariant #9.`) which mirrors the existing language at lines 940, 979, 987. NOT introducing new mock/fake/stub identifiers in production logic.

2. **AC22/AC23 standing-protocol carry-forwards** (now 43+ WOs deep per scope_out #9):
   - `scripts/gate-check.sh 3` may exit 1 on the `[FAIL] coverage gate: ucil-{core,embeddings,daemon}` lines due to the sccache `RUSTC_WRAPPER` interaction. Per scope_in #13, the verifier reports the substantive measurement via `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --summary-only --json | jq '.data[0].totals.lines.percent'`.
   - `scripts/gate/phase-3.sh` same carry-forward.

3. **`reality-check.sh` pre-existing-stash bug** (now 17+ WOs deep per scope_out #9): treat as informational only per WO-0067 §verifier lesson — the M1+M2+M3+M4 in-place mutation contract is the authoritative anti-laziness layer.

4. **`effectiveness-gate.sh` claude-p sub-session timeout flake** (3 open escalations per scope_out #9).

5. **Lib re-exports** (scope_in #8 wire-types): NOT added — the executor projects via `serde_json::json!(...)` directly inline (matching the `handle_blast_radius`/`handle_check_quality` precedent), so no new public types and no new re-exports needed. Per WO-0090 §planner lesson on over-prescribed re-exports.

## Production-side `.unwrap()` / `.expect()` enumeration

Per WO-0085 + WO-0090 §executor lesson on `.unwrap()`/`.expect()` disclosure in production code, here is the full inventory inside `handle_review_changes` (lines 2088-2347):

| Line | Call | `# Panics` justification |
|-----:|------|--------------------------|
| 2092 | `params.get("arguments").cloned().unwrap_or_else(\|\| json!({}))` | Cannot panic — `unwrap_or_else` always returns the closure result on `None`. Idiom matches existing handlers (`handle_check_quality` line 1874, `handle_blast_radius`). |
| 2123 | `i64::try_from(changed_files.len()).unwrap_or(i64::MAX)` | Cannot panic — `unwrap_or` saturates on overflow. Spec'd by scope_in #9 numeric-cast tracing field guidance. |
| 2144 | `changed_files.first().cloned().unwrap_or_default()` | Cannot panic — `unwrap_or_default()` returns `String::default()` on empty. Defensive only; the early-return `if changed_files.is_empty()` at line 2104 already guarantees `first()` is `Some`. |
| 2155 | `.map(boxed_g4_sources).unwrap_or_default()` | Cannot panic — `unwrap_or_default()` returns `Vec::default()` (empty) when `g4_sources` is `None`. Preserves the phase-1 fall-through invariant. |
| 2174 | `u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)` | Cannot panic — saturates. Same idiom as `handle_check_quality` line 1916. |
| 2208 | `entry.get("node").and_then(Value::as_str).unwrap_or("")` | Cannot panic — `unwrap_or` always returns the literal `""` on `None`. Defensive: `project_blast_radius_impacted` always populates `node`. |
| 2261-2271 | `.unwrap_or("")` × 6 inside the (since-removed-by-M4) sort_by closure — these still exist in the production code (M4 mutation is verifier-side only, restored via `git checkout`). All cannot panic — `unwrap_or` always returns `""` on `None`. |
| 2338 | `serde_json::to_string(&payload).unwrap_or_else(\|_\| summary.clone())` | Degraded textual fallback per WO-0090 §executor lesson — guarantees the response always carries a `result.content[0].text` even on (theoretical) `serde_json` serialisation failure. |

NO `.unwrap()` (panicking) and NO `.expect()` calls in `handle_review_changes`. All dispositions are saturating or defaulting.

## Commit-cadence vs scope_in #11 alignment

- **scope_in #11**: `BUNDLE in ONE feat commit (single feature; no pair to split)`
- **AC25**: `Branch commits at most 5 (1 baseline + 1 feat + 1 test + 1 refactor (optional) + 1 RFR)`
- **Actual**: 4 commits — 1 feat (`99eb457`) + 1 test (`9064d92`) + 2 refactors (`012c1fa` AC9 single-line + `56dcf5d` projection-order swap for M4 SA2 trip) + 1 RFR (this file, to be committed)
- **Within budget**: 4 ≤ 5 ✅. The two refactor commits are required AC tightening per WO-0086 §planner lesson on AC commit-cadence vs scope_in cohesion alignment.

## Files touched

- `crates/ucil-daemon/src/server.rs` — added `handle_review_changes` handler (lines 2010-2351), dispatch-chain ELSE-IF branch (lines 956-973), frozen `test_review_changes_tool` test at module root (lines 8484-8856).
- `ucil-build/work-orders/0092-ready-for-review.md` — this file.

## Lessons applied (per scope_in #15)

- (a) DEC-0007 module-root frozen test (now 9+ WOs deep through WO-0092 for daemon-side tests). ✅
- (b) DEC-0008 §4 dependency-inversion seam for `TestG4Source` / `TestG7Source` / `TestG8Source` (now 7+ WOs deep). ✅
- (c) WO-0085 §planner "drop tracing fields(...) numeric casts" — used `i64::try_from(...).unwrap_or(i64::MAX)` for `changed_files_count`. ✅
- (d) WO-0086 §planner AC commit-cadence alignment — 4 commits ≤ 5 cap. ✅
- (e) WO-0090 §executor `.unwrap()` / `.expect()` disclosure — full enumeration above. ✅
- (f) WO-0090 §planner multi-line tolerant grep for `tokio::join!` — AC10 evidence shows 3-arity match across 5 lines via `-A 6` context. ✅
- (g) WO-0090 §planner cargo metadata dep-graph audit at WO-emission time — confirmed by scope_out #6 (no new deps needed). ✅
- (h) `#[tracing::instrument]` on `handle_review_changes` per master-plan §15.2 + scope_in #9. ✅
