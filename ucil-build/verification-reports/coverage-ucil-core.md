# Coverage Report: `ucil-core` (post-WO-0096)

**Verifier session**: vrf-7d3f5ae3-567e-49ce-bd18-8f82cd831541
**Branch**: `feat/WO-0096-feedback-loop-post-hoc-analyser`
**HEAD**: `4cfcaa4121fd284d76431c68e7c4df4399f7c6df`
**Verified at**: 2026-05-09T14:52:00Z
**Floor**: line ≥ 85%, branch ≥ 75% (per AC34 standing protocol; the
`coverage-gate.sh` script-level entry is known-broken and exits 0 with
a spurious `cargo llvm-cov report errored` line — the
`env -u RUSTC_WRAPPER cargo llvm-cov` direct form below is the
authoritative measurement, 45+ WOs deep carry-forward).
**Verdict**: **PASS**

## Per-file (cargo-llvm-cov direct form, new module + totals)

```
$ env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-core --tests --summary-only
Filename                      Regions    Missed Regions     Cover   Functions  Missed Functions  Executed       Lines      Missed Lines     Cover    Branches   Missed Branches     Cover
-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
feedback.rs                       352                50    85.80%          13                 2    84.62%         317                39    87.70%           0                 0         -
TOTAL                            5254               308    94.14%         250                15    94.00%        3794               158    95.84%           0                 0         -
```

## Totals (machine-readable)

```
$ env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-core --tests --summary-only --json | jq '.data[0].totals.lines.percent'
95.83552978386926
```

## Verdict

- Total `ucil-core` line coverage **95.84% ≥ 85%** ✓
- New module `feedback.rs` line coverage **87.70% ≥ 85%** ✓
- New module `feedback.rs` region coverage **85.80%**
- New module `feedback.rs` function coverage **84.62%** (no per-file functions floor; the `Default` impls and the `FeedbackError` variants account for the 2 unexecuted of 13)
- Branch instrumentation not enabled — N/A per the AC25 / AC34 standing carve-out.

The new module clears the 85% line-coverage floor without ceremonial assertions; the 8-SA frozen test exercises the 5 §8.7 dispatch arms + fast-path + ordering + timestamp injection. The `FeedbackPersistence` trait surface is exercised in-process via the `#[cfg(test)] TestFeedbackPersistence` impl per WO-0085/0088/0093 precedent.

## Coverage-gate.sh sccache RUSTC_WRAPPER carry-forward

`scripts/verify/coverage-gate.sh ucil-core 85 75` exits 0 with body
`[FAIL] cargo llvm-cov report errored for ucil-core` due to the sccache
`RUSTC_WRAPPER` interaction (45+ WOs deep carry-forward). This report
overwrites the gate-script stub with the authoritative env-stripped
measurement.
