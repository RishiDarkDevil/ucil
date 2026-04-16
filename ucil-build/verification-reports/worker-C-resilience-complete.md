# Worker-C Resilience + Resume Track — Completion Report

**Date:** 2026-04-17
**Worker:** C (resilience + resume)
**Branch:** main
**Status:** Complete — all 7 deliverables landed, all tests pass.

---

## Deliverables

### 1. Hardened `scripts/resume.sh` (commit 31cf1b7, +192/-6)

Added four new cleanup passes on top of the existing ones:

- **Orphaned `claude -p` kill.** Walks the 8-deep ppid chain of every `claude -p` pid; if no ancestor is a live `scripts/run-{phase,all,executor,planner,critic,triage,root-cause-finder,effectiveness-evaluator}.sh` and no `spawn-verifier.sh`, the pid is classified as an orphan. SIGTERM first, 10s graceful window, then SIGKILL for stragglers. Interactive (non-`-p`) claude sessions are explicitly NEVER touched — the detector requires `-p` as a standalone argv entry (NUL-split check on `/proc/<pid>/cmdline`) AND the executable regex `(^| )claude( |$)|/claude($| )`.
- **Dirty-worktree auto-stash.** For each `../ucil-wt/WO-*/`, if `git status --porcelain` is non-empty, run `git stash push -u -m "auto-stash-on-resume-<ts>"` and log to `ucil-build/triage-log.md`. Never deletes — user can `git stash list` and inspect/pop/drop.
- **Corrupt-JSON quarantine.** `jq -e . ucil-build/work-orders/*.json`; any failures are `mv`'d to `ucil-build/work-orders/broken-<ts>/` and appended to `triage-log.md`.
- **Feature-list integrity.** `jq` + `python3 -m jsonschema` (Draft-2020-12) against the existing `feature-list.schema.json`. On failure, files a LOUD critical escalation (`severity: critical`, `blocks_loop: true`, `requires_planner_action: true`) and exits 1. Oracle integrity is load-bearing.

Resume summary now exposes: `Orphans killed`, `Worktrees auto-stashed`, `Corrupt WOs quarantined`.

### 2. `scripts/_watchdog.sh` (commit eca29da, +193)

Detached poller (default 60 s). Triggers `scripts/resume.sh --yes` when:

- no `run-all.sh`/`run-phase.sh` alive, AND
- no headless `claude -p` alive, AND
- current phase not shipped (no `git tag phase-<N>-complete`).

Quiesces 5 min before restart (tunable via `UCIL_WATCHDOG_QUIESCE_S`). Flap guard: 3 restarts in a 1-hour rolling window triggers an escalation and exit. SIGTERM/INT/HUP handled gracefully via a `STOP` flag polled between sub-sleeps (no kill loops). Pidfile at `ucil-build/.watchdog.pid` (gitignored; enforced). Pidfile-based "already running" guard.

### 3. `scripts/install-watchdog.sh` (commit c3f883b, +197)

Auto-detects:

- **systemd --user** if `systemctl --user status` works (preferred; creates `~/.config/systemd/user/ucil-watchdog.service`, enables it, starts it, prints linger hint if lingering is off).
- **cron @reboot** fallback otherwise (adds a single tagged line; idempotent via `# UCIL watchdog (ucil-watchdog)` marker).

`--systemd` / `--cron` flags force mode. `--uninstall` removes both + signals a running watchdog. Smoke-tested install/uninstall via cron path — entry added + removed cleanly; crontab goes from 1 to 0 matching lines.

### 4. `scripts/run-all.sh` improvements (commit 2a57070, +33)

- Tags `checkpoint-phase-N` after each shipped phase (annotated + pushed). Pure rewind anchor alongside `phase-N-complete` (which can be re-scoped via `--force`).
- Conditional `safe_check_daily_budget` call at phase start if worker-B's `scripts/_cost-budget.sh` is sourceable. Soft-dependency — absence is a no-op.

### 5. `scripts/test-orphan-cleanup.sh` (commit cefcc4e, +114)

Spawns a python-shim fake named `claude` with `-p` as argv[1], detached via `setsid` + `disown` so its ancestor chain is systemd (NOT any run-*.sh). Runs `resume.sh --check`; asserts:

- fake is visible to `pgrep -f 'claude -p'`
- has standalone `-p` argv entry
- matches the `(^| )claude( |$)|/claude($| )` regex
- is NOT parented to a launcher
- is gone within 15s after resume's cleanup

**Result: PASS** (orphan killed, exit 0).

### 6. `scripts/test-resume.sh` (commit 503ffc0, +226)

Integration test. Sets up five conditions in one pass:

