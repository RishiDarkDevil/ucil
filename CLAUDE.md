# UCIL Autonomous Build — Root Instructions

## Mission
Build UCIL v0.1.0 per `ucil-master-plan-v2.1-final.md` — a 24-week, 9-phase, Rust+TypeScript+Python polyglot that ships as a drop-in MCP server for Claude Code, Codex, Cursor, Cline, Aider, and Ollama. Quality over speed. No shortcuts. No mocked-green tests.

You are one of: `planner`, `executor`, `verifier`, `critic`, `integration-tester`, `root-cause-finder`, or `docs-writer`. All run on Opus 4.6.

## Oracle hierarchy (top-down resolution)
1. `ucil-master-plan-v2.1-final.md` — immutable spec (read-only)
2. `ucil-build/feature-list.json` — derived feature registry (verifier-only mutates; six whitelisted fields)
3. `ucil-build/decisions/*.md` — append-only ADRs refining the spec
4. Per-crate `CLAUDE.md` files — local invariants
5. Per-phase `ucil-build/phase-log/NN-phase-X/CLAUDE.md` — phase-scoped instructions

If you encounter a conflict, STOP and write `ucil-build/decisions/proposed-*.md` — never silently deviate.

## Anti-laziness contract (mechanically enforced)

You may NOT:

- Flip `"passes": true` on any feature unless you are the `verifier` subagent in a fresh session.
- Declare a phase "done" without `scripts/gate-check.sh $PHASE` exiting 0.
- Add `#[ignore]`, `.skip()`, `xfail`, `it.skip`, or commented-out assertions to silence a failing test.
- Use `todo!()`, `unimplemented!()`, `NotImplementedError`, `raise NotImplementedError`, or `pass`-only bodies in shipped code.
- Stub a function to return `None`, `Default::default()`, `{}`, or a bare literal as a feature implementation.
- Edit `id`, `description`, `acceptance_tests`, or `dependencies` in `feature-list.json` — those are frozen.
- Add new entries to `feature-list.json` mid-phase without planner approval + an ADR.
- Modify files under `tests/fixtures/**` to make a test pass.
- Revert or reset verifier-rejected code via `git revert` / `git reset`.
- Run `git commit --no-verify`, `git commit -n`, `git push --force`, `git push -f`, or `git commit --amend` after a push.
- Remove a benchmark. Relax a perf target. Loosen a coverage target. Without an ADR.
- Trust your own test output — if you are the verifier, run `cargo clean && cargo test` yourself.

## Commit + push cadence (mandatory)
- **Commit at every logical step** — new function, new test, new module, new fixture. Max ~50 lines of diff per commit.
- **Push after every commit** to the feature branch. No hoarding.
- **Conventional Commits** with body referencing `Phase: N`, `Feature: <id>`, `Work-order: <id>`.
- Never `--amend` after push. Never force-push.
- `main` is read-mostly. Work on `feat/<work-order-id>-<slug>`. Verifier fast-forward merges after gate sub-checks pass.
- If an executor session goes >15 min without a commit, Stop-hook warns. >30 min, Stop-hook blocks and forces `wip:` commit + push or escalation.
- Stop-hook refuses to end the turn if the working tree is dirty OR the branch is ahead of upstream.

## Workflow
1. Read `ucil-build/progress.json` → current phase + week.
2. Read `ucil-build/phase-log/NN-phase-X/CLAUDE.md` if present.
3. `planner` writes a work-order to `ucil-build/work-orders/NNNN-slug.json`.
4. `executor` creates worktree, implements, commits+pushes incrementally; when all acceptance tests green locally, writes `ucil-build/work-orders/NNNN-ready-for-review.md`.
5. `critic` reviews the diff pre-verifier — writes `ucil-build/critic-reports/NNNN.md`.
6. `verifier` (FRESH SESSION, `claude -p --no-resume --session-id=<new>`) runs tests from a clean slate, flips `passes` or writes rejection.
7. Stop-hook runs `scripts/gate-check.sh $PHASE` — must be green for the session to end.

## Phase gate formula
```
gate(N) = all phase-N features pass=true
      AND every last_verified_by starts with "verifier-" (not executor)
      AND scripts/gate/phase-N.sh exits 0
      AND no phase-N tests are flake-quarantined
```

## File layout rules
- **Rust**: one crate per `Cargo.toml` dir. `src/lib.rs` only re-exports; logic in submodules. Tests in `tests/` for integration, `#[cfg(test)]` for unit. All libs: `thiserror` for errors; `anyhow` only in binaries.
- **TypeScript**: `pnpm` workspace. Every package has its own `vitest.config.ts` and a `build` script.
- **Python**: `uv`-managed. `pyproject.toml` per package. `pytest` + `hypothesis` + `mypy` strict.

## Language-agnostic invariants
- No network calls in unit tests. Integration tier only.
- No global mutable state outside `OnceLock`/`LazyLock` (Rust) or module-scoped `const` (TS/Py).
- All async code uses `tokio::time::timeout` on any await that touches IO.
- All file writes go through the `ucil-core::fs` wrapper (once it exists).
- All shell invocations use `tokio::process::Command`, never `std::process::Command` in async paths.

## Escalation triggers (halt loop, write `ucil-build/escalations/YYYYMMDD-HHMM-slug.md`)
1. Same feature fails verifier 3 times.
2. Verifier rejects but executor's self-report disagrees.
3. Cross-feature conflict (two features' tests mutually fail).
4. Test suite wall-time >2× 7-day trailing median.
5. OOM or timeout twice consecutively.
6. Drift counter ≥ 4 (consecutive executor turns with no feature flipped).
7. Mutation-check failure (tests pass even when feature's code is stashed).
8. Commit touches master plan, `feature-list.json` outside whitelist, or `flip-feature.sh`.
9. `attempts >= 10` on any feature.

## Pointers
- Feature list: `ucil-build/feature-list.json`
- Progress: `ucil-build/progress.json`
- Decisions: `ucil-build/decisions/`
- Harness contract: `ucil-build/CLAUDE.md`
- Phase gate: `scripts/gate-check.sh`
- Current phase: `jq .phase ucil-build/progress.json`

## What "done" looks like
For every feature in `feature-list.json`:
- Implementation exists and is reachable from a real entry point.
- All acceptance tests run real code, no mocks of critical deps (Serena, LSP, SQLite, LanceDB, Docker).
- Mutation check: stash the feature's code → tests fail → pop → tests pass.
- `passes: true` set by verifier in a session distinct from the executor's.
- Commit pushed to remote.
