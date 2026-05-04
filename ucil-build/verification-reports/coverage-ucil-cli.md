# Coverage Gate — ucil-cli

- **Verdict**: PASS
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-05T01:35:00Z
- **Method**: documented `env -u RUSTC_WRAPPER cargo llvm-cov` workaround (carried through WO-0039 retry-1 / WO-0040 / WO-0041 / WO-0042 / WO-0043 / WO-0044). Tracking escalation: `ucil-build/escalations/20260419-0152-monitor-phase1-gate-red-integration-gaps.md`.

## Summary

| Metric    | Value | Floor | Verdict |
|-----------|-------|-------|---------|
| Line      | **86.83%** (910 / 1048) | 85% | PASS (+1.83 pp) |
| Function  | 78.33% (94 / 120)       | n/a | informational |
| Region    | 86.55% (1390 / 1606)    | n/a | informational |
| Branch    | _unavailable (stable toolchain — count=0 sentinel)_ | 75% | skipped per script logic |

## Per-file coverage

| File | Regions | Functions | Lines |
|------|---------|-----------|-------|
| commands/init.rs    | 88.60% | 84.00% | 90.64% |
| commands/plugin.rs  | 87.00% | 78.49% | **87.28%** |
| main.rs             |  0.00% |  0.00% |  0.00% (binary entry — not exercised by `cargo test --tests`) |
| **TOTAL**           | **86.55%** | **78.33%** | **86.83%** |

`commands/plugin.rs` (the file exercised by all five new module-root acceptance tests + five new `mod tests {}` emit-helper unit tests) sits at **87.28% line coverage** (above the 85% floor). The handler set added by this WO (`list_plugins`, `uninstall_plugin`, `enable_plugin`, `disable_plugin`, `reload_plugin`) plus the `read_state` / `write_state` / `mutate_state` helpers and the new `*Args` / `*Outcome` types are all covered by:

- 5 `commands::plugin::test_<subcommand>_*` module-root tests (real subprocess for `reload`; real tempdirs + tempfile-then-rename for state file).
- 5 `commands::plugin::tests::*_emits_*` emit-helper unit tests.
- The pre-existing `test_plugin_install_resolves_manifest_by_name` regression guard.
- The end-to-end `scripts/verify/P2-W6-F07.sh` driver.

## Why the canonical `scripts/verify/coverage-gate.sh` exits 1

In the verifier's environment:
1. `RUSTC_WRAPPER=sccache` (set globally in this workstation's shell) intercepts the rustc invocation in a way that prevents llvm-cov's instrumentation from reaching every test binary.
2. The new `test_plugin_reload_runs_health_check` and pre-existing `test_plugin_install_resolves_manifest_by_name` tests spawn the real `mock-mcp-plugin` binary as a subprocess. The mock receives a SIGTERM/SIGKILL when the parent test ends, so its `.profraw` payload is sometimes truncated.
3. Additionally, `cargo llvm-cov --package ucil-cli` only builds ucil-cli's tests — not `ucil-daemon`'s `mock-mcp-plugin` binary. Without an explicit pre-build into the llvm-cov target dir, those two tests fail with `expected mock-mcp-plugin binary at target/llvm-cov-target/debug/mock-mcp-plugin`.
4. The script's prune step (`llvm-profdata show`) catches most corrupt files but the merge step still aborts under either #2 or #3 above.

Workaround applied (same one used by WO-0039 retry-1 / WO-0040 / WO-0041 / WO-0042 / WO-0043 / WO-0044 verifiers, with one additional pre-build step for the subprocess binary):

```bash
env -u RUSTC_WRAPPER cargo llvm-cov clean --workspace
CARGO_TARGET_DIR=$(pwd)/target/llvm-cov-target \
  env -u RUSTC_WRAPPER cargo build -p ucil-daemon --bin mock-mcp-plugin
env -u RUSTC_WRAPPER cargo llvm-cov --package ucil-cli --no-report
LLVM_PROFDATA="$(rustc --print target-libdir)/../bin/llvm-profdata"
find target/llvm-cov-target -name '*.profraw' -size 0 -delete
for f in $(find target/llvm-cov-target -name '*.profraw'); do
  "$LLVM_PROFDATA" show "$f" >/dev/null 2>&1 || rm -f "$f"
done
env -u RUSTC_WRAPPER cargo llvm-cov report --package ucil-cli --summary-only --json
```

Numbers above are produced by that workaround.

## Raw JSON

```json
{
  "branches": { "count": 0, "covered": 0, "notcovered": 0, "percent": 0 },
  "functions": { "count": 120, "covered": 94, "percent": 78.33 },
  "instantiations": { "count": 498, "covered": 209, "percent": 41.97 },
  "lines": { "count": 1048, "covered": 910, "percent": 86.83 },
  "regions": { "count": 1606, "covered": 1390, "notcovered": 216, "percent": 86.55 }
}
```
