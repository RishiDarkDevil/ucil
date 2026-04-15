# DEC-0004: Allow skip markers in tests/fixtures/mixed-project/

**Status**: accepted
**Date**: 2026-04-15
**Context**: F14 (P0-W1-F14) requires a `tests/fixtures/mixed-project/` fixture whose
purpose is to contain *intentional* lint/test defects in Rust, TypeScript, and Python.
The work-order explicitly mandates:

> "at least one intentionally failing test in each language (the test must be marked
> `#[ignore]` / `.skip()` / `pytest.mark.skip` to prevent it running in CI — the
> fixture is NOT supposed to have passing tests; the failing test is the point)."

The pre-commit hook `pre-commit-no-ignore` (which enforces DEC-0003) bans `it.skip()`
and `@pytest.mark.skip` site-wide to prevent accidental test silencing. That ban is
correct for UCIL's own test suite, but it incorrectly blocks the defect fixture from
being committed.

**Decision**: Add `tests/fixtures/mixed-project/` to the ALLOW_PREFIX exemption list
in `.githooks/pre-commit-no-ignore`. Files under that path are intentionally defective
reference material, not UCIL test code, and must not be subject to the skip-marker ban.

**Rationale**:
- The fixture's skip markers are load-bearing: they prove that the fixture has failing
  tests while preventing them from contaminating the CI suite.
- Exempting the entire `tests/fixtures/mixed-project/` subtree is scoped enough to
  avoid abusing the exemption for real test code.
- The existing DEC-0003 SLOW-TEST exemption for `#[ignore]` still applies elsewhere;
  this ADR covers only the mixed-project directory.

**Consequences**:
- The hook diff is small (one regex group addition).
- Any future defect-fixture directories must be explicitly added to the exemption list
  (no blanket `tests/fixtures/` exemption).

**Revisit trigger**: If a second defect fixture is added, consider a single
`tests/fixtures/*-defect/` or `tests/fixtures/*-mixed/` glob pattern instead of
per-directory entries.
