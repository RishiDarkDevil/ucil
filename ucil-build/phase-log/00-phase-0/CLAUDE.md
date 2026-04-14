# Phase 0 — Project bootstrap (Week 1)

Scope derived from `ucil-master-plan-v2.1-final.md` §18 Phase 0 (line 1710) and §17 (directory skeleton). This file supplements, never overrides, the root `/CLAUDE.md` anti-laziness contract.

## Goal

Ship a compilable repository skeleton where `ucil init` works end-to-end: Cargo workspace, TypeScript pnpm workspace under `adapters/`, and `uv`-managed Python env under `ml/` all build cleanly on a fresh checkout; `ucil init` creates `.ucil/`, detects languages, verifies P0 plugin health, selects an LLM provider, and writes `.ucil/init_report.json`. CI lanes for all three toolchains pass. OpenTelemetry stdout exporter initialises without panic. Fixture projects (rust/python/typescript/mixed) are committed as **real code**, not stubs. Nothing beyond this scaffolding is in scope — indexing, daemon, MCP tools, plugins, agents all ship in later phases.

## Features in scope (14)

- **P0-W1-F01** — Cargo workspace + TS pnpm workspace + uv Python env all compile (root; blocks 13 others).
- **P0-W1-F02** — Core type system in `ucil-core/src/types.rs`: QueryPlan, Symbol, Diagnostic, KnowledgeEntry, ToolGroup, CeqpParams, ResponseEnvelope.
- **P0-W1-F03** — `ucil init` creates `.ucil/`, detects languages, writes `ucil.toml` defaults.
- **P0-W1-F04** — `ucil init --llm-provider` selector writes `[llm]` section to `ucil.toml`.
- **P0-W1-F05** — `ucil init` runs P0 plugin health verification (probes external tool binaries).
- **P0-W1-F06** — `ucil init` writes `.ucil/init_report.json` with detections + health + provider + schema version.
- **P0-W1-F07** — Schema version tracking in `ucil-core`: version stamped in `.ucil/state.db`, checked on startup.
- **P0-W1-F08** — CI pipeline: Rust cargo test+clippy, TS biome+vitest, Python ruff+mypy+pytest all green on clean checkout.
- **P0-W1-F09** — OpenTelemetry skeleton in `ucil-core/src/otel.rs`: stdout exporter, spans, no startup panic.
- **P0-W1-F10** — Full repo directory skeleton per §17 (crates/, adapters/, plugins/, ml/, tests/fixtures/, scripts/, docs/).
- **P0-W1-F11** — `tests/fixtures/rust-project/` (~5K LOC Rust), real Cargo project.
- **P0-W1-F12** — `tests/fixtures/python-project/` (~5K LOC Python), real `pyproject.toml` project.
- **P0-W1-F13** — `tests/fixtures/typescript-project/` (~5K LOC TypeScript), real `tsconfig.json` project.
- **P0-W1-F14** — `tests/fixtures/mixed-project/` (~3K LOC) with **intentional** lint errors, type errors, security issues, failing tests.

Dependency topology: F01 is the root. F02/F07/F09/F10/F11/F12/F13/F14 all depend on F01 only; F03 depends on F02; F04 and F05 depend on F03; F06 depends on F05; F08 depends on F01. F01 therefore ships first, alone or with F10 at most.

## Gate criteria

`scripts/gate/phase-0.sh` must exit 0. It runs (conditionally on files existing):
1. `cargo build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --all --check`.
2. `pnpm -C adapters build` if `adapters/package.json` exists.
3. `uv build` in `ml/` if `ml/pyproject.toml` exists.
4. `ucil init --no-install-plugins` smoke test in a temp dir; must produce `.ucil/init_report.json`.

Additionally: every phase-0 feature's `passes=true` AND `last_verified_by` starts with `verifier-` AND no phase-0 tests are flake-quarantined (per root gate formula).

## Dependencies required

Host toolchains (checked by `scripts/gate/phase-0.sh` transitively via build commands):
- **Rust stable** — pinned via `rust-toolchain.toml` (per `.claude/rules/rust-style.md`). Edition 2021.
- **Node 20+ LTS** with **`pnpm`** (per `.claude/rules/ts-style.md`). Biome is the only linter/formatter.
- **Python 3.11+** with **`uv`** (per `.claude/rules/py-style.md`). `ruff format`, `ruff check`, `mypy --strict`, `pytest`.

Explicitly **not** required in Phase 0: Docker, LanceDB, SQLite runtime binaries, Serena, any MCP plugin processes, ONNX Runtime, GPU. Plugin health verification (F05) probes binaries that may be absent — the probe MUST degrade gracefully and record the absence in `init_report.json`, not fail init.

## Risks carried

None. This is phase 0.

## Phase-0-specific invariants

1. **Skeleton-only**: no feature in Phase 0 implements indexing, query, MCP, daemon loop, plugin lifecycle, or agents. If a work-order strays beyond init/types/otel/fixtures/CI, planner must split it.
2. **Fixtures are real code, not stubs**: F11–F14 must be compileable/runnable code (LOC targets are approximate). F14 must contain **genuine** lint/type/security/test defects — not `// intentional error` comments that get ignored by tooling. The mixed-project's failing tests are the oracle later phases use to validate G7/G8 fusion.
3. **CI passes on a clean checkout** (F08): fresh `git clone` + the three toolchain installs must be sufficient. No hidden manual step.
4. **No `unwrap()`/`expect()`** outside `#[cfg(test)]` even in scaffolding. Per Rust style rules, anything that can fail uses `?` and a typed error.
5. **`src/lib.rs` is re-exports only** in every new Rust crate (per Rust style rules). Logic lives in submodules, including scaffolding code.
6. **No `.unwrap()` to get init moving**: `ucil init` paths return typed errors and propagate via `anyhow::Context` in the binary.
7. **Schema version (F07)** is `1.0.0` at Phase 0. The migration runner's only job in P0 is "stamp current version, refuse to downgrade." No real migrations ship yet.
8. **OpenTelemetry (F09)** uses `opentelemetry_stdout` exporter only. No Jaeger, no OTLP collector wiring — that is Phase 6.
9. **Work-order granularity**: F01 alone is one WO. F02+F07+F09 can ship together (all `ucil-core` skeleton). F03+F04+F05+F06 ship together as the `ucil init` pipeline after F02. F10 may bundle with F01. F08 ships after Rust/TS/Python trees exist. F11/F12/F13/F14 may each be their own WO given the LOC volumes.
10. **Reference §17 verbatim** for directory names. Do not rename crates. `ucil-core`, `ucil-daemon`, `ucil-cli`, `ucil-treesitter`, `ucil-lsp-diagnostics`, `ucil-agents`, `ucil-embeddings` — exact spellings.
