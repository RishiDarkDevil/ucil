# Coverage Gate — ucil-daemon

- **Verdict**: PASS
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-04T18:45:00Z
- **Method**: `env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon`
  one-shot workaround (the `scripts/verify/coverage-gate.sh` carrier still
  trips on the corrupt-profraw-from-spawned-subprocess
  harness-infrastructure bug — same path as WO-0039 retry-1 / WO-0040 /
  WO-0041 / WO-0042). Documented in escalation
  `ucil-build/escalations/20260419-0152-monitor-phase1-gate-red-integration-gaps.md`.

## Summary

| Metric        | Value |
|---------------|-------|
| Line          | 89.49% (floor 85%) — **+4.49 pp above floor** |
| Function      | 88.24% |
| Region        | 91.50% |
| Branch        | _unavailable on stable Rust toolchain (count=0)_ |
| Instantiation | 61.27% |

## Per-file (WO-0043 source surface)

| File | Regions | Functions | Lines |
|------|---------|-----------|-------|
| `crates/ucil-daemon/src/plugin_manager.rs` (only edited source file) | 89.12% (1163/1305) | 86.81% (79/91) | 87.18% (864/991) |

The plugin_manager.rs file — which carries 100% of WO-0043's net source
delta — is at **87.18% line coverage**, exceeding the 85% line floor by
2.18 pp.

## Raw JSON

```json
{
  "branches": {
    "count": 0,
    "covered": 0,
    "notcovered": 0,
    "percent": 0
  },
  "functions": {
    "count": 527,
    "covered": 465,
    "percent": 88.23529411764706
  },
  "instantiations": {
    "count": 852,
    "covered": 522,
    "percent": 61.267605633802816
  },
  "lines": {
    "count": 5920,
    "covered": 5298,
    "percent": 89.49324324324324
  },
  "regions": {
    "count": 8953,
    "covered": 8192,
    "percent": 91.50005584719089
  }
}
```

## Notes

Branch coverage is a no-op on stable Rust (count=0), so the 75% branch
floor is treated as unavailable per `coverage-gate.sh:169-173` (sentinel
`-1`, not `0%`). Line coverage alone clears the gate.
