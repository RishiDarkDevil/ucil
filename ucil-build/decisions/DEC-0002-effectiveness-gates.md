---
id: DEC-0002
title: Add effectiveness, usability, and end-to-end verification gates
date: 2026-04-15
status: accepted
raised_by: user
---

# DEC-0002: Effectiveness, usability, and end-to-end verification gates

## Context

After WO-0001 and WO-0002 landed, the harness was verified to check structural
(does it compile?) and functional (does it respond?) correctness. It did NOT
check **effectiveness** (does UCIL actually help an agent produce better
outcomes than the baseline?) or **end-to-end usability** (can a user drop UCIL
into a new project and actually use it?).

The master plan's §19.1 lists "Quality — context quality vs baselines — Compare
UCIL vs Aider/Cursor" and §15.3 names `ucil.bonus.usage_rate` as a metric, but
no concrete gates existed for either. The user flagged the gap explicitly and
asked for every phase to include live-usage testing with agents driving real
tasks against fixtures + real OSS repos + UCIL itself.

## Decision

Add seven new verification dimensions to the phase-gate scripts, implemented as
required scripts in `scripts/verify/*.sh`:

1. **Effectiveness evaluator** — per-scenario A/B comparison (UCIL vs. baseline
   grep+Read+LSP) scored by an LLM-as-judge rubric. Required from Phase 1
   onwards. Discovers `tests/scenarios/*.yaml` tagged with the current phase.

2. **Multi-language coverage** — probes UCIL's MCP against Rust + Python +
   TypeScript fixtures from Phase 1; adds Go from Phase 3. Required from
   Phase 1 onwards.

3. **Real OSS repo smoke** — clones a bounded well-known OSS repo (ripgrep,
   fastify, requests), runs `ucil init`, runs canned queries, validates
   results against known correct answers. Required from Phase 2 onwards.

4. **Dogfood on self** — UCIL indexes the ucil repo and answers a query whose
   answer we know (because we built it). Required from Phase 3 onwards.

5. **Concurrency** — three concurrent headless agent sessions across worktrees
   sharing the daemon. Checks no SQLITE_BUSY, RSS cap, DB integrity post-run.
   Required from Phase 3 onwards.

6. **Stability** — 30-minute mixed-load run with RSS sampling, P95 latency
   comparison, graceful shutdown. Required from Phase 6 onwards.

7. **Privacy / data-locality** — egress monitoring + secret scan of source +
   `.ucil/` PII scan. Validates the master-plan principle "Fully local. Zero
   cloud dependencies." Required from Phase 5 onwards.

8. **Host-adapter conformance** — per-host adapter transform + round-trip
   tests for Claude Code, Codex CLI, Cursor, Cline, Aider, Ollama. Required
   from Phase 4 onwards.

9. **Bonus-context usage rate** — synthetic session measures what fraction of
   offered bonus fields (conventions, pitfalls, related_code) are consumed by
   follow-up agent calls. Must be ≥ 0.30. Required from Phase 6 onwards.

10. **User journey** — a fresh sandbox, `scripts/install.sh`, `ucil init` on a
    real project, Claude Code plugin install, headless task requiring UCIL
    semantic surface, `ucil status`, `ucil export-brain`/`import-brain`
    round-trip. **This is the v0.1.0 release acceptance bar.** Required at
    Phase 8.

11. **Docs walkthrough** — a simulated new user (fresh claude -p session) with
    ONLY `docs/` in scope tries to install + use UCIL. Judges the docs'
    completeness. Required at Phase 8.

## Implementation

New files:
- `.claude/agents/effectiveness-evaluator.md` — new subagent
- `scripts/run-effectiveness-evaluator.sh` — launcher
- `scripts/verify/effectiveness-gate.sh` — generic per-phase gate runner
- `scripts/verify/{multi-lang-coverage,real-repo-smoke,dogfood-on-self,concurrency,stability,privacy-scan,host-adapter-conformance,bonus-usage-rate,user-journey,docs-walkthrough}.sh`
  — placeholder scripts that executors flesh out during their phase work.
  Each contains the full contract as comments.
- `tests/scenarios/README.md` — scenario format spec
- `tests/scenarios/{nav-rust-symbol,refactor-rename-python,add-feature-ts,arch-query}.yaml`
  — four initial scenarios covering Rust, Python, TypeScript, mixed-lang.

Gate wiring:
- `scripts/gate/phase-{1..8}.sh` updated to require the applicable checks.
- The effectiveness gate rejects the phase if ANY scenario FAILS or if no
  scenario tagged for the phase exists (phases 1–7 only; Phase 0 and 8 are
  handled differently).

## Consequences

### Positive
- Every phase from 1 onwards is effectiveness-tested before ship.
- Executors are forced to flesh out the stub verification scripts during their
  phase work — they cannot pass the gate without real implementations.
- Phase 8 cannot ship without a working new-user install flow.
- Agents can add more scenarios as they build features; the framework scales.
- The master plan's implied-but-unchecked quality principles (§1.2, §15.3,
  §19.1) now have teeth.

### Negative / costs
- Additional compute: ~3 claude -p sessions per scenario (UCIL run + baseline
  run + judge). At 4 scenarios × 7 phases = ~84 sessions. Estimated ~5–15M
  tokens total across the 24-week build.
- Phases 1 and 2 now have stub scripts that MUST be implemented by executors;
  executors cannot skip these (gate fails until they exist as working code).
- Executors may need to emit work-orders specifically for these verification
  scripts ("WO-NNNN-implement-effectiveness-scaffolding"). Planner should
  naturally pick this up but may need prompt reinforcement.

### Mitigations
- Scenarios are deliberately small-scope (one symbol, one refactor, one
  feature each) to keep per-scenario token spend bounded.
- Placeholder scripts contain full contracts as comments so executors know
  exactly what to implement without requiring a new planner round.
- Tool-availability probing means scenarios auto-skip when their required UCIL
  tools aren't yet implemented — no wasted compute in early phases.

## Revisit trigger

- If the effectiveness gate rejects >30% of phase attempts, re-examine whether
  the rubric is too strict or the baseline too unfair.
- If the total token cost of evaluation exceeds 20% of the build's total, add
  sampling (run only N of M scenarios per gate) with rotation.
