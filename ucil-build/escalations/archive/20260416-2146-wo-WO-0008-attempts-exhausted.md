---
timestamp: 2026-04-16T21:46:37Z
type: verifier-rejects-exhausted
work_order: WO-0008
verifier_attempts: 3
max_feature_attempts: 0
severity: high
blocks_loop: true
resolved: true
resolved_by: user-decision-DEC-0007
resolved_at: 2026-04-17T03:50:00Z
---

# WO-0008 hit verifier-reject cap

Verifier ran 3 times on WO-0008; at least one feature has
attempts=0.

Latest rejection: ucil-build/rejections/WO-0008.md
Latest root-cause: ucil-build/verification-reports/root-cause-WO-0008.md (if present)

If the rejection cites harness-script bugs (reality-check.sh,
flip-feature.sh, a hook, a launcher), triage may auto-resolve this
escalation via Bucket B before the loop halts.

## Resolution (2026-04-17T03:50Z — user decision via DEC-0007)

**Root cause per triage-log.md**: `cargo-mutants` scored 41% < 70% floor
on `ucil-daemon` with 7 surviving mutants on `session_manager.rs`
(lines 189/346/347 — mostly boundary-condition variants on TTL math).
**Functional tests were green, coverage gate passed, reality-check
passed**. Only the newly-added (by Worker A) cargo-mutants gate failed.

(An earlier note in this file about 401 auth failure was my wake-9
misdiagnosis — retracted. 401 errors during executor/critic retries
were transient; they weren't the halt cause.)

**DEC-0007** (committed this session) removes `scripts/verify/mutation-gate.sh`
from per-WO verifier gating. It remains in the repo as a **Phase 8
release-one-shot** at a relaxed 50% floor. Rationale: the master plan's
"mutation check" refers to `scripts/reality-check.sh` (stash-based),
which is still in force. Adding cargo-mutants per-WO was a Worker-A
inference, not in the spec. Cycle-time cost (~25 min/WO) and token cost
outweighed the marginal anti-laziness signal, given reality-check +
critic stub-scan + coverage gate already cover the same ground.

**Effect**: after this escalation is marked resolved and the watchdog's
next resume.sh cycle restarts run-phase.sh, the verifier re-runs on
WO-0008's branch without the cargo-mutants gate blocking. Expected
outcome: verifier green-flips P1-W3-F01 + P1-W4-F07 → merge → Phase 1
progresses.

**Attempts counter reset**: not needed. `max_feature_attempts=0` in the
frontmatter confirms no feature's `attempts` counter was ever
incremented (all rejections died before `flip-feature.sh ... reject`
could run). `feature-list.json` integrity is intact.

resolved: true
