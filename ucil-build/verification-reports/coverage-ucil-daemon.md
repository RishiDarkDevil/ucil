# Coverage Gate — ucil-daemon

- **Verdict**: PASS (via documented one-shot workaround)
- **Min line coverage**: 85%
- **Min branch coverage**: 75% (n/a on stable toolchain)
- **Generated**: 2026-05-04T18:05:00Z
- **Verifier**: vrf-WO-0042-2026-05-04
- **WO**: WO-0042

## Summary

| Metric        | Value | Floor | Verdict |
|---------------|-------|-------|---------|
| Line          | 89.68% | 85%  | PASS (+4.68 pp) |
| Function      | 88.47% | n/a  | -       |
| Region        | 91.70% | n/a  | -       |
| Branch        | _unavailable (stable toolchain — count=0)_ | 75% | n/a |

## Per-file coverage of WO-0042's primary source file

`crates/ucil-daemon/src/plugin_manager.rs` (the only source file changed
by this WO):

| Metric        | Value |
|---------------|-------|
| Lines         | 87.71% (664 / 757) |
| Functions     | 88.06% (59 / 67) |
| Regions       | 90.15% (924 / 1025) |
| Instantiations| 61.67% (74 / 120) |

The new code (CapabilitiesSection, ResourcesSection, ActivationSection,
PluginManifest::validate, activates_for_language, activates_for_tool,
PluginRuntime::register, mark_loading, mark_active, stop, mark_error,
PluginError::InvalidManifest, PluginError::IllegalTransition) is
exercised by the two new module-root acceptance tests
(`test_manifest_parser`, `test_lifecycle_state_machine`) and by the
preserved `test_hot_cold_lifecycle` regression guard.

## Method (workaround)

`scripts/verify/coverage-gate.sh` exits 1 in the current environment
because `RUSTC_WRAPPER=sccache` prevents instrumentation from reaching
every test binary (lines.count=249 vs. the crate's real ≈5,686
instrumented lines), and detached child processes write 472-byte
corrupt profraw files that the script's zero-byte prune misses. This is
the **same pre-existing harness-infrastructure bug** documented in:

- WO-0039 retry-1 verifier report (2026-04-19T01:34Z)
- WO-0040 verifier PASS verdict (2026-04-19T02:30Z)
- WO-0041 verifier PASS verdict (2026-04-19T03:25Z)
- escalation `ucil-build/escalations/20260419-0152-monitor-phase1-gate-red-integration-gaps.md`

The one-shot workaround used by every recent verifier:

```
cargo llvm-cov clean --workspace
env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --summary-only --json
```

## Raw JSON totals

```
{
  "branches":       {"count": 0,    "covered": 0,    "percent": 0},
  "functions":      {"count": 503,  "covered": 445,  "percent": 88.47},
  "instantiations": {"count": 809,  "covered": 500,  "percent": 61.80},
  "lines":          {"count": 5686, "covered": 5099, "percent": 89.68},
  "mcdc":           {"count": 0,    "covered": 0,    "percent": 0},
  "regions":        {"count": 8673, "covered": 7953, "percent": 91.70}
}
```

## Verdict

**PASS** at 89.68% line coverage on the workaround path (+4.68 pp above
the 85% floor). Branch coverage is unavailable on the stable toolchain
(count=0 sentinel, treated as n/a per coverage-gate.sh logic).
