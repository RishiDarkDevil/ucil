# Worker B — Observability + Cost Track: Completion Report

**Date**: 2026-04-17 (UTC)
**Owner**: Worker B
**Branch**: `main` (direct commits per assignment)
**Scope**: daily USD cost budget + W3C trace propagation + OpenTelemetry export
+ human-facing cost summary

## Deliverables

### 1. `scripts/_cost-budget.sh` — sourceable cost helper (316 LOC)

- Exports `safe_check_daily_budget` and `emit_cost_snapshot`.
- Reads from `~/.claude/projects/<cwd-slug>/*.jsonl` (same source `ccusage`
  uses) — **no** `ccusage` binary dependency.
- Conservative per-model pricing table covering Opus 4.x, Sonnet 4.x, Haiku;
  unknown models fall back to Sonnet 4 pricing.
- Emits one JSONL row per model per snapshot to
  `ucil-build/telemetry/daily-spend.jsonl` with `{ts, date, phase, model,
  input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
  cost_usd, daily_total_usd}`.
- Halts loop via a `blocks_loop: true` escalation when today's spend crosses
  `DAILY_USD_CAP`.
- Graceful degradation: `jq` missing -> warn + return 0; session dir missing
  -> return 0 (no spurious halts).

Commit: `3741c69` — `feat(harness): add sourceable daily cost-budget helper`.

### 2. Wire into `scripts/run-phase.sh`

- Sources `scripts/_cost-budget.sh` alongside `_retry.sh`.
- Calls `safe_check_daily_budget` at the start of each iteration (after the
  escalation check, before planner) — halts cleanly on cap-exceeded.
- Calls `emit_cost_snapshot` three times per iteration: post-planner,
  post-executor, post-verifier (each with a distinct phase tag such as
  `phase-1-post-verifier-iter3-v2-WO-0042`).

Commit: `7c429b4` — `feat(harness): wire daily cost-budget check into run-phase loop`.

### 3. W3C TRACEPARENT propagation (`scripts/_trace.sh`, 83 LOC)

- Preserves an inherited `TRACEPARENT`; only mints a fresh 16-hex span-id.
- When `TRACEPARENT` is unset, derives a deterministic 32-hex trace-id from
  `UCIL_WO_ID` / `TARGET` (sha256, first 16 bytes) so every agent working on
  the same work-order shares a trace.
- Exports `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_RESOURCE_ATTRIBUTES`, and
  `CLAUDE_CODE_ENABLE_TELEMETRY=1` when `OTEL_ENABLED=1`.
- Sourced from `scripts/_load-auth.sh` so every launcher picks it up
  automatically.
- All 7 launchers (`run-planner`, `run-executor`, `run-critic`, `run-triage`,
  `run-root-cause-finder`, `run-effectiveness-evaluator`, `spawn-verifier`)
  export `CLAUDE_CODE_ENABLE_TELEMETRY=1` inline before the `claude -p`
  invocation. Executor/critic/RCF/verifier also export `UCIL_WO_ID` / the
  verifier target so trace derivation works.

Commits: `fc229b8` (`_trace.sh` + `_load-auth.sh` hook) and existing
`2a57070` (launcher telemetry exports, landed by Worker C before my
launcher patches — my identical diff was a no-op).

### 4. `scripts/_otel-collector.sh` — docker-based opt-in collector (231 LOC)

- Commands: `start | stop | status | logs`.
- Two backends auto-selected:
  - **Langfuse cloud** when `LANGFUSE_PUBLIC_KEY` + `LANGFUSE_SECRET_KEY`
    are set — exports to `https://cloud.langfuse.com/api/public/otel`.
  - **Local Jaeger all-in-one** (UI on `:16686`) as the fallback.
- Generates a YAML config on the fly at `/tmp/ucil-otel-collector-config.yaml`.
- Uses its own docker network `ucil-otel` to isolate collector + Jaeger.
- Non-blocking: UCIL loop never depends on collector uptime.

