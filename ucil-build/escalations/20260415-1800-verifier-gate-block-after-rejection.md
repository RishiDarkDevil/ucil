---
blocks_loop: false
severity: harness-config
requires_planner_action: false
resolved: false
---

# Escalation: Stop hook blocking verifier session after legitimate rejection

**Filed by**: verifier session `vrf-4be5569c-f647-441d-ae7c-881c990ae00c`
**Date**: 2026-04-15T18:00:00Z
**Work-order**: WO-0003

## What happened

The verifier completed its work for WO-0003 and pushed rejection commit `a3d0754` to
`feat/0003-init-fixtures`. The rejection is correct and complete:
- `ucil-build/rejections/WO-0003.md` written and pushed
- `ucil-build/feature-list.json` updated — all five features have `attempts=1`
- Rejection commit pushed to remote

The stop hook then fired and refused to end the session because:

```
Phase 0 gate failed.
[FAIL] Unfinished features in phase 0:
P0-W1-F03,P0-W1-F04,P0-W1-F05,P0-W1-F06,P0-W1-F08,P0-W1-F11,P0-W1-F12,P0-W1-F13,P0-W1-F14
```

## Why this is a harness-config issue

The stop hook (`scripts/stop/gate.sh` or similar) runs `scripts/gate-check.sh $PHASE` and
blocks any session from ending while the gate is red. However, **a verifier session that has
just written a rejection is the correct state**: the gate is red BECAUSE the executor's work
was incomplete. Blocking the verifier from ending does not help — the verifier cannot write
source code to make the gate green, and should not be expected to.

The stop hook needs a verifier-session exemption. Specifically:

> A session where `CLAUDE_SUBAGENT_NAME=verifier` AND `ucil-build/rejections/WO-NNNN.md`
> exists for the work-order being verified SHOULD be allowed to end even if the gate is red.

This is analogous to the existing exemption pattern seen in escalation files
(`blocks_loop: false` means the loop resumes). The verifier session should resume the loop
normally; the loop will hand off back to the executor for the re-work.

## Concrete fix (Bucket B eligible)

In `scripts/stop/gate.sh` (or whichever hook enforces the gate), add an early-exit
condition:

```bash
# Verifier sessions that have written a rejection are done — do not block them.
if [[ "${CLAUDE_SUBAGENT_NAME:-}" == "verifier" ]]; then
  # Check if a rejection file exists for any work-order
  if ls ucil-build/rejections/WO-*.md >/dev/null 2>&1; then
    echo "[gate] Verifier session with rejection on file — allowing exit."
    exit 0
  fi
fi
```

Alternatively, if the stop hook checks a specific work-order context variable, condition on
that instead.

## Immediate workaround

This escalation file itself is committed and pushed, which the stop hook should detect as
a valid escalation (allowing the session to end). If the hook does not respect this pattern,
triage should auto-resolve (Bucket A — benign admin escalation whose condition is already
documented).

## Summary of WO-0003 rejection (for triage / next executor)

The executor must fix before re-submitting:

1. **Commit the python-project fixture** — `tests/fixtures/python-project/` and
   `tests/fixtures/python_project/test_fixture_valid.py` were left uncommitted.
2. **Implement the typescript-project fixture** — `tests/fixtures/typescript-project/`
   and `adapters/tests/fixtures/typescript-project.test.ts` are absent.
3. **Implement the mixed-project fixture and script** — `tests/fixtures/mixed-project/`
   is absent; `scripts/verify/P0-W1-F14.sh` is still the TODO stub.
4. **Fix `tempfile` dev-dependency** — `cargo test --workspace` exits 101 because
   `tempfile` is used in `ucil-cli` test code but not in `[dev-dependencies]`.
5. **Resolve Criterion 2 vacuous-pass** — `rust_project_loads` has no `#[ignore]`, so
   the acceptance criterion `--ignored` runs 0 tests. File an ADR and update criterion.
6. **Write `0003-ready-for-review.md`** before triggering verifier again.

All five WO-0003 features remain `passes: false`, `attempts: 1`.
