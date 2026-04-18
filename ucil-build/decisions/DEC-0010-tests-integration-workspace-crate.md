# DEC-0010: Workspace-member `ucil-tests-integration` crate at `tests/integration/` hosts cross-crate integration test binaries

**Status**: accepted
**Date**: 2026-04-18
**Work-order**: WO-0038 (P1-W5-F08)
**Supersedes**: —

## Context

17 features in `ucil-build/feature-list.json` declare `"crate": "tests/integration"`
and cargo selectors of the form `--test test_<name>` (without `-p <crate>`).
Examples:

| Phase  | Feature      | Selector                                |
|--------|--------------|-----------------------------------------|
| P1-W5  | `F08`        | `--test test_lsp_bridge`                |
| P2-W6  | `F08`        | `--test test_plugin_lifecycle`          |
| P3-W9  | `F11`        | `--test test_incremental`               |
| P3-W11 | `F13`-`F16`  | `--test test_quality_pipeline`, etc.    |
| P3.5   | `W12-F09`    | `--test test_agents_smoke`              |
| P3.5   | `W13-F07..9` | `--test test_multi_agent`/`_ceqp`/`_resilience` |
| P4-W15 | `F08`        | `--test test_*`                         |
| P5-W17 | `F11`        | `--test test_*`                         |
| P5-W18 | `F08..F09`   | `--test test_*`                         |
| P6-W20 | `F11..F12`   | `--test test_*`                         |
| P8-W23 | `F06`        | `--test test_*`                         |

The path `tests/integration/` in the repo root is currently a directory
containing only `.gitkeep` — not a cargo crate. Rust integration tests live
inside a crate's `tests/` subdirectory; to satisfy the
`cargo test --test test_<name>` selector at workspace level, a crate must
house the integration binaries, and that crate must be a workspace member
so cargo discovers its `--test` binaries.

Feature descriptions cite literal paths like
`"LSP and Serena integration test suite (tests/integration/test_lsp_bridge.rs)
passes against all four fixture projects"` — the file is expected to live at
the repo-relative path `tests/integration/test_lsp_bridge.rs`, not at
`tests/integration/tests/test_lsp_bridge.rs` (the cargo default for a
crate's integration tests).

Two candidate placements surfaced while planning WO-0038:

1. Put `test_lsp_bridge.rs` under `crates/ucil-lsp-diagnostics/tests/`. Ad-hoc,
   per-feature. Would need to be repeated 17 times across 8 phases, often
   with cross-crate deps the host crate doesn't own.
2. Create a dedicated workspace-member crate rooted at `tests/integration/`
   that uses Cargo's `[[test]] name = … path = …` overrides so each test's
   source file lands at the exact path cited in the feature-list description.

## Decision

Adopt option **2**: introduce a new workspace-member crate named
`ucil-tests-integration`, rooted at `tests/integration/`, whose only purpose
is to compile and link cross-crate integration test binaries. Each test
binary is declared with a `[[test]]` entry that pins both the binary name
(matching the `--test <name>` selector) and the source path (matching the
repo-relative path in the feature-list description).

### File layout

```
tests/integration/
├── Cargo.toml              # new — workspace member, publish = false
├── src/
│   └── lib.rs              # new — empty crate root (harness, not a library)
├── test_lsp_bridge.rs      # new — P1-W5-F08 binary (WO-0038 adds)
└── <future>_test_*.rs      # one file per future integration feature
```

### `tests/integration/Cargo.toml` skeleton

```toml
[package]
name = "ucil-tests-integration"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false
description = "UCIL cross-crate integration test binaries (tests/integration/*.rs)."

[lib]
path = "src/lib.rs"

[[test]]
name = "test_lsp_bridge"
path = "test_lsp_bridge.rs"

[dev-dependencies]
tokio       = { workspace = true }
async-trait = { workspace = true }
lsp-types   = { workspace = true }
serde_json  = { workspace = true }
tempfile    = { workspace = true }
ucil-core            = { path = "../../crates/ucil-core" }
ucil-lsp-diagnostics = { path = "../../crates/ucil-lsp-diagnostics" }
```

Future WOs for P2-W6-F08, P3-W9-F11, etc. add more `[[test]]` entries and
per-feature `test_<name>.rs` source files to the same crate — no further ADR
required.

### Root `Cargo.toml` edit

Add `"tests/integration"` to `[workspace].members`. No other workspace
files change.

## Rationale

- **Spec fidelity**: the feature-list's `"crate": "tests/integration"` field
  is frozen (only six fields are mutable post-seed per `ucil-build/CLAUDE.md`).
  The literal path `tests/integration/test_<name>.rs` in feature descriptions
  is the authoritative home; `[[test]] path =` overrides honor it exactly.
- **Selector compatibility**: `cargo test --test test_lsp_bridge` resolves
  workspace-wide without requiring `-p` flags, matching the frozen selectors.
- **Scalability**: 17 pending features use the same pattern. A single
  dedicated crate is the only scalable home — repeating the
  cross-crate-test setup 17 times across host crates would duplicate
  dev-deps and muddle ownership.
- **Dependency isolation**: integration tests that pull in multiple crates
  under test (bridge + KG + plugin manager) have a natural home without
  polluting those crates' dev-deps.
- **Zero impact on existing tests**: no existing crate loses a test; no
  existing test changes selector.

## Consequences

- New workspace member `tests/integration` is added. Root `Cargo.toml` gains
  one line in `[workspace].members`.
- Feature `P1-W5-F08` test file lives at
  `tests/integration/test_lsp_bridge.rs` (not inside a `tests/` subdir).
- The existing `tests/integration/.gitkeep` is removed once the real
  `Cargo.toml` + `src/lib.rs` land (no longer needed; the dir has real files).
- Future integration WOs append `[[test]]` entries + test source files; they
  do NOT create new workspace members unless dependency isolation forces it.
- `cargo test --workspace` now compiles the integration crate; slow tests
  inside it must follow DEC-0003's `// SLOW-TEST:` + `#[ignore]` rule if
  their wall-time exceeds the fast-CI budget.

## Anti-laziness notes

- The integration tests in this crate MUST run real code: real fixtures on
  disk (`tests/fixtures/*`), real `KnowledgeGraph::open` on tempdirs, real
  bridge entry points (`persist_diagnostics`, `persist_call_hierarchy_*`).
- The only permissible test double is a local `impl SerenaClient` in the
  integration test file — the `SerenaClient` trait is UCIL's own abstraction
  boundary (see `crates/ucil-lsp-diagnostics/src/diagnostics.rs` lines 301-320,
  rustdoc: *"not a mock of Serena MCP"*). Using a local `impl` of the trait
  is structurally the same pattern already approved for P1-W5-F05 / P1-W5-F06
  (`ScriptedFakeSerenaClient` in `quality_pipeline` / `call_hierarchy`).
- No mocking of `tokio::process::Command`, of `rusqlite::Connection`, of
  `notify`, or of any critical-dep process. Forbidden per root `CLAUDE.md`.

## Revisit trigger

If a single integration-test binary needs its own distinct dependency
profile (e.g., `tokio` with `test-util` feature enabled simultaneously
with a mocking crate), split `ucil-tests-integration` into sub-crates via a
supersedes-this-ADR new ADR. For 17 features that share `tokio` +
`async-trait` + `lsp-types` + `tempfile` + `serde_json` dev-deps, a single
crate is sufficient through Phase 8.
