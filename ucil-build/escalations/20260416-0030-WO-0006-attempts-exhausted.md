---
blocks_loop: true
severity: high
requires_planner_action: false
resolved: false
---
# Escalation: P1-W2-F06 failed verifier 3 times — loop must halt

**Filed by**: verifier-ad6e4e9e-ceb8-4ae9-abc3-b1c7ad4c27bd
**Filed at**: 2026-04-16T00:30:00Z
**Work-order**: WO-0006
**Feature**: P1-W2-F06 (two-tier `.ucil/` storage layout)

---

## Trigger

CLAUDE.md escalation rule §1: "Same feature fails verifier 3 times."

`P1-W2-F06` has `attempts=3` after this session's rejection (retry 3).
All three rejections cite the identical root cause.

---

## Root cause

`crates/ucil-daemon/src/storage.rs` wraps the test function inside
`#[cfg(test)] mod tests { ... }`, giving it the nextest path
`storage::tests::test_two_tier_layout`.

The **frozen** acceptance test selector in `feature-list.json` is:
```
-p ucil-daemon storage::test_two_tier_layout
```
(no `tests::` component). This selector matches **zero tests** — nextest exits 4.

This has been described in detail in every rejection report since retry 1.

---

## Why it was not fixed

The executor was not re-spawned after retry-1 or retry-2 rejections.
Prior escalations (`20260415-0810-WO-0003-retry3-harness-blocks-flip.md`,
`20260415-1800-WO-0003-rejection-gate-block.md`, etc.) may have blocked the
outer loop before a fix could be attempted.

---

## Fix (trivial, ~5 lines)

In `crates/ucil-daemon/src/storage.rs`, change:

```rust
// BEFORE
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_two_tier_layout() {
        // ... body ...
    }
}
```

to:

```rust
// AFTER
#[cfg(test)]
use tempfile::TempDir;

#[test]
fn test_two_tier_layout() {
    // ... body unchanged ...
}
```

After the fix, confirm:
```
cargo nextest run -p ucil-daemon storage::test_two_tier_layout
# Must exit 0 with 1 test passing
```

---

## Required action

1. **Spawn executor** (Bucket D — micro-fix, ~5 lines, single file):
   - Remove `mod tests { ... }` wrapper from `crates/ucil-daemon/src/storage.rs`
   - Run `cargo nextest run -p ucil-daemon storage::test_two_tier_layout` → confirm exit 0
   - Commit and push to `feat/WO-0006-symbol-extraction-chunker-storage`
2. **Spawn fresh verifier** (retry 4) after executor confirms fix.
3. Note: P1-W2-F02 and P1-W2-F03 tests are fully green and ready to pass as soon as P1-W2-F06 is resolved.

---

## Status of other WO-0006 features

| Feature | Status |
|---------|--------|
| P1-W2-F02 (symbol extraction) | All 6 tests PASS — waiting on P1-W2-F06 unblock |
| P1-W2-F03 (AST-aware chunker) | All 4 tests PASS — waiting on P1-W2-F06 unblock |
| P1-W2-F06 (storage layout) | BLOCKED — attempts=3, escalation triggered |
