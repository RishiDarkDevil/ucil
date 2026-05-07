# Coverage Gate — ucil-daemon

- **Verdict**: PASS (manual measurement; harness bug worked around)
- **Min line coverage**: 85%
- **Min branch coverage**: 75% (n/a — branch instrumentation not enabled on stable toolchain)
- **Generated**: 2026-05-05T22:38:00Z (WO-0055 verifier manual run)

## Summary

| Metric  | Value |
|---------|-------|
| Line    | 89.72% (5997 / 6684) — floor 85% (+4.72pp) |
| Region  | 91.24% (9112 / 9987) |
| Function| 86.44% (529 / 612) |
| Branch  | _unavailable (toolchain — branches.count = 0)_ |

## Per-file (WO-0055 diff targets)

| File | Lines % |
|------|---------|
| `crates/ucil-daemon/src/scip.rs` (NEW)        | 84% (per-file slightly under 85%; crate aggregate above floor — gate is crate-level not per-file) |
| `crates/ucil-daemon/src/executor.rs`          | 93% |
| `crates/ucil-daemon/src/plugin_manager.rs`    | 87% |

## Harness bug (same as WO-0049 / 0050 / 0051 / 0052)

`scripts/verify/coverage-gate.sh ucil-daemon 85 75` exits 1 reporting
line=22%. Root cause: `cargo llvm-cov show-env --export-prefix` fails to
propagate `-Cinstrument-coverage` to the subprocess `cargo test`
invocation in the same way `cargo llvm-cov test` does atomically. On top
of that, ucil-daemon's e2e_mcp_stdio integration tests spawn subprocess
children that are killed mid-run, leaving corrupt-header `.profraw`
files that break `llvm-profdata merge` even after the gate's prune step.

Manual workaround (this verifier session):

```bash
$ cargo llvm-cov clean --workspace
$ cargo llvm-cov --package ucil-daemon --summary-only --json --tests \
    > /tmp/vrf-WO-0055-llvm-cov.json   # corrupts at merge step
# Prune zero-byte and corrupt-header profraws via llvm-profdata show:
$ llvm_profdata=$(rustc --print target-libdir)/../bin/llvm-profdata
$ for f in target/llvm-cov-target/*.profraw; do
    "$llvm_profdata" show "$f" >/dev/null 2>&1 || rm -f "$f"
  done
$ cargo llvm-cov report --package ucil-daemon --summary-only --json \
    > /tmp/vrf-WO-0055-cov.json
$ jq '.data[0].totals' /tmp/vrf-WO-0055-cov.json
```

The crate aggregate at 89.72% is above the 85% line floor. Branch
coverage is reported as `count: 0` by the toolchain (n/a — no MC/DC or
branch instrumentation on stable Rust); per the gate script's own logic
at lines 169-173 this is treated as "unavailable" rather than 0%.

## Raw JSON

```
{
  "branches": { "count": 0, "covered": 0, "notcovered": 0, "percent": 0 },
  "functions": { "count": 612, "covered": 529, "percent": 86.43790849673204 },
  "instantiations": { "count": 1001, "covered": 589, "percent": 58.84115884115884 },
  "lines": { "count": 6684, "covered": 5997, "percent": 89.72172351885098 },
  "mcdc": { "count": 0, "covered": 0, "notcovered": 0, "percent": 0 },
  "regions": { "count": 9987, "covered": 9112, "notcovered": 875, "percent": 91.23861019325122 }
}
```

## Disposition

The coverage-gate script's exit-1 is a **harness bug**, not a
WO-0055-attributable failure. Coverage manually meets the gate. This
matches the disposition the WO-0049/0050/0051/0052 verifiers documented.
A separate harness-fixer escalation tracks the script-level fix per
DEC-0012.
