# UCIL Triage Log

Append-only. One line per Bucket-E halt. Format: `YYYY-MM-DDTHH:MMZ <slug> HALT — <reason>`.

---

2026-04-16T00:00Z wo-WO-0006-attempts-exhausted HALT — P1-W2-F06 hit attempts=3 (3 identical verifier rejections: test inside `mod tests {}` makes nextest path `storage::tests::test_two_tier_layout` but frozen selector is `storage::test_two_tier_layout`); executor must remove `mod tests {}` wrapper from `crates/ucil-daemon/src/storage.rs` and request retry-4 after user resets the attempts cap or grants an override.
