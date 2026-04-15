# WO-0004 ready for review

**Work-order**: WO-0004 (init-pipeline-and-ci)
**Branch**: feat/WO-0004-init-pipeline-and-ci
**HEAD commit**: d2af2f92bc73234bf019027b71ec61f41f8ecd5c
**Marked ready**: 2026-04-15T20:00:00Z
**Retry**: 2 (fixing verifier rejection from retry 1)

## Fix applied (retry 2)

**B1 — tokio::time::timeout on IO await** (the sole rejection blocker from verifier session
`vrf-41a07ee5` and root-cause-finder report `verification-reports/root-cause-WO-0004.md`):

Added `PLUGIN_PROBE_TIMEOUT` (5 s) constant and wrapped `.output().await` in
`tokio::time::timeout` inside `verify_plugin_health()` in
`crates/ucil-cli/src/commands/init.rs:174–192`.

Also addressed advisory warnings from critic/RCF:
- **S3**: `cargo test --workspace` → `cargo nextest run --workspace` in `.github/workflows/ci.yml`
- **S4**: `uv sync --all-extras` → `uv sync --all-packages` in `.github/workflows/ci.yml`

## Acceptance criteria — all green locally

| # | Criterion | Result |
|---|-----------|--------|
| 1 | `cargo nextest run -p ucil-cli` — `test_llm_provider_selection` | PASS |
| 2 | `cargo nextest run -p ucil-cli` — `test_plugin_health_verification` | PASS |
| 3 | `cargo nextest run -p ucil-cli` — `test_init_report_json` | PASS |
| 4 | `bash scripts/verify/P0-W1-F08.sh` | PASS |
| 5 | `cargo clippy -p ucil-cli -- -D warnings` | PASS |
| 6 | `cargo build -p ucil-cli` | PASS (implicit via nextest compile) |

## Features implemented

- P0-W1-F04 — LLM provider selection (`--llm-provider`, writes `[llm]` section to ucil.toml)
- P0-W1-F05 — P0 plugin health verification (`verify_plugin_health()`, timeout-bounded)
- P0-W1-F06 — `init_report.json` serialisation
- P0-W1-F08 — CI workflow (`.github/workflows/ci.yml`) + `scripts/verify/P0-W1-F08.sh`
