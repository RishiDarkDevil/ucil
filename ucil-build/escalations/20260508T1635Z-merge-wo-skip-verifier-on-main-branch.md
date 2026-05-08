---
ts: 2026-05-08T16:35:00Z
phase: 3
session: monitor
trigger: harness-defect-investigation
resolved: false
blocks_loop: false
severity: harness-config
auto_classify: bucket-B-fix-or-bucket-E-halt
requires_planner_action: false
related_to:
  - WO-0079 (graphiti — affected, recovered at 19906a0 cherry-pick)
  - WO-0080 (ruff — affected, recovered at 19906a0 cherry-pick)
  - WO-0082 (test-runner-mcp — NOT affected, merged correctly via 1e0f00e)
  - scripts/run-phase.sh:200-218 (feat branch flip detection)
  - scripts/flip-feature.sh:11 (cd to git toplevel)
  - .claude/agents/verifier.md (step 1: cd into worktree)
---

# Harness defect: verifier flip lands on main when verifier doesn't `cd` into feat worktree

## TL;DR

When the verifier subagent fails to `cd` into the feat worktree before running
`scripts/flip-feature.sh`, the flip commit lands on **main** instead of feat.
Subsequently `scripts/run-phase.sh` (lines 200-218) checks the FEAT branch's
feature-list.json, sees `passes=false`, treats it as "verifier rejected",
falls into the rejection branch, hits the stale-post-merge-redispatch guard
at line 339, and `break`s out of the loop **without ever invoking
merge-wo.sh**. Net effect: the feature flag is `passes=true` on main with
NO IMPLEMENTATION code — exactly the fake-green pattern the master plan's
anti-laziness contract forbids.

WO-0079 (graphiti) and WO-0080 (ruff) hit this defect today; recovery via
manual cherry-pick at commit `19906a0`. WO-0082 (test-runner-mcp) went
through the standard path correctly (verifier flipped on feat, merge-wo.sh
fast-forwarded feat → main at `1e0f00e`).

## Evidence

### 1. Critic + verifier commits for WO-0079/0080 are on main, NOT on feat

```
$ git log --oneline a331188 -1
a331188 chore(critic): WO-0079 graphiti G3 plugin manifest critic CLEAN
$ git log --oneline a331188^ -1
16b6439 feat(scripts): auto-prune merged worktrees post-merge to reclaim disk  ← MAIN
```

The parent of WO-0079's critic commit is the auto-prune wiring commit on
main, not the executor's RFR on feat. Same for the verifier flip:

```
$ git log --oneline 527277a -2
527277a chore(verifier): WO-0079 P3-W9-F10 graphiti G3 plugin manifest PASS
a331188 chore(critic): WO-0079 graphiti G3 plugin manifest critic CLEAN  ← parent on main
```

Compare to WO-0082 (correct path):

```
$ git log --oneline 381073e -2
381073e chore(verifier): WO-0082 P3-W11-F07 test-runner G8 plugin manifest PASS
f3aa92f chore(rfr): WO-0082 ready-for-review marker  ← parent on FEAT branch
```

### 2. Flip commit content is ONLY the report + the flip — no source files

```
$ git show 527277a --stat
 ucil-build/feature-list.json               |   8 +-
 ucil-build/verification-reports/WO-0079.md | 259 +++
```

The verifier session's `flip-feature.sh` write went to main's
`ucil-build/feature-list.json`. The executor's source files
(`plugins/knowledge/graphiti/plugin.toml`,
`scripts/devtools/install-graphiti-mcp.sh`,
`scripts/verify/P3-W9-F10.sh`,
`crates/ucil-daemon/tests/g3_plugin_manifests.rs` additions) were on
the feat branch and **never made it to main** until the manual cherry-
pick at `19906a0`.

### 3. The bypass mechanism in run-phase.sh

`scripts/run-phase.sh:200-218`:

```bash
_FEAT_REF="origin/feat/${WO_ID}-${_WO_SLUG}"
if git rev-parse --verify "$_FEAT_REF" >/dev/null 2>&1; then
  _FEAT_FLIST=$(git show "${_FEAT_REF}:ucil-build/feature-list.json" 2>/dev/null || echo '{}')
else
  _FEAT_FLIST=$(cat ucil-build/feature-list.json)
fi
all_pass=1
for fid in $WO_FEATURES; do
  p=$(printf '%s' "$_FEAT_FLIST" | jq -r --arg id "$fid" '.features[] | select(.id==$id) | .passes' 2>/dev/null)
  [[ "$p" != "true" ]] && all_pass=0
done

if [[ "$all_pass" -eq 1 ]]; then
  # MERGE PATH: this is what should happen
  echo "[run-phase] Step 5/5: merge ${WO_ID} → main"
  scripts/merge-wo.sh "$WO_ID"
  ...
fi

# REJECTION PATH: when verifier flipped on main, the feat branch's
# feature-list still says passes=false → all_pass=0 → fall into rejection
# branch even though main IS verified.
```

Then `run-phase.sh:328-344`:

