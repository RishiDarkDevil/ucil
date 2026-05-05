# DEC-0014: SCIP follows the CLI → SQLite pipeline pattern, not the WO-0044 stdio-MCP plugin pattern

**Status**: accepted
**Date**: 2026-05-06
**Phase**: 2
**Feature**: `P2-W7-F08`
**Work-order**: `WO-0055`

## Context

`P2-W7-F08` ships SCIP P1 install: "scip-rust and scip CLI produce a cross-repo
symbol index for the fixture rust-project; index loaded into SQLite and queried
via G1." (`ucil-build/feature-list.json`, frozen description.)

`ucil-build/phase-log/02-phase-2/CLAUDE.md` line 398 (WO-0051 lessons-learned
"For planner") predicted:

> F08 (SCIP) is the next P2-W7 plugin WO and is a structural plugin (real MCP
> server, language-specific symbol indexer). It will follow the WO-0044
> health-check pattern, NOT the WO-0051 parse-only pattern.

That prediction conflicts with `ucil-master-plan-v2.1-final.md` §3 line 284,
which classifies SCIP's interface verbatim as **"CLI → SQLite"** — adjacent in
the same comparison table to ast-grep / Serena / probe / ripgrep, all of which
ARE classified separately as MCP-server / CLI-with-MCP-wrapper / in-process
respectively. The master-plan classification is authoritative per the oracle
hierarchy in `CLAUDE.md`; the lesson-learned hint is downstream guidance and
must be superseded.

Concretely:

* SCIP-rust (`scip-rust` binary, sourcegraph/scip-rust) is a one-shot
  language-specific indexer that emits a `index.scip` file (binary protobuf
  payload defined by sourcegraph/scip).  It does NOT speak JSON-RPC over stdio
  and has no `tools/list` advertisement.  It is invoked once per index update,
  not once per query.
* The `scip` CLI (sourcegraph/scip) provides forensic operations
  (`scip print`, `scip stats`, `scip snapshot`, `scip lint`, `scip convert` —
  the last converts SCIP to LSIF, not to SQLite).  None of these is an MCP
  surface.
* The SQLite ingestion path is owned by UCIL itself: the daemon decodes the
  `.scip` protobuf into Rust types, writes rows into a UCIL-owned SQLite
  schema, and exposes a query API.  This mirrors the existing
  `KnowledgeGraph::execute_in_transaction` pattern at
  `crates/ucil-daemon/src/storage.rs` (`rusqlite` already in dev deps via
  `crates/ucil-daemon/Cargo.toml:38`).

The WO-0044 health-check pattern (`PluginManifest::from_path` → `PluginManager::
health_check_with_timeout` → real subprocess JSON-RPC `tools/list`) does not
apply because there is no JSON-RPC stdio MCP server to talk to.

## Decision

`P2-W7-F08` ships SCIP P1 along three coordinated artifact families:

1. **Plugin manifest (metadata-only, in-process pattern per DEC-0009)** at
   `plugins/structural/scip/plugin.toml`.  The manifest documents the CLI
   binaries (`scip-rust`, `scip`), pinned versions, advertised capabilities
   (`navigate.cross-repo`, `references.cross-repo`), supported languages
   (Rust today via scip-rust; broader language coverage as later WOs add
   indexer wrappers), and a leading TOML comment block citing upstream URLs +
   pinned versions per the WO-0044 reproducibility precedent.  No
   `[transport].command` because there is no stdio MCP transport.  Instead a
   new `[indexer]` table records the indexer-binary commands and a new
   `[ingest]` table records the destination SQLite store layout.

2. **Indexer + SQLite ingest + query pipeline** in a NEW
   `crates/ucil-daemon/src/scip.rs` module.  Public surface:
   * `pub async fn index_repo(repo_root: &Path, output_dir: &Path) -> Result<PathBuf, ScipError>`
     — runs `scip-rust index` via `tokio::process::Command`, returns absolute
     path of the produced `index.scip`.
   * `pub async fn load_index_to_sqlite(scip_path: &Path, db_path: &Path) -> Result<usize, ScipError>`
     — decodes the SCIP protobuf via the `scip` Rust crate, writes rows to
     a `scip_symbols(symbol, kind, file_path, start_line, end_line, role)`
     table in SQLite via `rusqlite`, returns row count.
   * `pub async fn query_symbol(db_path: &Path, symbol: &str) -> Result<Vec<ScipReference>, ScipError>`
     — reads back rows for the requested symbol; the `ScipReference` struct
     mirrors the §12.x SQLite schema used elsewhere in the daemon.
   * `pub struct ScipG1Source { db_path: PathBuf }` implements the
     `executor::G1Source` trait (added in WO-0047) so the orchestrator can
     fan SCIP into G1 alongside tree-sitter / Serena / ast-grep / diagnostics.
   * `ScipError` is a `thiserror::Error` enum with `#[non_exhaustive]` per
     `.claude/rules/rust-style.md`.

