# Commit style (UCIL)

Use Conventional Commits with UCIL-specific body fields.

## Format
```
<type>(<scope>): <short summary, imperative, <=70 chars>

<body — what and why, not how. Optional. Wrap at 72.>

Phase: <N>
Feature: <FEATURE-ID>
Work-order: <WO-ID>

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
```

## Types
- `feat` — new user-visible capability.
- `fix` — bug fix.
- `refactor` — no behavior change.
- `test` — tests only.
- `docs` — docs only.
- `perf` — perf improvements.
- `build` — build system, Cargo, pnpm.
- `ci` — CI config.
- `chore` — meta.
- `wip` — in-progress work (only permitted when Stop-hook forces a save).

## Scope
Short crate/package/module name: `core`, `treesitter`, `daemon`, `cli`, `embeddings`, `adapter-claude-code`, `ml-embed`, etc.

## Rules
- One logical change per commit. No "kitchen-sink" commits.
- ~50 lines of diff per commit is a soft target. Big refactors split into review-friendly chunks.
- Push immediately after commit. No hoarding.
- `--amend` after push is forbidden.
- Force-push is forbidden.
- Every feature commit includes `Feature: <ID>` referencing a real entry in `ucil-build/feature-list.json`.

## Examples

```
feat(treesitter): add LMDB-backed tag cache keyed by mtime

Warm reads complete in <1ms on the fixture rust-project. Cold reads fall
back to parse-and-cache.

Phase: 1
Feature: P1-W2-F03
Work-order: WO-0042

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
```

```
test(daemon): cover signal handling on SIGTERM and SIGHUP

Phase: 1
Feature: P1-W3-F07
Work-order: WO-0055

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
```
