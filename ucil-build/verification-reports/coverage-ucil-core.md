# Coverage Report: `ucil-core` (post-WO-0088)

**Verifier session**: vrf-4b0b8412-3d68-45ab-b052-f716dfc58557
**Branch**: `feat/WO-0088-response-assembly-and-bonus-context`
**HEAD**: `f154ebb998c6cebac60b1538314703685f8985dc`
**Verified at**: 2026-05-08T22:52:52Z
**Floor**: line ≥ 85%, branch ≥ 75% (per AC25 standing protocol; the
`coverage-gate.sh` script-level entry is known-broken and exits 0 with
a spurious `cargo llvm-cov report errored` line — the
`env -u RUSTC_WRAPPER cargo llvm-cov` direct form below is the
authoritative measurement).
**Verdict**: **PASS**

## Per-file (cargo-llvm-cov direct form)

```
$ env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-core --tests --summary-only
Filename                      Regions    Missed Regions     Cover   Functions  Missed Functions  Executed       Lines      Missed Lines     Cover    Branches   Missed Branches     Cover
-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
bonus_selector.rs                 115                 5    95.65%           6                 0   100.00%          91                 4    95.60%           0                 0         -
ceqp.rs                           294                11    96.26%          13                 1    92.31%         170                 5    97.06%           0                 0         -
context_compiler.rs               640                25    96.09%          24                 1    95.83%         402                12    97.01%           0                 0         -
cross_group.rs                    594                26    95.62%          36                 4    88.89%         524                15    97.14%           0                 0         -
fusion.rs                         451                33    92.68%          21                 4    80.95%         421                21    95.01%           0                 0         -
incremental.rs                    201                 1    99.50%          18                 1    94.44%          93                 1    98.92%           0                 0         -
knowledge_graph.rs               2048               118    94.24%          90                 1    98.89%        1361                33    97.58%           0                 0         -
otel.rs                            29                 0   100.00%           3                 0   100.00%          18                 0   100.00%           0                 0         -
schema_migration.rs               170                 8    95.29%          12                 0   100.00%          99                 4    95.96%           0                 0         -
tier_merger.rs                    205                31    84.88%           6                 1    83.33%         189                24    87.30%           0                 0         -
types.rs                          155                 0   100.00%           8                 0   100.00%         109                 0   100.00%           0                 0         -
-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
TOTAL                            4902               258    94.74%         237                13    94.51%        3477               119    96.58%           0                 0         -
```

JSON-form summary:

```
$ env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-core --tests --summary-only --json | jq '.data[0].totals.lines.percent'
96.57750934713833
```

## Summary

| Metric | Value | Floor | Verdict |
|--------|-------|-------|---------|
| Line coverage | **96.58%** | 85% | **PASS** |
| Region coverage | 94.74% | n/a | informational |
| Function coverage | 94.51% | n/a | informational |
| Branch coverage | n/a (`branches.count == 0` on stable toolchain) | 75% | n/a per toolchain |

## New / extended files (WO-0088 scope)

* **`bonus_selector.rs`** (NEW, 499 LOC): **95.60% lines / 100.00% functions**.
  The 4 missed lines are inside the `AlwaysEmpty` doctest example impl
  + the `BonusContextSource::fetch_bonus` trait-method body invoked
  from the doctest (rendered for documentation but not directly
  exercised by SA1-SA7).
* **`context_compiler.rs`** (EXTENDED, +410 LOC): **97.01% lines /
  95.83% functions**. Post-WO-0087 baseline preserved; the new
  `assemble_response` / `hit_token_estimate` paths fully covered by
  SA1-SA8.

## Notes

* `coverage-gate.sh ucil-core 85 75` script-level run reports
  `cargo llvm-cov report errored` and exits 0 — known-broken script
  that's been documented as a standing carry-forward across 43+ WOs.
  The `env -u RUSTC_WRAPPER cargo llvm-cov` direct invocation above is
  the authoritative measurement; verifier confirms PASS.
* No regression introduced by WO-0088 — every per-file line coverage
  ≥ the post-WO-0087 baseline.
