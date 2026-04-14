# UCIL — Unified Code Intelligence Layer

> Autonomous-build harness + spec. The code is built by Claude Code agents from `ucil-master-plan-v2.1-final.md` according to the rules in `CLAUDE.md`.

## What this repo contains right now

- **`ucil-master-plan-v2.1-final.md`** — the 121 KB definitive spec for UCIL v0.1.0 (24 weeks, 9 phases).
- **`CLAUDE.md`** — root rules of engagement for every agent building UCIL.
- **`.claude/`** — subagents, hooks, skills, and settings that orchestrate the build.
- **`.githooks/`** — pre-commit / pre-push guards (feature-list whitelist, secret scan, no-ignore).
- **`ucil-build/`** — build-harness brain: `feature-list.json` (immutable oracle), `progress.json`, work-orders, verification reports, ADRs.
- **`scripts/`** — bootstrap, gate-check, spawn-verifier, flip-feature, run-phase, run-all.

What this repo does **not** yet contain (it will after the autonomous build runs):
`crates/`, `adapters/`, `ml/`, `plugin/`, `plugins/`, `tests/fixtures/`, `docs/`.

## Kickoff (first-time setup)

```bash
cd /home/rishidarkdevil/Desktop/ucil
./scripts/install-prereqs.sh                     # one sudo prompt; rust+node+python+docker+ollama+scanners
cp .env.example .env && $EDITOR .env              # fill ANTHROPIC_API_KEY, GITHUB_TOKEN
./scripts/bootstrap.sh                            # wires git hooks, validates harness
./scripts/verify-harness.sh                       # asserts every hook/agent/skill/MCP is wired
./scripts/seed-features.sh                        # one-shot planner → feature-list.json (5-15 min, ~2M tokens)
# review ucil-build/feature-list.json, then:
UCIL_SEEDING=1 git add ucil-build/feature-list.json
UCIL_SEEDING=1 git commit -m "freeze: feature oracle v1.0.0"
git push -u origin main
jq '.seeded = true' ucil-build/progress.json > /tmp/p.json && mv /tmp/p.json ucil-build/progress.json
git add ucil-build/progress.json && git commit -m "chore: mark features seeded" && git push

# Now start the autonomous build:
claude    # then: /phase-start 0
```

Or hands-free end-to-end:

```bash
./scripts/run-all.sh           # prompts between phases
./scripts/run-all.sh --yes     # fully autonomous 0 → 8
```

## The autonomous loop

```
Planner (Opus) ──► Executor (Opus, worktree) ──► Critic (Opus, read-only) ──► Verifier (Opus, FRESH SESSION)
   │                    │                              │                            │
   │ work-order         │ commits + pushes             │ findings                   │ flips passes=true
   │                    │                              │                            │ (only writer)
   ▼                    ▼                              ▼                            ▼
   work-orders/   git branches                  critic-reports/           feature-list.json
```

**The verifier is always spawned in a FRESH Claude Code session** via `scripts/spawn-verifier.sh` with `--no-resume` and a new session ID. This is the single most important guard against agentic laziness: the verifier cannot trust the executor's "tests green" claim — it must re-run `cargo clean && cargo test` itself.

## Phase gates

Before a phase ships, `scripts/gate-check.sh N` must return 0:

- All features in phase N have `passes: true`.
- Each feature's `last_verified_by` starts with `verifier-` (not the executor).
- `scripts/gate/phase-N.sh` exits 0 (encodes master-plan deliverables).
- No feature's test is in the flake-quarantine list.

The Stop hook (`.claude/hooks/stop/gate.sh`) refuses to let the agent end its turn cleanly if:
- The working tree is dirty or the branch is ahead of upstream (commit + push first).
- The current phase gate is red.

## Anti-laziness contract (enforced mechanically)

See `CLAUDE.md` §"Anti-laziness contract" for the full list. Highlights:

- `#[ignore]`, `.skip`, `xfail`, `it.skip` — blocked by `pre-commit-no-ignore`.
- `todo!()`, `unimplemented!()`, `NotImplementedError`, `pass`-only bodies — rejected by critic, reverted by verifier.
- Modifying `tests/fixtures/**` — blocked by `pre-tool-use/path-guard.sh`.
- Self-verification (same session writes and verifies) — blocked by `scripts/flip-feature.sh`.
- `git push --force`, `git commit --amend` post-push — blocked by `pre-tool-use/block-dangerous.sh` and `.githooks/pre-push`.
- Mocking Serena / LSP / SQLite / LanceDB / Docker in tests — blocked by critic.
- Leaving uncommitted work at session end — blocked by Stop hook.

## License

MIT. See `LICENSE`.
