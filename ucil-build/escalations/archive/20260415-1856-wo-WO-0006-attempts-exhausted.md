---
timestamp: 2026-04-15T18:56:22Z
type: verifier-rejects-exhausted
work_order: WO-0006
verifier_attempts: 3
max_feature_attempts: 0
severity: high
blocks_loop: true
resolved: true
---

# WO-0006 hit verifier-reject cap

Verifier ran 3 times on WO-0006; at least one feature has
attempts=0.

Latest rejection: ucil-build/rejections/WO-0006.md
Latest root-cause: ucil-build/verification-reports/root-cause-WO-0006.md (if present)

If the rejection cites harness-script bugs (reality-check.sh,
flip-feature.sh, a hook, a launcher), triage may auto-resolve this
escalation via Bucket B before the loop halts.

## Resolution (2026-04-17)

**This escalation has TWO distinct root causes** — both must be addressed
before the verifier can green-flip P1-W2-F06.

### Root cause 1 — critic-blocked on commit size (addressed)

Critic report `ucil-build/critic-reports/WO-0006.md` rejected on three
module-introduction commits exceeding the 200-line soft ceiling
(`711fb2c` +605, `3cedd46` +293, `aa8c4aa` +234). Code was CLEAN on all
other dimensions (no stubs, no mocked critical deps, no skipped tests,
full rustdoc, real tree-sitter + real filesystem test coverage).

**DEC-0005-WO-0006-module-coherence-commits.md** extends the DEC-0001
precedent (WO-0002 types.rs+schema_migration.rs) to cover symbols.rs,
chunker.rs, and storage.rs — each a single new module file containing
coherent type + impl + `#[cfg(test)] mod tests` where splitting would
produce non-compiling or dead-code intermediate commits.

### Root cause 2 — verifier-blocked on test-selector mismatch (NEW micro-WO required)

Verifier rejected 3× (commits `0c604df`, `ab943e5`, `a8ab537`) with the
same issue: feature `P1-W2-F06` has acceptance selector
`storage::test_two_tier_layout`, but the test function in
`crates/ucil-daemon/src/storage.rs` is wrapped in `#[cfg(test)] mod
tests { ... }`, making the actual nextest path
`storage::tests::test_two_tier_layout` — no match on the frozen selector.

The feature-list selector is immutable (oracle hierarchy level 2). The fix
must live in UCIL source: flatten the test into the module by removing the
`mod tests { ... }` wrapper and decorating the test function directly with
`#[cfg(test)] #[test]`.

Triage will synthesize **WO-0007** (micro-WO, Bucket D) on the next outer-loop
iteration with scope:
- `crates/ucil-daemon/src/storage.rs` only
- Remove `#[cfg(test)] mod tests { ... }` wrapper
- Each `#[test]` function becomes module-level with `#[cfg(test)] #[test]`
  pair (or `mod _test { ... }` with flat inner visibility — either works
  with the frozen selector as long as the selector path matches)
- `acceptance_criteria`: `cargo nextest run -p ucil-daemon storage::test_two_tier_layout`
- `feature_ids`: [] (micro-WO — verifier flips nothing directly; after
  merge, a separate verifier pass on the WO-0006 branch will flip
  P1-W2-F06 because its acceptance test now resolves).

After WO-0007 merges: orchestrator re-runs verifier on WO-0006's branch,
P1-W2-F06's selector now matches, verifier flips `passes=true`, merge-wo.sh
merges WO-0006 into main. Both WOs close.

resolved: true
