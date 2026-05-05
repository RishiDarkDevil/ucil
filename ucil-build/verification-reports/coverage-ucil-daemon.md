# Coverage Gate — ucil-daemon

- **Verdict**: PASS (manual measurement; harness bug worked around)
- **Min line coverage**: 85%
- **Min branch coverage**: 75% (n/a — branch instrumentation not enabled on stable toolchain)
- **Generated**: 2026-05-05T18:00:00Z (verifier retry 2 manual run)

## Summary

| Metric  | Value |
|---------|-------|
| Line    | 89.88% (5784 / 6435) — floor 85% (+4.88pp) |
| Region  | 91.69% (8785 / 9581) |
| Function| 88.77% (506 / 570) |
| Branch  | _unavailable (toolchain — branches.count = 0)_ |

## Per-file (this WO's diff targets)

| File | Lines % | Regions % |
|------|---------|-----------|
| `session_manager.rs` (F04 implementation) | **94.61%** (369 / 390) | 94.28% |
| `executor.rs` (DEC-0013 doc-link fix carve-out) | 93.68% | 93.57% |

Both files are well above the 85% line floor; regions also above. The
five `executor.rs` edits per DEC-0013 are doc-only (no executable code
changes), so its line coverage is unchanged from `main`.

## Harness bug (same as WO-0049 / 0050 / 0051 / 0052 retry-1)

`scripts/verify/coverage-gate.sh ucil-daemon 85 75` exits 1 with the
message `error: failed to merge profile data: not found *.profraw files
in /home/rishidarkdevil/Desktop/ucil-wt/WO-0052/target`. Root cause is
the env-prefix from `cargo llvm-cov show-env --export-prefix` not
propagating `-Cinstrument-coverage` to the subprocess `cargo test`
invocation in the same way that `cargo llvm-cov test` does atomically.
On top of that, ucil-daemon's e2e_mcp_stdio integration tests spawn
subprocess children that are killed mid-run, leaving corrupt-header
`.profraw` files that break `llvm-profdata merge` even after the
gate's prune step.

Manual workaround (this verifier session):

```bash
$ cargo llvm-cov clean --workspace
$ cargo llvm-cov --package ucil-daemon --summary-only --json   # leaves profraws
# … some corrupt headers cause merge to fail …
# Prune zero-byte and corrupt-header profraws via llvm-profdata show:
$ for f in target/llvm-cov-target/*.profraw; do
    llvm-profdata show "$f" >/dev/null 2>&1 || rm -f "$f"
  done
$ cargo llvm-cov report --package ucil-daemon --summary-only --json \
    > /tmp/llvm-cov-WO-0052.json
$ jq '.data[0].totals' /tmp/llvm-cov-WO-0052.json
```

Both the `session_manager.rs` file (this WO's diff target) at 94.61%
and the crate aggregate at 89.88% are above the 85% line floor.
Branch coverage is reported as `count: 0` by the toolchain (n/a — no
MC/DC or branch instrumentation on stable Rust); per the gate
script's logic this is treated as "unavailable" rather than 0%.

## Raw JSON

```
{
  "branches": { "count": 0, "covered": 0, "notcovered": 0, "percent": 0 },
  "functions": { "count": 570, "covered": 506, "percent": 88.7719298245614 },
  "instantiations": { "count": 923, "covered": 566, "percent": 61.32177681473456 },
  "lines": { "count": 6435, "covered": 5784, "percent": 89.88344988344988 },
  "mcdc": { "count": 0, "covered": 0, "notcovered": 0, "percent": 0 },
  "regions": { "count": 9581, "covered": 8785, "notcovered": 796, "percent": 91.69189019935288 }
}
```

## Disposition

The coverage-gate script's exit-1 is a **harness bug**, not a
WO-0052-attributable failure. Coverage manually meets the gate. This
matches the disposition the retry-1 verifier (`vrf-def33d94`)
documented at `ucil-build/rejections/WO-0052.md` lines 199-238 and the
identical disposition WO-0049/0050/0051 verifiers used. A separate
harness-fixer escalation should target the script (per DEC-0012).
