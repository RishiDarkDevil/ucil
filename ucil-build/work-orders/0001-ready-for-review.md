# WO-0001 Ready for Review

**Work-order**: WO-0001 — workspace-skeleton  
**Branch**: feat/WO-0001-workspace-skeleton  
**Final commit**: 70d710ac113f96de44e548050a0cfb87bb2941d0  
**Features**: P0-W1-F01, P0-W1-F10  
**Date**: 2026-04-15

## What was verified locally

- [x] `cargo build --workspace` exits 0 (clean `cargo clean` run, all 7 crates compile)
- [x] `cargo clippy --workspace -- -D warnings` exits 0 (no warnings or errors)
- [x] `cargo fmt --all --check` exits 0 (no formatting drift)
- [x] `cargo test -p ucil-core` exits 0 — 2 tests pass in `tests/smoke.rs`:
  - `version_is_semver`
  - `version_has_two_dots`
- [x] `pnpm -C adapters install && pnpm -C adapters build` exits 0 (TypeScript compiles cleanly)
- [x] `cd ml && uv build` exits 0 — builds `ucil_ml-0.1.0.tar.gz` and `ucil_ml-0.1.0-py3-none-any.whl`
- [x] `bash scripts/verify/P0-W1-F10.sh` exits 0 — all §17 required directories present

## Commits on this branch

- `c82795c` feat(workspace): add Cargo workspace with 7 crates + rust-toolchain.toml
- `c4c042e` fix(workspace): fix clippy doc_markdown lints in daemon and cli lib.rs
- `40c788a` chore(workspace): update .gitignore, P0-W1-F10.sh impl, Cargo.lock
- `f544fe1` feat(adapters): add pnpm workspace skeleton under adapters/
- `319922b` feat(ml): add uv-managed Python project skeleton under ml/
- `70d710a` feat(skeleton): add full §17 directory tree with .gitkeep placeholders

## Scope delivered

### P0-W1-F01 — Workspace configs
- `Cargo.toml` workspace with 7 members + shared deps (`thiserror`, `anyhow`, `tokio`, `tracing`)
- `rust-toolchain.toml` pinning stable channel with `rustfmt` + `clippy`
- 7 crates: `ucil-core`, `ucil-daemon`, `ucil-cli`, `ucil-treesitter`, `ucil-lsp-diagnostics`, `ucil-agents`, `ucil-embeddings`
  - All library `lib.rs` files are re-export-only (no logic) per rust-style.md
  - `ucil-daemon` and `ucil-cli` have `src/main.rs` shells that compile and exit 0
- `adapters/` pnpm workspace: `package.json`, `pnpm-workspace.yaml`, `tsconfig.json` (strict), `biome.json`, `src/index.ts`
- `ml/` uv project: `pyproject.toml` (hatchling, python≥3.11, ruff+mypy+pytest dev-deps), `ml/src/ucil_ml/__init__.py`
- `.gitignore` covering `target/`, `node_modules/`, `.venv/`, `dist/`, `__pycache__/`, `.ucil/`, `adapters/*/dist/`

### P0-W1-F10 — Directory skeleton
- All §17 directories present with `.gitkeep` placeholders for empty ones
- `scripts/verify/P0-W1-F10.sh` replaced with real assertions (exits 1 + names first missing dir on failure)

## Scope intentionally excluded (per scope_out)

- `ucil-core/src/types.rs` (P0-W1-F02)
- `ucil init` command (P0-W1-F03 through F06)
- Schema version tracking (P0-W1-F07)
- CI workflow logic (P0-W1-F08) — `.github/workflows/` dir present with `.gitkeep`
- OpenTelemetry wiring (P0-W1-F09)
- Fixture project bodies (P0-W1-F11 through F14)
- Plugin manifests (later phases)