3. **G1 wiring and authority-rank update** in `crates/ucil-daemon/src/
   executor.rs`:
   * `G1ToolKind` grows a fifth variant `Scip`.  All `match` sites on the
     existing 4-variant enum become exhaustive on 5; the compiler enforces
     completeness.
   * `authority_rank()` (`executor.rs:1300`) gains a `G1ToolKind::Scip => 4`
     arm — SCIP ranks below the four existing sources (Serena=0, TreeSitter=1,
     AstGrep=2, Diagnostics=3) because SCIP is an offline batch indexer, so a
     freshly-indexed Serena/LSP signal beats a stale SCIP entry whenever
     they conflict.  This matches master-plan §22 line 616
     "Source authority as soft guidance: LSP/AST → SCIP → Dep tools → KG → Text".
   * `lib.rs` re-exports `ScipG1Source`, `ScipError`, `ScipReference`,
     `index_repo`, `load_index_to_sqlite`, `query_symbol` per the
     cumulative re-export discipline (now 9 consecutive WOs cleared on
     this).

## Rationale

* **Master-plan classification is authoritative.**  §3 line 284 is the
  spec-frozen interface; the WO-0051 lesson-learned hint is an upstream
  prediction the planner is now superseding with a written ADR.
* **No mocks of critical deps.**  The CLI → SQLite pipeline runs real
  `scip-rust` and real `rusqlite` writes against a `tempfile::TempDir`.
  No mocking of `tokio::process::Command`, no SQLite mock — both are real
  subprocess + real on-disk store, consistent with the anti-laziness
  contract.
* **In-process protobuf decode** parallels DEC-0009's in-process ripgrep
  decision: avoid a `scip convert` external CLI dependency in the hot
  ingest path; use the `scip` Rust crate directly.  The `scip` CLI on
  PATH is still required by the verify script for forensic checks
  (`scip print` to confirm the index is well-formed), but the daemon's
  ingest path does not shell out to it.
* **G1 wiring lands with the indexer** so a future WO does not have to
  re-touch executor.rs and re-derive the authority-rank invariant.  The
  whole feature is one architectural unit per WO-0048 lessons line 348
  ("the planner's WO MUST require `authority_rank` to grow a 5th match arm"
  — explicitly addressed here).
* **Plugin manifest as forward-looking metadata.**  Even though SCIP is
  not stdio-MCP, the `plugins/structural/scip/plugin.toml` file lives in
  the conventional location for `ucil plugin list` discovery (P2-W6-F07
  CLI) and gives operators a uniform way to enumerate / version-check
  installed structural plugins.  The manifest schema additions
  (`[indexer]`, `[ingest]`) are forward-extensible additive tables, not
  schema-breaking changes — the existing `PluginManifest::from_path`
  parser tolerates unknown tables when they are `#[serde(default)]`.

## Consequences

* Adds two workspace dependencies: `scip` (sourcegraph SCIP protobuf Rust
  bindings; verify availability + licence on crates.io as part of the
  WO's research step) and `prost` (transitively required by `scip` for
  protobuf decoding; if `scip` already pulls `prost` as a non-feature dep
  this is a no-op).  If either dep is unavailable or has incompatible
  licensing, the executor MUST escalate via a follow-up ADR before
  implementing.
* `crates/ucil-daemon/Cargo.toml` gets two new `[dependencies]` lines
  (`scip = { workspace = true }`, `prost = { workspace = true }`) and
  potentially `Cargo.toml` workspace-deps grows two entries.
* `G1ToolKind` grows from 4 to 5 variants.  Every existing `match`
  on `G1ToolKind` becomes a 5-arm exhaustive match; the compiler
  enforces completeness.  No silent drift.
