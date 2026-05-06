# WO-0057 superseded by WO-0063

**Date**: 2026-05-07
**Reason**: WO-0057 (P2-W7-F06 search_code G2 fused, emitted 2026-05-06T22:30Z)
sat unexecuted for ~30 hours while the autonomous loop pivoted to the W8
embedding track (WO-0058 → WO-0062, all merged). During that interval, five
new lessons-learned blocks landed in `phase-log/02-phase-2/CLAUDE.md` carrying
W8-tier discipline that materially affects this WO's emit shape:

1. **WO-0059 lesson 607** — pre-flight ≤70-char commit-subject validation at
   planner-emit time (post-push `--amend` is forbidden, so an oversize subject
   prescribed in `scope_in[N]` becomes structurally unfixable).
2. **WO-0061 lesson 696** — generic "ban-words pre-flight" item for any WO
   with case-insensitive AC greps on `mock|fake|stub|fixture` against `.rs`
   files. Three consecutive WOs (WO-0058 / WO-0059 / WO-0061) ate post-push
   docs-cleanup commits for benign rustdoc prose. WO-0063 should pre-emptively
   warn the executor.
3. **WO-0060 lesson 644** — upstream-API research checklist for any WO that
   first-time-consumes a sibling crate's API (here: `ucil-daemon` first
   consumes the WO-0044 `plugins/search/probe/plugin.toml` schema and the
   WO-0035 `crate::text_search` substrate via NEW `g2_search.rs` module).
4. **WO-0058 lesson 561** — when calling FFI runtime wrappers (`ort`, `tch`,
   etc.) assume `&mut self` until the crate's `Session::run` signature is
   confirmed. NOT directly relevant to F06 (LancedbProvider is filesystem-only
   per DEC-0015 D3, no `lancedb` crate import) but the `&mut self` warning
   carries to any future tightening of `LancedbProvider::execute`.
5. **DEC-0016** (2026-05-07T02:14Z, AFTER WO-0057's emit) — `feat/WO-0053-
   lancedb-per-branch` orphan-branch state. WO-0057 was emitted before this
   ADR existed; WO-0063 explicitly cross-references DEC-0016 to confirm F06
   does NOT depend on the orphan branch (`StorageLayout::branch_vectors_dir`
   lives at `crates/ucil-daemon/src/storage.rs:230` on `main`).

## Substance preservation

WO-0063 PRESERVES all 15 `scope_in` items and all 18 `acceptance_criteria`
from WO-0057 verbatim — the architectural decisions (DEC-0008 trait
ownership, DEC-0009 in-process ripgrep, DEC-0015 three-decision triplet,
DEC-0016 orphan-branch carve-out) and the implementation plan (`g2_search.rs`
module, `G2SourceProvider` trait, three providers, `G2SourceFactory`, MCP
server `with_g2_sources` builder, additive `_meta.g2_fused` field) are
identical. The five new W8-tier discipline items are ADDITIVE
`scope_in[16..20]` entries.

The sole behavioural delta is wording / cross-reference; the executor
reading WO-0063 produces the same source-tree as one reading WO-0057 plus
the W8-tier process discipline (pre-flight grep on commit-subject lengths
and ban-word vocabulary; documented upstream-API research notes in the
ready-for-review marker).

## Fate of the worktree branch

`feat/WO-0057-search-code-g2-fused` exists locally with ONE commit (the
planner emit `280f0fb`). Per the executor's standard workflow, WO-0063's
worktree branch `feat/WO-0063-search-code-g2-fused-refresh` is created
fresh from `main`; the WO-0057 branch is left in place for git-history
auditability and is NOT pushed to origin (it never was).

## Files updated

- `ucil-build/work-orders/0063-search-code-g2-fused-refresh.json` (new)
- `ucil-build/work-orders/0057-superseded-by-WO-0063.md` (this file)
- `ucil-build/escalations/20260507T0750Z-wo-0053-orphan-branch-blocks-w8-f04-f07-f08.md` (new — DEC-0016 prerequisite for any future F04/F07/F08 WO emission)

## References

- `ucil-build/work-orders/0057-search-code-g2-fused.json` (the original,
  preserved as historical artefact)
- `ucil-build/work-orders/0050-superseded-by-WO-0056.md` (precedent — same
  supersession shape)
- `ucil-build/work-orders/0054-superseded-by-WO-0058.md` (precedent — same
  supersession shape)
- `ucil-build/decisions/DEC-0015-search-code-g2-fan-out-and-fused-meta-field.md`
- `ucil-build/decisions/DEC-0016-wo-0053-feat-branch-not-merged.md`
- `ucil-build/phase-log/02-phase-2/CLAUDE.md` lessons WO-0058 → WO-0062
