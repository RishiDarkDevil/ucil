# WO-0054 superseded by WO-0058

**Status**: SUPERSEDED
**Superseded at**: 2026-05-06T23:15:00Z
**Superseded by**: WO-0058 (`0058-ucil-embeddings-onnx-session.json`)

## Reason

WO-0054 was emitted on 2026-05-05T20:37:23Z (commit `92e76a3`) targeting
P2-W8-F01 (`ucil-embeddings` ORT session loader). No executor session ever
created the `feat/WO-0054-ucil-embeddings-ort-session` branch (verified
via `git branch -a | grep WO-0054` returning empty, both local and
remote). No commits, no progress on `main`, no `ucil-build/work-orders/0054-ready-for-review.md`
marker. The crate `crates/ucil-embeddings/` on `main` is still the
7-line `lib.rs` skeleton + 14-line `Cargo.toml` from WO-0001 — proving
WO-0054's plan never reached an executor.

The feature `P2-W8-F01` in `ucil-build/feature-list.json` shows
`passes: false`, `attempts: 0`, `last_verified_by: null` — confirming
no verifier ever ran against any WO-0054 output.

## Critical correction in WO-0058

WO-0054's plan named the new module `ort_session.rs` and the test
`test_ort_session_loads_minimal_model`. However, the FROZEN
acceptance-test selector in `feature-list.json` is:

```
-p ucil-embeddings onnx_inference::
```

Per the root `CLAUDE.md` ("Edit `id`, `description`, `acceptance_tests`,
or `dependencies` in `feature-list.json` — those are frozen") this
selector is immutable. Any executor following WO-0054 verbatim would
have shipped `ort_session::test_ort_session_loads_minimal_model`, which
would NOT match the frozen selector substring `onnx_inference::`, and
the verifier's `cargo test -p ucil-embeddings onnx_inference::` would
report `0 tests passed; no tests matched`.

**WO-0058 corrects the module file to `onnx_inference.rs`** so the
test selector resolves correctly. All other architectural decisions
(error-type design, `OrtSession` → renamed `OnnxSession` for symmetry
with the module name, sync API surface, single-batch ndarray wrapper,
minimal.onnx fixture under `crates/ucil-embeddings/tests/data/`) are
preserved verbatim from WO-0054 with cumulative-discipline streak
counts updated through WO-0056.

## Disposition

- WO-0054's JSON file (`0054-ucil-embeddings-ort-session.json`) is
  retained on disk for archival; it is NO LONGER an active work-order.
- The next executor session targeting P2-W8-F01 MUST use WO-0058,
  NOT WO-0054.
- This pattern mirrors `0050-superseded-by-WO-0056.md` (the abandoned
  G2 RRF fusion WO that was redone as WO-0056).

## Cross-references

- `ucil-build/work-orders/0058-ucil-embeddings-onnx-session.json` —
  the active redo WO.
- `ucil-build/work-orders/0050-superseded-by-WO-0056.md` — precedent
  for the supersede marker format.
- `feature-list.json:P2-W8-F01.acceptance_tests[0].selector` —
  the frozen selector that drove the module-name correction.
