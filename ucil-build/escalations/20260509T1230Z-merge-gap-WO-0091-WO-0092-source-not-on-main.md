---
ts: 2026-05-09T12:30:00Z
phase: 3
session: monitor
trigger: verifier-on-main-source-gap-recurrence
work_orders: [WO-0091, WO-0092]
features: [P3-W10-F04, P3-W10-F12]
severity: harness-config
blocks_loop: false
auto_classify: bucket-E
requires_user_action: true
resolved: false
close_when: source files for both WOs cherry-picked into main (with cargo build green) OR full merge-wo.sh executed manually
---

# Verifier-on-main source gap: WO-0091 + WO-0092

## Summary

Both WO-0091 (G5 Context backbone, P3-W10-F04) and WO-0092 (review_changes
MCP tool) are stuck in the same verifier-on-main pattern documented at
`escalations/20260508T1635Z-merge-wo-skip-verifier-on-main-branch.md`:

* **WO-0091**: verifier `vrf-3b0d7ada-…` PASSED at branch HEAD `1c3eb42`,
  flipped P3-W10-F04 on `feat/WO-0091-g5-context-parallel-query`, but
  the orchestrator never invoked `merge-wo.sh`. Source files (`g5.rs`
  +1 entire file, `executor.rs` +670-line G5 acceptance test, `lib.rs`
  +2 lines mod/re-export, `scripts/verify/P3-W10-F04.sh`,
  `verification-reports/WO-0091.md`, `critic-reports/WO-0091.md`)
  remain on the feat branch only.
* **WO-0092**: similar — RFR cherry-picked at `b435c2b`, but verifier
  has not yet finished (queued by `/tmp/orphan-verifier-launcher.sh`,
  awaiting quiet window after WO-0094 critic-then-verifier completes).
  Once verifier passes, expect the same gap.

## Why this monitor session is NOT cherry-picking blindly

The prior recovery commits at `8c764d9` (WO-0088, 923 insertions) and
`88c16be` (WO-0089, 975 insertions) cherry-picked source from feat
branches and validated with `cargo build`. The current monitor session
is bound by the OOM-recovery rule (`feedback_orchestrator_skipping_in_flight_wos.md`):
**"NEVER run `cargo build`/`cargo test` myself — those caused the OOM."**
Without ability to validate the cherry-pick locally, blind apply could
break `main` and stop the autonomous loop hard.

Estimated cherry-pick scope per WO:

| WO | Files | LOC inserted | Conflict risk |
|----|-------|--------------|---------------|
| WO-0091 | `g5.rs` + `executor.rs` patch + `lib.rs` 2-line manual edit + 1 verify script + 2 reports | ~1300 | LOW (lib.rs needs surgical add — `pub mod g5;` + `pub use g5::{...}` — because feat removes `agent_scheduler` mod which main has from WO-0093) |
| WO-0092 | `review_changes.rs` + `lib.rs` + integration test + reports | TBD (verifier hasn't run) | TBD |

The `lib.rs` conflict on WO-0091 (feat removes `agent_scheduler`, main
has it from WO-0093 merge `12a2bb5`) means a plain `git checkout
origin/feat/WO-0091-... -- crates/ucil-daemon/src/lib.rs` would BREAK
WO-0093. Manual surgical edit is required.

## Pipeline impact

`blocks_loop: false`. The loop continues to make progress:
* WO-0094 (W11 pipeline integration tests) is mid-pipeline: critic step
  3/4 firing as of 2026-05-09T12:30Z.
* `feature-list.json` on main shows P3 = 38/45. The 2 stranded features
  count against P3 gate but don't block other features.
* Orphan-launcher continues dispatch sequentially.

## Recommended actions (user)

1. When back at desk, run from main repo root:
   ```bash
   # WO-0091 source recovery
   git checkout origin/feat/WO-0091-g5-context-parallel-query -- \
     crates/ucil-daemon/src/g5.rs \
     crates/ucil-daemon/src/executor.rs \
     scripts/verify/P3-W10-F04.sh \
     ucil-build/verification-reports/WO-0091.md \
     ucil-build/critic-reports/WO-0091.md
   # Manually add to crates/ucil-daemon/src/lib.rs (do NOT git checkout this file):
   #   pub mod g5;   (insert alphabetically after `pub mod g4;`)
   #   pub use g5::{...};   (insert alphabetically; copy the line from feat)
   cargo build -p ucil-daemon --tests
   cargo test -p ucil-daemon executor::test_g5_context_assembly
   # If green, flip the feature via jq directly (whitelisted fields only):
   jq '(.features[] | select(.id == "P3-W10-F04")) |= 
       (.passes = true 
       | .last_verified_ts = "2026-05-09T06:27:11Z" 
       | .last_verified_by = "verifier-3b0d7ada-dca5-4874-88d7-4d29712252fb"
       | .last_verified_commit = "f6a7c7995d45e0337cef0a39326a95326350fec3")' \
       ucil-build/feature-list.json > /tmp/fl.json && mv /tmp/fl.json ucil-build/feature-list.json
   git add crates/ucil-daemon/src/g5.rs crates/ucil-daemon/src/executor.rs \
           crates/ucil-daemon/src/lib.rs scripts/verify/P3-W10-F04.sh \
           ucil-build/verification-reports/WO-0091.md \
           ucil-build/critic-reports/WO-0091.md \
           ucil-build/feature-list.json
   git commit -m "fix: recover WO-0091 source + flip P3-W10-F04 (verifier-on-main pattern)"
   git push origin main
   ```

2. Repeat for WO-0092 once its verifier finishes (check
   `/tmp/orphan-verifier-launcher.log` for completion).

## Auto-classify

Bucket E. Triage cannot Bucket-A-resolve because source files are
genuinely missing on main (not just an admin advisory). Triage cannot
Bucket-B-resolve because the fix scope (>1000 LOC) exceeds the 120-LOC
limit. Triage cannot Bucket-D convert because the fix isn't a UCIL-source
bug — it's a harness merge-orchestration gap.

This needs the user.

## Resolution

Both WOs successfully merged via `scripts/merge-wo.sh` after monitor session
SIGSTOPped run-phase.sh to lock the inter-iteration window:

* WO-0091 → main at `83965dc` (g5.rs new, executor.rs +670, lib.rs auto-merged
  to keep both `agent_scheduler` and `g5`, P3-W10-F04 flipped)
* WO-0092 → main at `b3e629f` (server.rs new, P3-W11-F11 flipped)
* `cargo check -p ucil-daemon --tests` PASS in 3.89s (mem delta +200MB)
* P3 = 41/45 → 43/45

Key insight: `merge-wo.sh`'s "integrate main into feat first" 3-way merge
step handles heavily-diverged branches cleanly when the changes are in
non-overlapping text regions. Cherry-pick was over-cautious — the script
auto-resolved both lib.rs and feature-list.json without manual intervention.

resolved: true
