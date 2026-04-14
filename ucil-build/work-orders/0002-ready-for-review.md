# WO-0002 ready for verifier review

**Work-order**: WO-0002 `ucil-core-foundations`
**Branch**: `feat/WO-0002-ucil-core-foundations`
**Final commit**: `e5ecae2aff20c1ad4696272242187ab18436774f`
**Features**: P0-W1-F02, P0-W1-F07, P0-W1-F09

## What was verified locally

- `cargo test -p ucil-core types::` → **7 tests passed** (JSON round-trip + derive macros for all 7 types)
- `cargo test -p ucil-core schema_migration::` → **5 tests passed** (stamp creates table+row, check Ok after stamp, empty-db Ok, future-version Downgrade error, idempotent stamps)
- `cargo test -p ucil-core otel::` → **1 test passed** (init_tracer + span + set_attribute + end + shutdown, no panic)
- `cargo clippy -p ucil-core -- -D warnings` → **exit 0** (no warnings, pedantic + nursery enabled)
- `cargo fmt -p ucil-core --check` → **exit 0**
- `cargo build --workspace` → **exit 0** (all 7 workspace crates build clean; no regressions)

## Files changed

| File | Purpose |
|------|---------|
| `Cargo.toml` | Added workspace deps: serde, serde_json, rusqlite (bundled), opentelemetry 0.27, opentelemetry_sdk 0.27, opentelemetry-stdout 0.27, tempfile |
| `crates/ucil-core/Cargo.toml` | Wired new workspace deps into ucil-core |
| `crates/ucil-core/src/types.rs` | 7 domain types with serde, Debug/Clone/PartialEq(+Eq), rustdoc, unit tests |
| `crates/ucil-core/src/schema_migration.rs` | SCHEMA_VERSION, stamp_version(), check_version(), MigrationError, unit tests |
| `crates/ucil-core/src/otel.rs` | init_tracer(), shutdown_tracer(), unit test |
| `crates/ucil-core/src/lib.rs` | pub mod declarations + re-exports |

## Notes for verifier

- `KnowledgeEntry` and `ResponseEnvelope` intentionally do **not** derive `Eq` — they contain `Vec<f32>` and `f64` fields respectively, which are not `Eq`.
- All other types derive `Eq` alongside `PartialEq` (clippy::derive_partial_eq_without_eq).
- OTel uses `opentelemetry_sdk::trace::TracerProvider` (not `SdkTracerProvider` — that rename did not land in 0.27.x).
- Phase-log invariant 8 is observed: stdout-only, no Jaeger/OTLP.
