# Rejection: WO-0003 (retry-2)

**Verifier session**: vrf-c0abe489-dba2-44ec-bdeb-396c6daf02c7
**Branch**: feat/0003-init-fixtures
**Rejected at**: 2026-04-15T18:30:00Z
**HEAD at verification**: d5133304d2e22aae1091e38647c79e7660e42fc9

## Work-order features

P0-W1-F03, P0-W1-F11, P0-W1-F12, P0-W1-F13, P0-W1-F14

## Summary

This is the **second rejection** of WO-0003.

Progress since retry-1:
- F03 (`ucil init`): acceptance test and mutation check both pass. ✅
- F11 (rust-project fixture): acceptance test passes (criterion 2 now runs 1 real test, not 0). Mutation check has a **harness bug** — see details below. ⚠️
- F12/F13/F14: Still not implemented. Same failures as rejection 1. ❌

Root causes for this rejection:
1. **F12 — python-project fixture not committed**: Files were created as untracked but never committed. `git clean` removed them.
2. **F13 — typescript-project fixture missing**: `tests/fixtures/typescript-project/` does not exist. `adapters/tests/` does not exist.
3. **F14 — mixed-project fixture missing**: `tests/fixtures/mixed-project/` does not exist. `scripts/verify/P0-W1-F14.sh` is still the TODO stub.

## Criteria

| # | Criterion | Result | Duration | Notes |
|---|-----------|--------|----------|-------|
| 1 | `bash scripts/verify/P0-W1-F03.sh exits 0` | **PASS** | 5.4s | All 5 assertions green; idempotency check green |
| 2 | `cargo test -p ucil-core -- --ignored fixture::rust_project_loads exits 0` | **PASS** | 2.4s | 1 test ran and passed (previous vacuous-pass issue fixed) |
| 3 | `cd tests/fixtures/python-project && uv run pytest ../python_project/test_fixture_valid.py exits 0` | **FAIL** | <1s | Directory absent after `git clean` — fixture was never committed |
| 4 | `pnpm --filter adapters vitest run adapters/tests/fixtures/typescript-project.test.ts exits 0` | **FAIL** | <1s | `ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT: None of the packages has a "vitest" script` — fixture and adapters/tests/ both absent |
| 5 | `bash scripts/verify/P0-W1-F14.sh exits 0` | **FAIL** | <1s | Script outputs `TODO: implement acceptance test for P0-W1-F14` and exits 1 |
| 6 | `cargo clippy -p ucil-cli -- -D warnings exits 0` | **PASS** | 2.9s | No warnings |
| 7 | `cargo clippy -p ucil-core -- -D warnings exits 0` | **PASS** | 1.3s | No warnings |
| 8 | `cargo test --workspace exits 0` | **PASS** | 0.7s | All non-ignored tests green |

## Mutation checks

| Feature | Stashed → fail? | Popped → pass? | Verdict |
|---------|-----------------|----------------|---------|
| P0-W1-F03 | yes | yes | OK |
| P0-W1-F11 | yes | **HARNESS BUG — see below** | UNCONFIRMED |
| P0-W1-F12 | n/a (acceptance test already fails) | — | n/a |
| P0-W1-F13 | n/a (acceptance test already fails) | — | n/a |
| P0-W1-F14 | n/a (acceptance test already fails) | — | n/a |

## Stub scan

No `todo!()`, `unimplemented!()`, or single-`pass` bodies in changed source files
(`crates/ucil-cli/src/commands/init.rs`, `crates/ucil-cli/src/commands/mod.rs`,
`crates/ucil-cli/src/main.rs`, `crates/ucil-core/src/otel.rs`,
`crates/ucil-core/tests/fixture.rs`).

`scripts/verify/P0-W1-F14.sh` contains a `TODO` stub — but that script is a
**verify harness script**, not shipped UCIL source code.

## Failed criterion detail

### Criterion 3 — F12 python-project fixture not committed

**Expected**: `tests/fixtures/python-project/` exists and committed. `tests/fixtures/python_project/test_fixture_valid.py` passes pytest.

**Actual**:
```
$ ls tests/fixtures/python-project/
ls: cannot access 'tests/fixtures/python-project/': No such file or directory
EXIT: 2
```

