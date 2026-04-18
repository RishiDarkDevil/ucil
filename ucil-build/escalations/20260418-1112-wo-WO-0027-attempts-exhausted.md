---
timestamp: 2026-04-18T11:12:41Z
type: verifier-rejects-exhausted
work_order: WO-0027
verifier_attempts: 3
max_feature_attempts: 3
severity: high
blocks_loop: false
resolved: true
---

# WO-0027 hit verifier-reject cap

Verifier ran 3 times on WO-0027; at least one feature has
attempts=3.

Latest rejection: ucil-build/rejections/WO-0027.md
Latest root-cause: ucil-build/verification-reports/root-cause-WO-0027.md (if present)

If the rejection cites harness-script bugs (reality-check.sh,
flip-feature.sh, a hook, a launcher), triage may auto-resolve this
escalation via Bucket B before the loop halts.

## Resolution

Resolved 2026-04-18 — user-authorized direct fix landed on
`feat/WO-0027-watchman-detection-and-backend-selection` at **`036e9cf`**:
the process-wide PATH guard was promoted from watcher module-local
(`ENV_GUARD` in `crates/ucil-daemon/src/watcher.rs`) to a new crate-scoped
`test_support` module. session_manager tests that spawn git via
`tokio::process::Command::new("git")` now hold the same mutex, so the
watcher tests' blank-PATH window can no longer race with git subprocess
spawns under `cargo test`'s one-process-many-threads model.

**Verified locally**: `cargo test -p ucil-daemon --lib` → 59 passed,
0 failed (previously 5/5 failed via `cargo llvm-cov` which drives
`cargo test`).

The three prior rejections (592c908, de5039d, 42aba9d) predate the
fix and stand as historical markers on the feat branch. All 3 cited
the same PATH-mutation race root cause.

Triage passes 1-3 correctly applied the Bucket E rubric (attempts=3
with rejections); the PATH-race was too structural for Bucket-B
harness auto-resolve and exceeded Bucket-D's <60 lines scope, so the
human path was appropriate. User authorized direct fix after triage
halted.

Next: outer loop resume via `scripts/resume.sh --yes` will spawn a
fresh verifier session against feat branch tip `036e9cf`. The
verifier's in-memory `attempts` counter resets per `run-phase.sh`
invocation (DEC-0007 rubric), so the fix gets an unblocked first
attempt.

Bucket A — user-authorized remediation landed.
