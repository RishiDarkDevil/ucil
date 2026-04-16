# UCIL Triage Log

Append-only. One line per Bucket-E halt. Format: `YYYY-MM-DDTHH:MMZ <slug> HALT — <reason>`.

---

2026-04-16T00:00Z wo-WO-0006-attempts-exhausted HALT — P1-W2-F06 hit attempts=3 (3 identical verifier rejections: test inside `mod tests {}` makes nextest path `storage::tests::test_two_tier_layout` but frozen selector is `storage::test_two_tier_layout`); executor must remove `mod tests {}` wrapper from `crates/ucil-daemon/src/storage.rs` and request retry-4 after user resets the attempts cap or grants an override.
2026-04-16T05:30Z wo-WO-0006-attempts-exhausted HALT (cap-rescue pass 2) — same issue; P1-W2-F06 rejected 3× on feat/WO-0006 branch; fix known (~5 lines remove mod tests wrapper) but never applied by executor; user action required: reset attempts cap or manually fix and re-run verifier.

## 2026-04-16T20:22:58Z resume auto-stash

- ../ucil-wt/WO-TEST-RESUME-1776370978/ :: auto-stash-on-resume-20260416T202258Z

Inspect with: `git -C <wt> stash list` — pop or drop per executor's judgement.

## 2026-04-16T20:22:58Z resume corrupt-JSON quarantine

Moved 1 work-order(s) to `ucil-build/work-orders/broken-20260416T202258Z/`:
- broken-test-resume-1776370978.json
