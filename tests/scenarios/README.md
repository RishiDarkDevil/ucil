# UCIL effectiveness scenarios

Each file is a single live-usage task the effectiveness-evaluator runs twice
(once via UCIL, once via a baseline grep+Read+LSP agent) and scores via an
LLM-as-judge rubric. Gate scripts for phases 1–7 require at least one
scenario tagged for the phase to exist and pass.

## Format

```yaml
id: <kebab-case-id, unique>
phases: [<list of phase numbers this scenario is valid for>]
fixture: <name of tests/fixtures/<name>/ directory>
requires_tools:
  - <UCIL tool name, e.g. find_definition>
  - ...
task: |
  <natural-language task description>
acceptance_checks:
  - name: <short name>
    cmd: <shell command; must exit 0 when solution is correct>
  - ...
rubric:
  - id: <rubric item id>
    weight: <positive float>
    description: <what the judge scores>
  - ...
max_turns: <integer, default 30>
timeout_seconds: <integer, default 600>
```

## Scenario categories to cover (minimum)

When agents add scenarios during phase work, we want coverage of:

| Category              | Covered by                                | First phase it's required |
|-----------------------|-------------------------------------------|---------------------------|
| Symbol navigation     | `nav-*` scenarios                         | Phase 1                   |
| Cross-file refactor   | `refactor-*` scenarios                    | Phase 2                   |
| Add a feature         | `add-feature-*` scenarios                 | Phase 3                   |
| Debug a known bug     | `debug-*` scenarios                       | Phase 3                   |
| Answer arch question  | `arch-*` scenarios                        | Phase 3                   |
| Review a diff         | `review-*` scenarios                      | Phase 5                   |
| Query runtime/CI data | `runtime-*` scenarios                     | Phase 7                   |
| Multi-language        | `ts-*`, `py-*`, `go-*` scenarios          | Phase 1 onwards           |
| Real OSS repo         | `realrepo-*` scenarios                    | Phase 2                   |

Gate scripts DO NOT check category coverage explicitly — but a Phase-N post-mortem
flagging insufficient coverage is a valid reason to delay phase-ship.

## How UCIL is invoked vs. baseline

**UCIL run**: a child `claude -p` session where `.claude/settings.json` inside the
fixture tempdir contains UCIL's MCP server spec plus its plugin. The task prompt
is given as-is; the agent is expected to discover and use UCIL tools.

**Baseline run**: same child `claude -p` session with a **stripped** settings.json
containing ONLY built-in tools (Read, Write, Grep, Glob, Bash) — no MCP servers,
no custom skills, no hooks. Same task, same model, same timeout.

The evaluator captures both sessions' diffs, tool calls, token counts, and the
pass/fail of the `acceptance_checks` commands.

## How the judge scores

A fresh (`--session-id=$(uuidgen) --no-resume`) Claude session is fed the task,
both solutions, and the rubric. It outputs a strict JSON with per-criterion scores
0–5. The evaluator computes a weighted average per solution and decides the verdict.

## Fixture state

Scenarios reference `tests/fixtures/<name>/`. The evaluator copies the fixture to a
tempdir before each run, so the real fixture is never mutated. If a scenario needs
a specific starting state (e.g. a known bug planted), document it in the task itself
or add a `setup:` key with a shell script that runs inside the tempdir before the task.

## Tagging scenarios to phases

A scenario may be tagged for multiple phases — it runs against each. Useful when a
scenario covers functionality that exists in one phase and is expected to remain
correct as later phases add features. Example: `nav-rust-symbol` is tagged
`[1, 2, 3, 4, 5, 6, 7, 8]` because symbol navigation works from Phase 1 onwards
and must never regress.
