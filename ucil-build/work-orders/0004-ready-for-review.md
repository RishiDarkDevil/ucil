# WO-0004 Ready for Review

**Work-order**: WO-0004 — init-pipeline-and-ci  
**Branch**: feat/WO-0004-init-pipeline-and-ci  
**Final commit**: d50caaf7e0860031b3c99574ab8ae5dc677ca125  
**Features**: P0-W1-F04, P0-W1-F05, P0-W1-F06, P0-W1-F08

## What was verified locally

- `cargo nextest run -p ucil-cli --test-threads 1` → **8/8 PASSED** including:
  - `test_llm_provider_selection` PASS — `--llm-provider ollama` writes `provider = "ollama"` to ucil.toml; absent provider defaults to `"none"`.
  - `test_plugin_health_verification` PASS — `verify_plugin_health()` returns one entry per P0 plugin (Ok or Degraded), never panics on missing binaries; `skipped_plugin_health()` returns all Skipped.
  - `test_init_report_json` PASS — `run()` with `--no-install-plugins` produces `.ucil/init_report.json` with correct fields (schema_version=1.0.0, llm_provider=claude, languages=[rust], plugin_health all skipped).
- `bash scripts/verify/P0-W1-F08.sh` → exits 0 — ci.yml exists, YAML validates, smoke test produces init_report.json, JSON is valid with required fields.
- `cargo clippy -p ucil-cli -- -D warnings` → exits 0 (no warnings).
- `cargo build -p ucil-cli` → exits 0.
- `ucil init --no-install-plugins` in fresh temp dir → `.ucil/init_report.json` produced (exercised by P0-W1-F08.sh).

## Commits in this work-order

1. `a5a5470` — feat(cli): add LlmProvider, --llm-provider, --no-install-plugins, verify_plugin_health, InitReport + init_report.json (F04/F05/F06)
2. `d50caaf` — feat(ci): add .github/workflows/ci.yml (rust/ts/python jobs) + implement P0-W1-F08.sh (F08)