```bash
_rejection_file="ucil-build/rejections/${WO_ID}.md"
_rcf_file="ucil-build/verification-reports/root-cause-${WO_ID}.md"
_all_features_pass=1
for _fid in $WO_FEATURES; do
  # NOTE: this reads main's feature-list, not feat's
  _p=$(jq -r --arg id "$_fid" '.features[] | select(.id==$id) | .passes' \
       ucil-build/feature-list.json 2>/dev/null)
  [[ "$_p" != "true" ]] && _all_features_pass=0
done
if [[ "$_all_features_pass" -eq 1 ]] \
     && [[ ! -f "$_rejection_file" ]] \
     && [[ ! -f "$_rcf_file" ]]; then
  echo "[run-phase] ${WO_ID}: ... stale post-merge re-dispatch detected, breaking retry loop."
  break  # ← HERE: skips merge-wo.sh entirely
fi
```

**Stuck in this exact path**: feat-branch flist check fails (passes=false),
main flist check passes (passes=true, because verifier wrote there directly),
guard fires, loop breaks, merge-wo.sh never runs.

### 4. Why the verifier was on main: hypothesis

`scripts/flip-feature.sh:11` does `cd "$(git rev-parse --show-toplevel)"`.
This resolves to whichever git repo top the caller is in. If the verifier
subagent's bash session is `cd`'d into the feat worktree
(`../ucil-wt/WO-0079`), the toplevel is the worktree, and the flip writes
to feat. If the bash session is in main (`/home/rishidarkdevil/Desktop/ucil`),
the flip writes to main.

The verifier subagent prompt
(`.claude/agents/verifier.md` step 1) says: "`cd` into the executor's
worktree." But there's no enforcement — if the verifier session forgets
or fails to cd, or cd's out before running flip-feature.sh, the flip
silently lands on main.

## Suggested fixes (Bucket B candidate, < 120 LOC)

### Fix 1 (defensive — recommended): make flip-feature.sh refuse to run on main branch

Add to `scripts/flip-feature.sh` after the cd:

```bash
# Refuse to flip on main — the verifier MUST be in the feat worktree.
# This prevents the silent "verifier on main" bug that lets passes=true
# land without merge-wo.sh running.
_current_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
if [[ "$_current_branch" == "main" ]]; then
  echo "ERROR: flip-feature.sh called from main branch." >&2
  echo "       The verifier must \`cd\` into the feat worktree before flipping." >&2
  echo "       Worktree path is typically ../ucil-wt/<WO-ID>/." >&2
  exit 6
fi
```

This makes the bug fail-loud instead of fail-silent. The verifier session
gets a clear error and either fixes its cd or escalates.

### Fix 2 (belt + braces): post-flip integrity check in run-phase.sh

After the all_pass check (line 217), if `all_pass=0` BUT main's
feature-list shows passes=true for the same WO, this is the verifier-
on-main signature — file an escalation and halt instead of breaking
silently:

```bash
# Detect verifier-on-main bug: feat says fail but main says pass.
if [[ "$all_pass" -eq 0 ]]; then
  _main_all_pass=1
  for fid in $WO_FEATURES; do
    p=$(jq -r --arg id "$fid" '.features[] | select(.id==$id) | .passes' \
        ucil-build/feature-list.json 2>/dev/null)
    [[ "$p" != "true" ]] && _main_all_pass=0
  done
  if [[ "$_main_all_pass" -eq 1 ]]; then
    cat > "ucil-build/escalations/$(date -u +%Y%m%dT%H%MZ)-${WO_ID}-verifier-on-main.md" <<EOF
... escalation body documenting the divergence ...
EOF
    git add ucil-build/escalations/...; git commit ...; git push
    echo "[run-phase] CRITICAL: verifier-on-main divergence detected." >&2
    exit 1
  fi
fi
```

### Fix 3 (simplest, complementary): verifier prompt hardening

Update `.claude/agents/verifier.md` step 1 to be more emphatic:

```
1. **First action, no exceptions**: `cd ../ucil-wt/<WO-ID>/` before any
   other shell command. If that directory doesn't exist, halt with an
   escalation — DO NOT run flip-feature.sh from any other directory
   (flip-feature.sh now refuses to run on main, but defense in depth).
```

## Recovery applied

The 8 missing source files for WO-0079 + WO-0080 have been cherry-picked
from their respective feat branches into main at commit `19906a0`.
P3 = 21/45 now reflects 21 features that ACTUALLY have implementations
on main. Disk freed: 72GB by removing 4 orphaned worktrees + branches.

## What I did NOT do

- Did NOT modify `scripts/flip-feature.sh` or `scripts/run-phase.sh` —
  those are harness-territory and should land via a Bucket-B fix WO with
  proper review (the user asked me to investigate, which is done; the
  fix lands as a separate WO authored by harness-fixer or planner).
- Did NOT modify `.claude/agents/verifier.md` for the same reason.
- Did NOT revert any commits — kept the cherry-pick recovery clean.

## Recommended next action

Triage Bucket-B if the fix is < 120 LOC (Fix 1 alone is 6 LOC + comment;
trivial). Otherwise emit a Bucket-D micro-WO that bundles Fix 1 + Fix 2 +
Fix 3 with integration-test that simulates verifier-on-main and asserts
flip-feature.sh exits 6.

resolved: false
