#!/usr/bin/env bash
# Print a human-readable summary of UCIL's daily spend, tokens in/out, and
# model split. Reads ucil-build/telemetry/daily-spend.jsonl (emitted by
# scripts/_cost-budget.sh::emit_cost_snapshot) and optionally also computes
# today's live total straight from ~/.claude/projects/ (whether or not a
# snapshot row has been written yet).
#
# Usage:
#   scripts/cost-summary.sh             # default: last 14 days + today live
#   scripts/cost-summary.sh --days 30   # change the window
#   scripts/cost-summary.sh --today     # only today
#   scripts/cost-summary.sh --raw       # emit the raw JSONL rows
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

DAYS=14
MODE="default"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --days)  DAYS="${2:-14}"; shift 2 ;;
    --today) MODE="today";     shift ;;
    --raw)   MODE="raw";       shift ;;
    -h|--help)
      cat <<EOF
Usage: $0 [--days N] [--today] [--raw]

Summarises ucil-build/telemetry/daily-spend.jsonl and today's live spend.
EOF
      exit 0 ;;
    *) echo "Unknown flag: $1" >&2; exit 2 ;;
  esac
done

TELEMETRY="ucil-build/telemetry/daily-spend.jsonl"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq required for cost-summary.sh" >&2
  exit 3
fi

if [[ "$MODE" == "raw" ]]; then
  if [[ -f "$TELEMETRY" ]]; then
    cat "$TELEMETRY"
  fi
  exit 0
fi

# ---- Live today totals (computed fresh from ~/.claude/projects).
# Source the cost-budget helper and call _ucil_today_spend_json.
if [[ -f scripts/_cost-budget.sh ]]; then
  # shellcheck source=scripts/_cost-budget.sh
  source scripts/_cost-budget.sh
  TODAY_JSON="$(_ucil_today_spend_json)"
else
  TODAY_JSON='{"cost_usd":0,"input":0,"output":0,"cache_read":0,"cache_creation":0,"by_model":{}}'
fi

TODAY_DATE="$(date -u +%Y-%m-%d)"
CAP="${DAILY_USD_CAP:-50}"

# Load any existing .env so DAILY_USD_CAP overrides the default.
if [[ -f .env ]]; then
  # shellcheck disable=SC1091
  set -a; source .env; set +a
  CAP="${DAILY_USD_CAP:-50}"
fi

printf '\n== UCIL cost summary — window: last %s days (UTC) ==\n\n' "$DAYS"

# ---- Live today breakdown (from jsonl source, not from telemetry snapshot).
live_cost=$(printf '%s' "$TODAY_JSON" | jq -r '.cost_usd // 0')
live_in=$(printf '%s' "$TODAY_JSON"   | jq -r '.input // 0')
live_out=$(printf '%s' "$TODAY_JSON"  | jq -r '.output // 0')
live_cr=$(printf '%s' "$TODAY_JSON"   | jq -r '.cache_read // 0')
live_cc=$(printf '%s' "$TODAY_JSON"   | jq -r '.cache_creation // 0')

pct=$(awk -v c="$live_cost" -v cap="$CAP" 'BEGIN { if (cap+0 == 0) {print "n/a"} else {printf "%.1f", 100.0 * c / cap} }')

printf 'Today (%s, live):\n' "$TODAY_DATE"
printf '  spend            : $%.4f  (cap $%s  %s%%)\n' "$live_cost" "$CAP" "$pct"
printf '  input tokens     : %d\n' "$live_in"
printf '  output tokens    : %d\n' "$live_out"
printf '  cache read       : %d\n' "$live_cr"
printf '  cache creation   : %d\n' "$live_cc"

# ---- Per-model breakdown today.
if [[ "$(printf '%s' "$TODAY_JSON" | jq -r '.by_model | length')" -gt 0 ]]; then
  printf '\n  model split:\n'
  printf '    %-30s %12s %12s %12s %10s\n' "model" "input" "output" "cache_read" "cost_usd"
  printf '    %-30s %12s %12s %12s %10s\n' "------" "------" "------" "----------" "--------"
  printf '%s' "$TODAY_JSON" | jq -r '
    .by_model | to_entries[]
    | "    \(.key|.[0:30] + (if (.|length) > 30 then "…" else "" end) | .[0:30])"
      + " \(.value.input          | tostring | ( "            " + .)[-12:])"
      + " \(.value.output         | tostring | ( "            " + .)[-12:])"
      + " \(.value.cache_read     | tostring | ( "            " + .)[-12:])"
      + " $\(.value.cost_usd      | tonumber | . * 10000 | round / 10000 | tostring)"
  '
fi

# ---- Historical: walk daily-spend.jsonl, reduce to per-date aggregates.
if [[ -f "$TELEMETRY" && "$MODE" != "today" ]]; then
  printf '\nHistorical (from %s):\n' "$TELEMETRY"
  printf '  %-12s %12s %12s %12s %12s\n' "date" "in" "out" "cache_read" "cost_usd"
  printf '  %-12s %12s %12s %12s %12s\n' "----" "--" "---" "----------" "--------"

  # Per (date,model) we already have daily_total_usd in rows. Reduce to
  # the MAX daily_total_usd seen per date (the latest snapshot of that day).
  # Sum token columns across models for that date by picking the last row
  # per (date,model).
  cutoff=$(date -u -d "-${DAYS} days" +%Y-%m-%d 2>/dev/null || date -u +%Y-%m-%d)

  jq -Rrs --arg cutoff "$cutoff" '
    split("\n") | map(select(length > 0) | fromjson)
    | map(select(.date >= $cutoff))
    | group_by(.date)
    | map({
        date:    .[0].date,
        cost:    (map(.daily_total_usd) | max // 0),
        # Sum tokens per (date,model) last row to avoid double-counting.
        per_model: (
          group_by(.model)
          | map({
              model:         .[0].model,
              input:         (last.input_tokens          // 0),
              output:        (last.output_tokens         // 0),
              cache_read:    (last.cache_read_tokens     // 0)
            })
        )
      })
    | map({
        date:       .date,
        cost:       .cost,
        input:      (.per_model | map(.input)      | add // 0),
        output:     (.per_model | map(.output)     | add // 0),
        cache_read: (.per_model | map(.cache_read) | add // 0)
      })
    | sort_by(.date)
    | .[]
    | "  \(.date)"
      + "  \(.input          | tostring      | ("            " + .)[-10:])"
      + "  \(.output         | tostring      | ("            " + .)[-10:])"
      + "  \(.cache_read     | tostring      | ("            " + .)[-10:])"
      + "  $\(.cost          | . * 10000 | round / 10000 | tostring)"
  ' <"$TELEMETRY" 2>/dev/null || {
    echo "  (no rows in window — check $TELEMETRY)"
  }
fi

printf '\n'
