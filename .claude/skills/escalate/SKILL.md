---
name: escalate
description: Write an escalation file for the user and halt the autonomous loop. Use when blocked, uncertain, or when a root-cause-finder is needed.
allowed-tools: Bash, Write, Read
---

# /escalate <REASON>

Halt the autonomous loop and page the user.

## Steps

1. Gather context:
   - Current phase, week, active feature IDs
   - Last 5 commits
   - Recent rejections / critic reports
   - Current branch + worktree
2. Write `ucil-build/escalations/YYYYMMDD-HHMM-<slug>.md` with:
   ```markdown
   # Escalation: <short title>

   **When**: <iso-ts>
   **Agent role**: <role>
   **Phase**: <N>
   **Feature(s) affected**: <ids>

   ## Symptoms
   - what's happening

   ## What I tried
   - attempt 1: ...
   - attempt 2: ...

   ## Options
   - A: ...
   - B: ...
   - C: ...

   ## Recommended next step
   <your best guess — user can override>
   ```
3. Commit the escalation, push.
4. Print: "Escalation filed at <path>. User will be paged at session start via UserPromptSubmit hook."

## Arguments

- `$1` — free-text reason (used for the filename slug). Required.

## Notes

- This is the **correct** alternative to stubbing, skipping tests, or guessing. Prefer escalating over taking a shortcut.
- The session-start and user-prompt-submit hooks surface open escalations so the user sees them at their next session.