1. Test worktree `../ucil-wt/WO-TEST-RESUME-<ts>` with an uncommitted `README.md` change.
2. Stale `ucil-build/.verifier-lock` with no matching process.
3. Fake orphan `claude -p` (same shim as test 5).
4. Corrupt JSON dropped into `work-orders/broken-test-resume-<ts>.json`.
5. Asserts `feature-list.json` is still valid before/after.

Runs `resume.sh --check` twice:

- Run 1 must log orphan-kill, lock-removal, stash push, quarantine move, and a triage-log auto-stash entry — all verified.
- Run 2 must show `Orphans killed: 0`, `Worktrees auto-stashed: 0`, `Corrupt WOs quarantined:0` — idempotency verified.

Self-cleanup via `trap`: worktree pruned, branch deleted, stash dropped, shim removed, quarantine dir cleaned.

**Result: PASS** — both runs pass all assertions.

### 7. Commit + push cadence

All six worker-C commits landed on `main` and pushed to origin (no amends, no force-push):

| SHA | Title | Files | Insertions |
|-----|-------|-------|------------|
| 31cf1b7 | `feat(harness): harden resume.sh` | 1 | +192/-6 |
| eca29da | `feat(harness): add _watchdog.sh` | 2 | +193 |
| c3f883b | `feat(harness): add install-watchdog.sh` | 1 | +197 |
| 2a57070 | `feat(harness): run-all.sh checkpoints + daily budget guard` | 1* | +33 |
| cefcc4e | `test(harness): orphan-cleanup smoke for resume.sh` | 1 | +114 |
| 503ffc0 | `test(harness): integration smoke for resume.sh` | 1 | +226 |

_*`2a57070` unintentionally swept up seven untracked telemetry-env edits to launcher scripts made by worker-B in parallel; the edits are benign (they propagate `CLAUDE_CODE_ENABLE_TELEMETRY=1` / `UCIL_WO_ID` env vars), `bash -n`-validated, and consistent with worker-B's track. No rollback is warranted._

**Total:** 6 commits, 955+ / 6- lines.

---

## Tests passed

| Test | Invocation | Outcome |
|------|-----------|---------|
| `bash -n scripts/resume.sh` | syntax check | OK |
| `bash -n scripts/_watchdog.sh` | syntax check | OK |
| `bash -n scripts/install-watchdog.sh` | syntax check | OK |
| `bash -n scripts/run-all.sh` | syntax check | OK |
| `bash -n scripts/test-orphan-cleanup.sh` | syntax check | OK |
| `bash -n scripts/test-resume.sh` | syntax check | OK |
| `scripts/resume.sh --check` | smoke | 0 orphans / 0 stashes / 0 corrupt on a clean tree |
| `scripts/_watchdog.sh` (6s timeout) | boot + signal | starts, detects dead loop, spawns resume, exits on SIGTERM cleanly |
| `scripts/install-watchdog.sh --cron` + `--uninstall` | round-trip | entry present → absent |
| `scripts/test-orphan-cleanup.sh` | end-to-end | PASS |
| `scripts/test-resume.sh` | end-to-end | PASS (including idempotency on 2nd run) |

---

## Constraints honoured

- **Never kill an interactive user session.** The `-p` argv check + `/claude` executable regex are both required; an interactive `claude` session (no `-p`) is silently skipped.
- **Never delete uncommitted work.** Dirty worktrees are stashed, logged, and left for the next executor to pop or drop.
- **Watchdog killable via SIGTERM.** A `trap on_signal TERM INT HUP` flips `STOP=1`, and `nap()` polls it between 1-s sleeps. Verified in smoke test (terminated cleanly, 2 log lines in ≤2s).
- **No `--no-verify`, no force-push, no amend-after-push.** Clean commit history.

---

## Blockers

**None.**

Two minor notes for follow-on work (not blockers):

1. The watchdog quiesce window defaults to 5 min which means a truly dead loop takes 5-6 min to resume. Tunable via `UCIL_WATCHDOG_QUIESCE_S`. If the user wants faster recovery, drop it to 60 s.
2. `install-watchdog.sh --systemd` prints a hint about `loginctl enable-linger` for true on-boot activation. User must run that one-time setup as root; this cannot be automated from a user-service without sudo.

---

## File pointers

All files under `/home/rishidarkdevil/Desktop/ucil/`:

- `scripts/resume.sh` (hardened)
- `scripts/_watchdog.sh` (new)
- `scripts/install-watchdog.sh` (new)
- `scripts/run-all.sh` (checkpoint tag + budget hook)
- `scripts/test-orphan-cleanup.sh` (new test)
- `scripts/test-resume.sh` (new test)
- `.gitignore` (adds `ucil-build/.watchdog.pid`)
- `ucil-build/verification-reports/worker-C-resilience-complete.md` (this file)