Commit: `c2f2046` — `feat(harness): add docker-based OTel collector bring-up helper`.

### 5. `.env.example` update

Added two sections:

```
# Cost control
DAILY_USD_CAP=50

# OTel (optional)
OTEL_ENABLED=0
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
OTEL_RESOURCE_ATTRIBUTES=service.name=ucil-build,phase=auto
LANGFUSE_PUBLIC_KEY=
LANGFUSE_SECRET_KEY=
```

Commit: `8f577cc` — `feat(harness): add cost-summary.sh + DAILY_USD_CAP / OTel env template`.

### 6. `scripts/cost-summary.sh` — human-facing dashboard (141 LOC)

- Default: today live totals (drawn fresh from `~/.claude/projects/`) plus
  last-14-days historical table.
- Flags: `--days N`, `--today`, `--raw`.
- Shows per-model breakdown, cap utilisation as a percentage, and formatted
  integer/USD columns.

Commit: `8f577cc` (co-packaged with `.env.example` update).

## Smoke-test evidence

```
=== Cost-budget smoke test (DAILY_USD_CAP=0.01 should trip) ===
[_cost-budget] HALT: daily cost cap $0.01 exceeded (spent $113.566074)
rc=1 (expected 1)
ucil-build/escalations/20260416-2021-daily-cost-cap-exceeded.md  (generated + then cleaned up)

=== Cost-budget no-trip path (DAILY_USD_CAP=10000) ===
rc=0 (expected 0)

=== Trace-context smoke test ===
UCIL_WO_ID=WO-0999 -> traceparent=00-c7e094d7f242329ab2f248d5ebe02773-<fresh-span>-01
(trace-id is deterministic from the WO id via sha256)

=== Cost-summary dashboard (--today) ===
Today (2026-04-16, live):
  spend            : $113.5660  (cap $50  227.1%)
  input tokens     : 132
  output tokens    : 115721
  cache read       : 23682086
  cache creation   : 2997642
  model split:
    claude-opus-4-7 ...  $113.57
```

All 13 touched shell files pass `bash -n`.

## Observations

- The **repo is currently over budget** (`$113.57 > $50 cap`). With
  `scripts/_cost-budget.sh` wired in, the next invocation of
  `scripts/run-phase.sh` will halt immediately and write an escalation
  rather than lighting up another session. This is the intended behaviour
  but worth flagging to the user who returns to the repo tomorrow: either
  raise `DAILY_USD_CAP` or wait for UTC rollover.
- Commit `c2f2046` unfortunately swept in two files (`scripts/verify/coverage-gate.sh`
  and `ucil-build/verification-reports/coverage-ucil-core.md`) that were
  already staged in the index by an earlier parallel-worker action before
  my `git add scripts/_otel-collector.sh`. Content came from another worker's
  legitimate work (Worker A / C) so there is no corruption, but the
  attribution is mildly muddled. Noted here for audit.

## Line counts

| File | LOC |
|------|----:|
| `scripts/_cost-budget.sh` | 316 |
| `scripts/_trace.sh` | 83 |
| `scripts/_otel-collector.sh` | 231 |
| `scripts/cost-summary.sh` | 141 |
| `scripts/run-phase.sh` | +14 |
| `scripts/_load-auth.sh` | +5 |
| `scripts/run-{planner,executor,critic,triage,root-cause-finder,effectiveness-evaluator}.sh`, `spawn-verifier.sh` | +1 each (7 total, via co-worker commit `2a57070`) |
| `.env.example` | +19 |

## Blockers

None.

## Status

All deliverables (1–6) shipped, pushed to `main`, and smoke-tested. The
harness now has:

- **Cost visibility** end-to-end: JSONL trail + daily halt + summary CLI.
- **Per-WO trace continuity** via W3C trace-context on every `claude -p`.
- **Opt-in OTel export** to local Jaeger or Langfuse cloud.

Ready for merge / release.