* Two new external binary dependencies (`scip-rust`, `scip`) are required
  on the verifier's PATH for `scripts/verify/P2-W7-F08.sh` to pass.
  Operator-actionable install hints land in
  `scripts/devtools/install-scip-rust.sh` + `scripts/devtools/install-scip.sh`
  per the WO-0044 / WO-0051 install-helper precedent.
* Cross-repo symbol queries become available via
  `ScipG1Source::execute(query)` and surface through the existing
  `find_references` MCP tool wired in WO-0049.  Delivery of the
  user-visible cross-repo references behaviour is in this WO.
* SCIP indexes the **fixture** `tests/fixtures/rust-project` only in
  Phase 2 — broader language coverage (TypeScript, Java, Python via
  scip-typescript, scip-java, scip-python) is out of phase, deferred
  to Phase 3+ wrappers under the same `plugins/structural/scip/` family.

## Revisit trigger

Revisit if any of:

1. Sourcegraph publishes a first-party `scip-mcp` server speaking JSON-RPC
   stdio.  At that point the manifest can grow a `[transport]` table and
   the `health_check_with_timeout` pattern can be added without removing
   the CLI ingest path (additive, non-breaking).
2. The `scip` Rust crate on crates.io is unmaintained or its license
   incompatible with UCIL.  Fallback: shell out to `scip print --json`
   and parse the JSON output.
3. SQLite write performance becomes a bottleneck (very large
   monorepos, > 5M lines indexed).  Fallback: bulk-load via `INSERT OR
   REPLACE` batches inside a single transaction (the ingest path already
   uses `execute_in_transaction`; only batch sizing changes).
4. G1 result-fusion benchmarks show authority-rank=4 misranks a
   high-quality SCIP result against a degraded LSP source.  In that
   case authority-rank for `Scip` MAY be re-tuned via a follow-up ADR;
   the fusion algorithm itself does not change.

## Alternatives considered

**Stdio-MCP wrapper around scip-rust.**  Build a small UCIL-owned MCP
server that wraps `scip-rust` and surfaces queries over JSON-RPC stdio.
Rejected: introduces a UCIL-internal MCP server distinct from the
external-binary MCP servers (Serena, ast-grep, probe), creating two
plugin patterns to maintain.  The CLI → SQLite pattern matches the
master-plan classification verbatim and reuses the existing rusqlite
infrastructure.

**Skip SQLite, query the `.scip` protobuf at every G1 invocation.**
Rejected: G1 has a 5-second master deadline (`executor.rs:819`); decoding
a multi-MB protobuf per query would blow the deadline on a non-trivial
repo.  SQLite gives index lookup latency under 100 µs, well within the
G1 per-source 4.5-second budget.

**Use LMDB (already used for tree-sitter tag cache) for SCIP storage.**
Rejected: master-plan §3 line 284 says "SQLite", and SCIP rows are
relational (symbol → many references); SQLite indexed lookup matches
the access pattern naturally.  Reusing LMDB would require hand-rolling
a B-tree index on a key-value store.

## References

* `ucil-master-plan-v2.1-final.md` §3 line 284 (SCIP interface = "CLI →
  SQLite") — frozen classification.
* `ucil-master-plan-v2.1-final.md` §22 line 616 ("Source authority as
  soft guidance: LSP/AST → SCIP → Dep tools → KG → Text") — informs
  authority-rank=4 for `G1ToolKind::Scip`.
* `ucil-master-plan-v2.1-final.md` §28 phase-log "external-deps line"
  — `scip-rust` and `scip` binaries listed as Phase 2 Week 7 install
  prerequisites.
* `ucil-build/phase-log/02-phase-2/CLAUDE.md` line 398 (WO-0051
  prediction) — superseded by this ADR.
* `ucil-build/phase-log/02-phase-2/CLAUDE.md` line 348 (WO-0048
  guidance to extend `G1ToolKind` + `authority_rank` together)
  — applied here verbatim.
* `ucil-build/decisions/DEC-0009-search-code-in-process-ripgrep.md`
  — precedent for in-process protobuf/regex decoding in lieu of
  shelling out to a CLI on the hot path.
* `crates/ucil-daemon/src/executor.rs:862` (`G1ToolKind` enum),
  `:1300` (`authority_rank` table) — extension targets.
* `crates/ucil-daemon/Cargo.toml:38` (`rusqlite` already a workspace
  dep, no new SQL workspace dep needed).
* `crates/ucil-daemon/src/storage.rs` (`KnowledgeGraph::execute_in_transaction`
  pattern — reused for SCIP SQLite writes).
