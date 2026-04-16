---
timestamp: 2026-04-16T21:46:37Z
type: verifier-rejects-exhausted
work_order: WO-0008
verifier_attempts: 3
max_feature_attempts: 0
severity: high
blocks_loop: true
---

# WO-0008 hit verifier-reject cap

Verifier ran 3 times on WO-0008; at least one feature has
attempts=0.

Latest rejection: ucil-build/rejections/WO-0008.md
Latest root-cause: ucil-build/verification-reports/root-cause-WO-0008.md (if present)

If the rejection cites harness-script bugs (reality-check.sh,
flip-feature.sh, a hook, a launcher), triage may auto-resolve this
escalation via Bucket B before the loop halts.
