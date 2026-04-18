---
timestamp: 2026-04-18T09:02:44Z
type: verifier-rejects-exhausted
work_order: WO-0024
verifier_attempts: 4
max_feature_attempts: 0
severity: high
blocks_loop: true
---

# WO-0024 hit verifier-reject cap

Verifier ran 4 times on WO-0024; at least one feature has
attempts=0.

Latest rejection: ucil-build/rejections/WO-0024.md
Latest root-cause: ucil-build/verification-reports/root-cause-WO-0024.md (if present)

If the rejection cites harness-script bugs (reality-check.sh,
flip-feature.sh, a hook, a launcher), triage may auto-resolve this
escalation via Bucket B before the loop halts.
