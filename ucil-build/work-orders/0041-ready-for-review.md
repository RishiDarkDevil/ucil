---
work_order: WO-0041
slug: mcp-stdio-repo-kg-bootstrap
branch: feat/WO-0041-mcp-stdio-repo-kg-bootstrap
final_commit: d870d50
phase: 1
week: 4
---

# WO-0041 — mcp --stdio --repo KG bootstrap — ready for review

## Commit ladder (5 commits)

| sha       | type  | summary                                                       |
|-----------|-------|---------------------------------------------------------------|
| aa6b977   | build | promote `tempfile` to `[dependencies]` for `--repo` code path |
| 2342b47   | feat  | add `--repo` arg parser to `mcp` subcommand in `main.rs`      |
| 098fead   | feat  | add `walk_supported_source_files` helper in `main.rs`         |
| 6531ad3   | feat  | bootstrap `KnowledgeGraph` + `IngestPipeline` when `--repo` present |
| d870d50   | test  | e2e integration — spawn binary with `--repo`, assert real `find_definition` |

## What I verified locally

All 20 acceptance criteria from `ucil-build/work-orders/0041-mcp-stdio-repo-kg-bootstrap.json` passed:

- [x] `cargo build -p ucil-daemon --bin ucil-daemon --quiet` → binary at `./target/debug/ucil-daemon`
- [x] `cargo test -p ucil-daemon --test e2e_mcp_with_kg` → `1 passed; 0 failed`
- [x] `cargo test -p ucil-daemon --test e2e_mcp_stdio` → `1 passed; 0 failed` (WO-0040 regression guard green)
- [x] `cargo test -p ucil-daemon --lib` → `119 passed; 0 failed`
- [x] `cargo test --workspace --no-fail-fast` → no `FAILED` lines
- [x] `cargo clippy -p ucil-daemon --all-targets -- -D warnings` → no `^error`
- [x] `cargo doc -p ucil-daemon --no-deps` → no `^error`, no `^warning: unresolved`
- [x] `cargo fmt --check` → clean
- [x] `crates/ucil-daemon/tests/e2e_mcp_with_kg.rs` exists
- [x] `grep fn e2e_mcp_stdio_with_repo_returns_real_find_definition crates/ucil-daemon/tests/e2e_mcp_with_kg.rs` → present
- [x] `grep CARGO_BIN_EXE_ucil-daemon crates/ucil-daemon/tests/e2e_mcp_with_kg.rs` → present
- [x] `grep "--repo" crates/ucil-daemon/tests/e2e_mcp_with_kg.rs` → present
- [x] `grep "tree-sitter+kg" crates/ucil-daemon/tests/e2e_mcp_with_kg.rs` → present
- [x] `grep with_knowledge_graph crates/ucil-daemon/src/main.rs` → present
- [x] `grep "--repo" crates/ucil-daemon/src/main.rs` → present
- [x] `grep walk_supported_source_files crates/ucil-daemon/src/main.rs` → present
- [x] `grep IngestPipeline crates/ucil-daemon/src/main.rs` → present
- [x] `grep with_writer crates/ucil-daemon/src/main.rs` → present (stderr routing preserved)
- [x] `timeout 3 ./target/debug/ucil-daemon` → 0 bytes on stdout
- [x] `timeout 30 ./target/debug/ucil-daemon mcp --stdio </dev/null` → exit 0 (stub path EOF)

## Manual smoke verification

Running `echo '{initialize}\n{find_definition evaluate}' | ./target/debug/ucil-daemon mcp --stdio --repo ./tests/fixtures/rust-project`
returned a response envelope with:

* `_meta.tool = "find_definition"`
* `_meta.source = "tree-sitter+kg"`
* `_meta.found = true`
* `_meta.file_path = "…/tests/fixtures/rust-project/src/util.rs"`
* `_meta.start_line = 128`
* `_meta.signature = "pub fn evaluate(expr: &Expr) -> Result<Value, EvalError>"`

and the stderr channel carried the `--repo supplied` and `bootstrap complete` tracing
spans — stdout remained pristine JSON-RPC.

## Scope adherence

* No `feature-list.json` edits.
* No forbidden-path touches: only `crates/ucil-daemon/src/main.rs`,
  `crates/ucil-daemon/Cargo.toml`, and the new
  `crates/ucil-daemon/tests/e2e_mcp_with_kg.rs`.
* No docker / Serena / external-LSP dependencies introduced.
* No CLI-framework dep added (hand-rolled `--repo` parser per DEC-0006 precedent).
* Rustdoc addition is two sentences, plain backticks, no intra-doc link shorthand.
* Stderr-only tracing in `mcp` arm preserved (`.with_writer(std::io::stderr)`).

## Next step

Please review + verify + merge. Expected gate impact: phase-1 effectiveness
evaluator's `nav-rust-symbol` scenario unblocks from `skipped_tool_not_ready`
once `.claude/settings.json` registers `mcpServers.ucil` to invoke
`ucil-daemon mcp --stdio --repo <REPO>` (harness-side, separately tracked in
`ucil-build/escalations/20260419-0152-monitor-phase1-gate-red-integration-gaps.md`).
