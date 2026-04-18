---
timestamp: 2026-04-18T08:48:02Z
type: verifier-rejects-exhausted
work_order: WO-0024
verifier_attempts: 3
max_feature_attempts: 0
severity: high
blocks_loop: true
resolved: true
---

# WO-0024 hit verifier-reject cap

Verifier ran 3 times on WO-0024; at least one feature has
attempts=0.

Latest rejection: ucil-build/rejections/WO-0024.md
Latest root-cause: ucil-build/verification-reports/root-cause-WO-0024.md (if present)

If the rejection cites harness-script bugs (reality-check.sh,
flip-feature.sh, a hook, a launcher), triage may auto-resolve this
escalation via Bucket B before the loop halts.

---

## Resolution

Resolved 2026-04-18 by triage cap-rescue pass — companion to
`20260418-0820-pre-existing-incremental-rustdoc-bug.md` which carries the
technical details. The rejection does NOT cite harness-script bugs; it
cites a pre-existing UCIL-source bug in
`crates/ucil-core/src/incremental.rs` (two ambiguous intra-doc links
introduced silently by WO-0009 commit `5c2739a`). Both affected features
P1-W4-F02 and P1-W4-F08 carry `attempts: 0` in feature-list.json (verifier
protocol forbids flipping on reject), so the Bucket-D branch applies.

Remediation path: the companion 0820 escalation was converted to
**WO-0025 (fix-incremental-rustdoc-ambiguity)** in this same triage pass
(commits `347f1df` emitting the WO + `52ec529` resolving 0820). The
orchestrator's next iteration will process WO-0025, land the 4-character
fix, and then re-verify WO-0024 with a fresh verifier session (the
verifier_attempts counter is orchestrator-in-memory per DEC-0007 / triage
rubric). Criterion 5 (`cargo doc`) will go green; the other 17 checks
already pass per the three prior rejection runs.

Bucket A — companion escalation, remediation path established via WO-0025.

**Update 2026-04-18:** WO-0025 fix landed directly on main at commit
**`f6ec86e`** (user applied the 4-char intra-doc-link disambiguation
manually after the orchestrator kept re-verifying WO-0024 instead of
scheduling WO-0025 first). `cargo doc -p ucil-core --no-deps` verified
green locally. WO-0024 can re-verify cleanly on next loop iter.
