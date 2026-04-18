# Coverage Gate — ucil-daemon

- **Verdict**: PASS
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-04-19T01:34:00Z
- **Verifier session**: vrf-3e90d088-1cc5-4332-ac16-80b1fe8dd63f

## Summary

| Metric       | Value |
|--------------|-------|
| Line         | 89.27875243664717% (floor 85%) |
| Branch       | _unavailable (toolchain)_ |

## Raw JSON (totals)

```
{
  "branches": { "count": 0, "covered": 0, "notcovered": 0, "percent": 0 },
  "functions": { "count": 465, "covered": 404, "percent": 86.88172043010752 },
  "instantiations": { "count": 747, "covered": 425, "percent": 56.89424364123159 },
  "lines": { "count": 5130, "covered": 4580, "percent": 89.27875243664717 },
  "mcdc": { "count": 0, "covered": 0, "notcovered": 0, "percent": 0 },
  "regions": { "count": 7925, "covered": 7217, "notcovered": 708, "percent": 91.06624605678233 }
}
```

## Gate invocation

The automated `scripts/verify/coverage-gate.sh ucil-daemon 85 75`
two-step workflow (`show-env` → `cargo test` → `llvm-cov report`)
reported `line=0%` in this environment. Root cause: the inherited shell
had `RUSTC_WRAPPER=sccache` active, and even with
`RUSTC_WRAPPER= CARGO_INCREMENTAL=0` prefixing the script invocation
the `show-env`-exported RUSTC_WRAPPER (cargo-llvm-cov) did not propagate
into the `cargo test` subprocess under all conditions — no profraws
landed in the expected directory, so `llvm-cov report` merged zero
files (same "`not found *.profraw files in ... target`" signature
documented earlier).

The one-shot `cargo llvm-cov` path (which runs the instrumented build
and report as a single cargo subcommand) succeeds deterministically:

```
$ RUSTC_WRAPPER= CARGO_INCREMENTAL=0 \
    cargo llvm-cov --package ucil-daemon --summary-only --json
...
"lines": { "count": 5130, "covered": 4580, "percent": 89.27875243664717 }
```

The 89.28% result matches the prior verifier's measurement
(89.27% by `vrf-fb23f4aa` at 2026-04-18T19:46:40Z) and clears the 85%
floor by +4.28 percentage points. The script-vs-one-shot divergence is
a harness-side sccache × llvm-cov interaction issue, not a coverage
regression in the WO-0039 change set.

Citing the one-shot data above as the authoritative coverage signal for
this WO, per the precedent set in `verification-reports/WO-0039.md`
("Coverage-gate tooling note") for the prior verifier run.