Before `git clean`, `tests/fixtures/python-project/` and `tests/fixtures/python_project/` existed as **untracked files**. They were removed by `git clean -fdx`, proving they were never committed. The executor created the files but forgot to `git add` and `git commit` them.

### Criterion 4 — F13 typescript-project fixture absent

**Expected**: `tests/fixtures/typescript-project/` exists, `adapters/tests/fixtures/typescript-project.test.ts` exists, pnpm vitest runs it.

**Actual**:
```
$ pnpm --filter adapters vitest run adapters/tests/fixtures/typescript-project.test.ts
ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT  None of the packages has a "vitest" script
EXIT: 1
```

`tests/fixtures/typescript-project/` is missing. `adapters/tests/` does not exist. The `adapters/package.json` has no `vitest` script.

### Criterion 5 — F14 mixed-project fixture absent, verify script is a stub

**Expected**: `tests/fixtures/mixed-project/` exists with intentional lint issues, `scripts/verify/P0-W1-F14.sh` verifies existence and at least one clippy warning.

**Actual** — script content:
```bash
#!/usr/bin/env bash
# Acceptance test for P0-W1-F14
# Executor agents will implement this script during the feature's work-order.
echo "TODO: implement acceptance test for P0-W1-F14"
exit 1
```

Output:
```
$ bash scripts/verify/P0-W1-F14.sh
TODO: implement acceptance test for P0-W1-F14
EXIT: 1
```

Neither `tests/fixtures/mixed-project/` nor the verify script body has been implemented.

## Harness bug: F11 mutation check failure in reality-check.sh

`scripts/reality-check.sh` for kind=`cargo_test` appends `--no-fail-fast` after the selector:

```bash
cargo nextest run $selector --no-fail-fast 2>/dev/null || cargo test $selector --no-fail-fast
```

For the F11 selector `-p ucil-core -- --ignored fixture::rust_project_loads`, this becomes:

```
cargo test -p ucil-core -- --ignored fixture::rust_project_loads --no-fail-fast
```

The `--no-fail-fast` flag is passed **after `--`**, so it reaches the test binary (not cargo), which rejects it:

```
error: Unrecognized option: 'no-fail-fast'
error: test failed, to rerun pass `-p ucil-core --lib`
```

The F11 acceptance test itself passes when run directly (criterion 2 exited 0). The mutation check failure is entirely due to the harness bug, not a code defect.

**This is a Bucket B harness bug.** The `triage` agent should fix `scripts/reality-check.sh` by not appending `--no-fail-fast` after `--` when a `cargo_test` selector already contains `--`.

## Features flipped

- P0-W1-F12 → `attempts=2` (fail verdict) via `flip-feature.sh fail`
- P0-W1-F13 → `attempts=2` (fail verdict) via `flip-feature.sh fail`
- P0-W1-F14 → `attempts=2` (fail verdict) via `flip-feature.sh fail`
- P0-W1-F03 → NOT flipped (whole WO rejected; executor must re-submit with F12/F13/F14)
- P0-W1-F11 → NOT flipped (mutation check unconfirmable due to harness bug; whole WO rejected)

## Repro

```bash
# From a clean checkout of feat/0003-init-fixtures:
cargo clean
git clean -fdx -e ucil-build/ -e .env
# Criterion 3:
ls tests/fixtures/python-project/        # → No such file or directory
# Criterion 4:
pnpm --filter adapters vitest run adapters/tests/fixtures/typescript-project.test.ts
# → ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT
# Criterion 5:
bash scripts/verify/P0-W1-F14.sh       # → TODO; exit 1
```

## Suspected causes

1. **F12**: Executor created the python-project files but did not commit them (untracked). A simple `git add && git commit` was missing.
2. **F13**: Executor never started on the typescript-project fixture implementation.
3. **F14**: Executor never started on the mixed-project fixture or the verify script.

## Next steps

1. **Triage** should apply a Bucket B fix to `scripts/reality-check.sh` to avoid passing `--no-fail-fast` after `--` in cargo_test selectors.
2. **Executor** must implement F12 (commit the python-project fixture), F13 (typescript-project fixture + `adapters/tests/`), and F14 (mixed-project fixture + complete `scripts/verify/P0-W1-F14.sh`).
3. **Executor** may re-submit WO-0003 once all three are committed and all 8 acceptance criteria pass locally.

Note: F03 and F11 do NOT need to be re-implemented. Their acceptance tests are passing.
