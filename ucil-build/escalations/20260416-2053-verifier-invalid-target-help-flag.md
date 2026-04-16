---
severity: low
kind: harness-misuse
raised_by: verifier
raised_at: 2026-04-16T20:53:47Z
target_argument: "--help"
blocks_loop: false
---
# Verifier invoked with invalid target `--help`

## Summary

A verifier session was spawned with the target argument `--help`
(`scripts/spawn-verifier.sh --help`). `--help` is NOT a valid feature
id and NOT a work-order id; no matching entry exists in
`ucil-build/feature-list.json` (all ids follow the pattern
`P<phase>-W<week>-F<nn>`), and no file under `ucil-build/work-orders/`
references it.

The root cause is that `scripts/spawn-verifier.sh` does not implement
a `--help` flag — it treats its first positional argument as a target
id unconditionally (see lines 15–19). So a caller typing `--help`
expecting CLI help instead launches a verifier against a non-existent
feature.

## What this verifier did

- Confirmed no feature with id `--help` (or substring `help`) exists in
  `ucil-build/feature-list.json`.
- Confirmed no work-order file named `*help*` exists in
  `ucil-build/work-orders/`.
- Did NOT run `cargo clean`, tests, mutation check, or any quality
  gate — there is no code change to verify, no branch to check out
  (current branch is `main`, and the dashboard shows 22 pre-existing
  uncommitted changes unrelated to any target).
- Did NOT mutate `ucil-build/feature-list.json`. Did NOT call
  `scripts/flip-feature.sh`.
- Did NOT edit any source code.

## Verdict

**REJECT — invalid target.** No `passes` flip performed. No rejection
file under `ucil-build/rejections/` written because a rejection
requires a real WO-ID, and there is none.

## Repro

```
$ scripts/spawn-verifier.sh --help
[spawn-verifier] new session: <uuid>
[spawn-verifier] target: --help
# verifier session then finds no feature / no work-order matching "--help"
```

## Suggested fix (not applied — verifier does not edit source)

Add a guard at the top of `scripts/spawn-verifier.sh` that, when the
first positional arg is `-h` / `--help`, prints the usage block and
exits 0 without spawning a verifier. Rough sketch for the maintainer:

```bash
case "${1:-}" in
  -h|--help)
    cat <<'USAGE'
Usage: scripts/spawn-verifier.sh <work-order-id|feature-id> [claude-cli args...]
  Spawns a FRESH Claude Code session as the UCIL verifier.
USAGE
    exit 0 ;;
esac
```

A secondary hardening would be to validate the target against
`feature-list.json` / `ucil-build/work-orders/` before consuming an
LLM turn, and exit non-zero with a clear message if neither resolves.

## Next step

- Triage: classify as harness-misuse, low severity. Not a source-code
  bug for UCIL proper; a usability bug in the spawn script.
- Planner/executor: optionally open a trivial WO to add the `--help`
  guard shown above. No feature in `feature-list.json` is affected.
