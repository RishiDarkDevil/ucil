# Coverage Gate — ucil-daemon

- **Verdict**: PASS
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-05T00:50:00Z
- **Method**: documented `env -u RUSTC_WRAPPER cargo llvm-cov` workaround (carried through WO-0039 retry-1 / WO-0040 / WO-0041 / WO-0042 / WO-0043). Tracking escalation: `ucil-build/escalations/20260419-0152-monitor-phase1-gate-red-integration-gaps.md`.

## Summary

| Metric    | Value | Floor | Verdict |
|-----------|-------|-------|---------|
| Line      | **89.51%** (5299 / 5920) | 85% | PASS (+4.51 pp) |
| Function  | 88.24% (465 / 527) | n/a | informational |
| Region    | 91.50% (8192 / 8953) | n/a | informational |
| Branch    | _unavailable (stable toolchain — count=0 sentinel)_ | 75% | skipped per script logic |

## Per-file coverage

| File | Regions | Functions | Lines |
|------|---------|-----------|-------|
| executor.rs | 93.96% | 96.97% | 94.12% |
| lifecycle.rs | 84.38% | 71.43% | 79.71% |
| main.rs | 95.17% | 100.00% | 94.78% |
| plugin_manager.rs | 89.12% | 86.81% | 87.18% |
| priority_queue.rs | 100.00% | 100.00% | 100.00% |
| server.rs | 95.64% | 83.78% | 93.70% |
| session_manager.rs | 92.75% | 93.75% | 93.11% |
| session_ttl.rs | 100.00% | 100.00% | 100.00% |
| startup.rs | 88.05% | 88.89% | 86.39% |
| storage.rs | 96.67% | 92.00% | 95.68% |
| test_support.rs | 94.44% | 100.00% | 88.89% |
| text_search.rs | 94.70% | 100.00% | 90.42% |
| understand_code.rs | 83.05% | 85.11% | 78.94% |
| watcher.rs | 84.01% | 88.10% | 81.24% |
| **TOTAL** | **91.50%** | **88.24%** | **89.51%** |

`plugin_manager.rs` (the file consumed read-only by WO-0044's new tests
via `PluginManifest::from_path` + `PluginManager::health_check_with_timeout`)
sits at **87.18% line coverage**, well above the 85% floor — the new
integration tests at `crates/ucil-daemon/tests/plugin_manifests.rs` add
fresh exercise of the `spawn` → `initialize` → `notifications/initialized`
→ `tools/list` path against two NEW real subprocesses (vs. the
prior-WO mock-only coverage), and both mutation checks confirm the
spawn path is genuinely traversed.

## Why the canonical `scripts/verify/coverage-gate.sh` exits 1

In the verifier's environment:
1. `RUSTC_WRAPPER=sccache` (set globally in this workstation's shell)
   intercepts the rustc invocation in a way that prevents llvm-cov's
   instrumentation from reaching every test binary.
2. The new integration tests at `tests/plugin_manifests.rs` plus the
   prior-WO `tests/e2e_mcp_*.rs` spawn detached child processes
   (`npx -y …`, mock MCP plugin, etc.). Some of those subprocess
   threads are killed before they finish writing their `.profraw`
   payload, so corrupt-header `.profraw` files end up in
   `target/llvm-cov-target/`.
3. The script's prune step uses `llvm-profdata show`, which catches
   most corrupt files but misses the windowed-write race in this
   workload. The merge step then aborts.

Workaround applied (same one used by WO-0039 retry-1 / WO-0040 /
WO-0041 / WO-0042 / WO-0043 verifiers):

```bash
env -u RUSTC_WRAPPER cargo llvm-cov clean --workspace
env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-daemon --no-report
# manual prune of zero-byte + corrupt-header profraw
env -u RUSTC_WRAPPER cargo llvm-cov report --package ucil-daemon --summary-only --json
```

Numbers above are produced by that workaround.

## Raw JSON

```json
{
  "branches": { "count": 0, "covered": 0, "notcovered": 0, "percent": 0 },
  "functions": { "count": 527, "covered": 465, "percent": 88.23529411764706 },
  "instantiations": { "count": 855, "covered": 525, "percent": 61.40350877192983 },
  "lines": { "count": 5920, "covered": 5299, "percent": 89.51013513513513 },
  "mcdc": { "count": 0, "covered": 0, "notcovered": 0, "percent": 0 },
  "regions": { "count": 8953, "covered": 8192, "percent": 91.50284820729364 }
}
```
