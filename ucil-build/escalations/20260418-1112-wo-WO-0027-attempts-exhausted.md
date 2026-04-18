---
timestamp: 2026-04-18T11:12:41Z
type: verifier-rejects-exhausted
work_order: WO-0027
verifier_attempts: 3
max_feature_attempts: 3
severity: high
blocks_loop: true
---

# WO-0027 hit verifier-reject cap

Verifier ran 3 times on WO-0027; at least one feature has
attempts=3.

Latest rejection: ucil-build/rejections/WO-0027.md
Latest root-cause: ucil-build/verification-reports/root-cause-WO-0027.md (if present)

If the rejection cites harness-script bugs (reality-check.sh,
flip-feature.sh, a hook, a launcher), triage may auto-resolve this
escalation via Bucket B before the loop halts.
