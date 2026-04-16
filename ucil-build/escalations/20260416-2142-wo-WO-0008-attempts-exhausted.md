---
timestamp: 2026-04-16T21:42:53Z
type: verifier-rejects-exhausted
work_order: WO-0008
features:
  - P1-W3-F01
  - P1-W4-F07
verifier_attempts: 3
max_feature_attempts: 3
severity: high
blocks_loop: true
requires_planner_action: true
resolved: false
---

# WO-0008 hit verifier-reject cap (3× reject)

The verifier has rejected WO-0008 three times in a row. Both
`P1-W3-F01` (process lifecycle) and `P1-W4-F07` (session state
tracking) now have `attempts == 3`, matching the harness contract's
unconditional halt condition ("Same feature fails verifier 3 times",
root `CLAUDE.md` → Escalation triggers #1).

## Rejection history

| Retry | Reject commit | Mutation score | Delta |
|-------|---------------|----------------|-------|
| 1 | `b512bfb` (2026-04-17T03:03Z) | 41% | initial reject |
| 2 | `17a1d39` (2026-04-17T03:09Z) | 41% | no code change since retry 1 |
| 3 | (this retry, 2026-04-16T21:42Z) | 41% | no code change since retry 2 |

All six **functional** acceptance criteria have passed on every retry:

- `cargo nextest run -p ucil-daemon lifecycle::` 7/7
- `cargo nextest run -p ucil-daemon session_manager::test_session_state_tracking` 1/1
- `cargo nextest run -p ucil-daemon session_manager::` 8/8 (no regression on P1-W2-F05)
- `cargo clippy -p ucil-daemon -- -D warnings` clean
- `cargo build --workspace` clean
- Module-level placement (line 422 < 491)

The **quality-gate** blocker is identical in all three retries: the
`ucil-daemon` crate mutation score is 41% (floor 70%).

## Why retries 2 and 3 keep reproducing the same outcome

No executor commits have landed on this branch since the initial
ready-for-review marker `50f053d`. The last three commits are verifier
REJECT commits (`b512bfb`, `17a1d39`, pending for this retry). The
re-verification runs full `cargo clean` from scratch and comes back
with the same result every time because the test suite has not
changed.

## Prescribed fix (already named in retry-1 and retry-2 rejections)

No source code change is needed. Three additional test-assertions
inside (or beside) `test_session_state_tracking` will kill the five
P1-W4-F07 mutants and push the score above 70%:

1. **Default-TTL branch** — create a session, skip `set_ttl`, call
   `purge_expired(created_at + DEFAULT_TTL_SECS + 1)`, assert removal.
   Kills mutants `session_manager.rs:189:36` (`+ → -`, `+ → *`).
2. **Boundary of `>`** — `set_ttl(10)`, call `purge_expired(created_at
   + 10)` (== `expires_at`) and confirm no purge; call
   `purge_expired(created_at + 11)` and confirm purge. Kills mutants
   `session_manager.rs:346:48` (`> → ==`, `> → >=`).
3. **Multi-session count** — create three sessions, set TTLs so two
   expire and one does not; confirm `purge_expired` returns 2 and the
   survivor remains. Kills mutant `session_manager.rs:347:30` (`- →
   +`).

Two additional pre-existing mutants (`main.rs:10:5`,
`session_manager.rs:225:19`) share the crate-level gate but are out of
this WO's direct scope; they should be tackled by a follow-up micro-WO
on ucil-daemon test coverage.

## Next-step options

- **Preferred**: spawn a root-cause-finder to confirm the above fix is
  correct and all-test, then re-dispatch the executor with a scoped
  test-only work-order. The executor should not touch `lifecycle.rs`
  or `session_manager.rs` production code.
- **Alternative**: the user may manually apply the three-item test
  plan and push to `feat/WO-0008-daemon-lifecycle-session-state`, then
  request a fresh verifier run via `scripts/spawn-verifier.sh WO-0008`.

## Pointers

- Latest rejection: `ucil-build/rejections/WO-0008.md` (this retry)
- Latest verification report: `ucil-build/verification-reports/WO-0008.md`
- Mutation detail: `ucil-build/verification-reports/mutation-ucil-daemon.md`
- Critic report (CLEAN with DEC-0005 warning):
  `ucil-build/critic-reports/WO-0008.md`
- Work-order: `ucil-build/work-orders/0008-daemon-lifecycle-session-state.json`
