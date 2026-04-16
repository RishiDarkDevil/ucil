#!/usr/bin/env bash
# Cost-budget helper. Source this from any script that wants to check the
# daily USD spend cap before it lights up a fresh `claude -p` session.
#
# Reads token usage from `~/.claude/projects/<cwd-slug>/*.jsonl` (the native
# Claude Code session logs — same source ccusage reads). No ccusage dependency
# is required; if present it is preferred.
#
# Exported entry points:
#   safe_check_daily_budget         — returns 1 and writes an escalation if
#                                     today's spend >= DAILY_USD_CAP. Cap is
#                                     DISABLED by default (unset / empty / 0);
#                                     set DAILY_USD_CAP=<positive-number> in .env
#                                     to re-enable.
#   emit_cost_snapshot [phase_tag]  — appends one JSONL row with today's totals
#
# Output:
#   ucil-build/telemetry/daily-spend.jsonl
#
# Behaviour on missing ccusage / jq:
#   jq is treated as required (installed by scripts/install-prereqs.sh). If it
#   is missing we log a warning and DEGRADE GRACEFULLY: return 0 (i.e. do NOT
#   block the loop), and skip the JSONL emit. This matches the "never invent
#   data" constraint.

# Source guard: allow multiple sourcing without clobbering.
if [[ -n "${_UCIL_COST_BUDGET_SOURCED:-}" ]]; then
  return 0 2>/dev/null || true
fi
_UCIL_COST_BUDGET_SOURCED=1

# ---- Pricing table (USD per 1M tokens). Keep in lockstep with Anthropic's
#      public pricing. Numbers intentionally conservative — over-estimate
#      slightly rather than under-estimate. See:
#        https://www.anthropic.com/pricing
#
# For a model we don't recognise we fall back to Sonnet-4 pricing (a
# reasonable midpoint). Unknown models are logged once per invocation.
_ucil_cost_model_price() {
  # stdout: "<input_usd_per_mtok> <output_usd_per_mtok> <cache_read_usd_per_mtok> <cache_creation_usd_per_mtok>"
  local model="$1"
  case "$model" in
    *opus-4-7*|*opus-4.7*)             echo "15.00 75.00 1.50 18.75" ;;
    *opus-4-6*|*opus-4.6*)             echo "15.00 75.00 1.50 18.75" ;;
    *opus-4*|*opus*)                   echo "15.00 75.00 1.50 18.75" ;;
    *sonnet-4-6*|*sonnet-4.6*)         echo "3.00 15.00 0.30 3.75" ;;
    *sonnet-4-7*|*sonnet-4.7*)         echo "3.00 15.00 0.30 3.75" ;;
    *sonnet-4*|*sonnet*)               echo "3.00 15.00 0.30 3.75" ;;
    *haiku*)                           echo "0.80 4.00 0.08 1.00" ;;
    *)                                 echo "3.00 15.00 0.30 3.75" ;;
  esac
}

# ---- Resolve the project's session-log directory under ~/.claude/projects/.
# Claude Code encodes the project cwd by replacing "/" with "-" and prefixing
# "-" (so /home/foo/bar -> -home-foo-bar). We trust that convention here.
_ucil_project_session_dir() {
  local cwd
  cwd="$(pwd)"
  # Encode: replace every "/" with "-" (leading slash becomes leading "-").
  local slug="${cwd//\//-}"
  printf '%s/.claude/projects/%s' "$HOME" "$slug"
}

# ---- Compute today's spend (UTC) from all JSONL files under the project session
# dir. Stdout is a single JSON object:
#   {"cost_usd":..., "input":..., "output":..., "cache_read":..., "cache_creation":..., "by_model":{"model":{...}}}
#
# Only considers assistant messages with a usage block (skips meta / queue-op
# rows). Uses the event `timestamp` field (ISO-8601 UTC) and keeps only rows
# whose date prefix matches today's UTC date.
_ucil_today_spend_json() {
  local sess_dir today
  sess_dir="$(_ucil_project_session_dir)"
  today="$(date -u +%Y-%m-%d)"

  if [[ ! -d "$sess_dir" ]]; then
    printf '{"cost_usd":0,"input":0,"output":0,"cache_read":0,"cache_creation":0,"by_model":{},"source":"no-session-dir"}'
    return 0
  fi

  if ! command -v jq >/dev/null 2>&1; then
    printf '{"cost_usd":0,"input":0,"output":0,"cache_read":0,"cache_creation":0,"by_model":{},"source":"jq-missing"}'
    return 0
  fi

  # Aggregate per model, then sum, via jq over a concatenated stream.
  # shellcheck disable=SC2016
  local jq_prog
  read -r -d '' jq_prog <<'JQ' || true
# Input: stream of JSONL events. Select only assistant-message events with
# a usage block, dated today (UTC). Group by model, sum token fields.
select(.type == "assistant")
| select(.message? != null)
| select(.message.usage? != null)
| select((.timestamp // "") | startswith($today))
| {
    model: (.message.model // "unknown"),
    input: (.message.usage.input_tokens // 0),
    output: (.message.usage.output_tokens // 0),
    cache_read: (.message.usage.cache_read_input_tokens // 0),
    cache_creation: (.message.usage.cache_creation_input_tokens // 0)
  }
JQ

  # Per-model aggregation as JSON (model -> {input,output,cache_read,cache_creation}).
  local per_model_json
  per_model_json=$(
    {
      shopt -s nullglob
      for f in "$sess_dir"/*.jsonl; do
        jq -c --arg today "$today" "$jq_prog" "$f" 2>/dev/null || true
      done
      shopt -u nullglob
    } | jq -s '
      group_by(.model) | map({
        key: .[0].model,
        value: {
          input:           (map(.input)          | add // 0),
          output:          (map(.output)         | add // 0),
          cache_read:      (map(.cache_read)     | add // 0),
          cache_creation:  (map(.cache_creation) | add // 0)
        }
      }) | from_entries
    '
  )

  if [[ -z "$per_model_json" || "$per_model_json" == "null" ]]; then
    per_model_json='{}'
  fi

  # Apply the pricing table (bash case) to each model's aggregate.
  local total_cost=0 total_in=0 total_out=0 total_cr=0 total_cc=0
  local by_model_json='{}'
  local models
  models=$(printf '%s' "$per_model_json" | jq -r 'keys[]? // empty')

  while IFS= read -r m; do
    [[ -z "$m" ]] && continue
    local tokens
    tokens=$(printf '%s' "$per_model_json" | jq -c --arg m "$m" '.[$m]')
    local ti to tcr tcc
    ti=$(printf '%s' "$tokens" | jq -r '.input')
    to=$(printf '%s' "$tokens" | jq -r '.output')
    tcr=$(printf '%s' "$tokens" | jq -r '.cache_read')
    tcc=$(printf '%s' "$tokens" | jq -r '.cache_creation')

    local price
    price=$(_ucil_cost_model_price "$m")
    local p_in p_out p_cr p_cc
    read -r p_in p_out p_cr p_cc <<<"$price"

    # cost = sum(tokens_i * price_i / 1_000_000)
    local cost
    cost=$(awk -v ti="$ti" -v to="$to" -v tcr="$tcr" -v tcc="$tcc" \
               -v pi="$p_in" -v po="$p_out" -v pcr="$p_cr" -v pcc="$p_cc" \
               'BEGIN { printf "%.6f", (ti*pi + to*po + tcr*pcr + tcc*pcc) / 1000000.0 }')

    total_cost=$(awk -v a="$total_cost" -v b="$cost" 'BEGIN { printf "%.6f", a+b }')
    total_in=$((total_in + ti))
    total_out=$((total_out + to))
    total_cr=$((total_cr + tcr))
    total_cc=$((total_cc + tcc))

    by_model_json=$(
      printf '%s' "$by_model_json" | jq -c \
        --arg m "$m" --argjson ti "$ti" --argjson to "$to" \
        --argjson tcr "$tcr" --argjson tcc "$tcc" --argjson cost "$cost" \
        '.[$m] = {input:$ti,output:$to,cache_read:$tcr,cache_creation:$tcc,cost_usd:$cost}'
    )
  done <<<"$models"

  jq -nc \
    --argjson cost "$total_cost" \
    --argjson ti "$total_in" \
    --argjson to "$total_out" \
    --argjson tcr "$total_cr" \
    --argjson tcc "$total_cc" \
    --argjson by_model "$by_model_json" \
    '{cost_usd:$cost, input:$ti, output:$to, cache_read:$tcr, cache_creation:$tcc, by_model:$by_model, source:"jsonl"}'
}

# ---- Emit one JSONL row to ucil-build/telemetry/daily-spend.jsonl.
# Args: [phase_tag]   — optional label identifying the phase / snapshot point.
emit_cost_snapshot() {
  local phase_tag="${1:-untagged}"
  local repo_root
  repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
  local out_dir="${repo_root}/ucil-build/telemetry"
  local out_file="${out_dir}/daily-spend.jsonl"
  mkdir -p "$out_dir"

  local ts today
  ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  today="$(date -u +%Y-%m-%d)"

  local spend_json
  spend_json="$(_ucil_today_spend_json)"

  local src
  src=$(printf '%s' "$spend_json" | jq -r '.source // "jsonl"' 2>/dev/null || echo "unknown")

  if [[ "$src" == "jq-missing" ]]; then
    printf '[_cost-budget] WARN: jq missing; cost snapshot skipped\n' >&2
    return 0
  fi

  # Emit a flat row per model (plus one "aggregate" row with daily_total_usd).
  local total_cost
  total_cost=$(printf '%s' "$spend_json" | jq -r '.cost_usd // 0')

  local models
  models=$(printf '%s' "$spend_json" | jq -r '.by_model | keys[]? // empty')
  if [[ -z "$models" ]]; then
    # Still write an aggregate row so tooling can detect "no spend yet today".
    jq -nc \
      --arg ts "$ts" --arg date "$today" --arg phase "$phase_tag" \
      --arg model "none" --argjson it 0 --argjson ot 0 --argjson cost 0 \
      --argjson total "$total_cost" \
      '{ts:$ts,date:$date,phase:$phase,model:$model,input_tokens:$it,output_tokens:$ot,cost_usd:$cost,daily_total_usd:$total}' \
      >>"$out_file"
    return 0
  fi

  while IFS= read -r m; do
    [[ -z "$m" ]] && continue
    local row
    row=$(printf '%s' "$spend_json" | jq -c --arg m "$m" '.by_model[$m]')
    local it ot cost cr cc
    it=$(printf '%s' "$row" | jq -r '.input')
    ot=$(printf '%s' "$row" | jq -r '.output')
    cr=$(printf '%s' "$row" | jq -r '.cache_read')
    cc=$(printf '%s' "$row" | jq -r '.cache_creation')
    cost=$(printf '%s' "$row" | jq -r '.cost_usd')

    jq -nc \
      --arg ts "$ts" --arg date "$today" --arg phase "$phase_tag" \
      --arg model "$m" \
      --argjson it "$it" --argjson ot "$ot" --argjson cr "$cr" --argjson cc "$cc" \
      --argjson cost "$cost" --argjson total "$total_cost" \
      '{ts:$ts,date:$date,phase:$phase,model:$model,
        input_tokens:$it,output_tokens:$ot,
        cache_read_tokens:$cr,cache_creation_tokens:$cc,
        cost_usd:$cost,daily_total_usd:$total}' \
      >>"$out_file"
  done <<<"$models"
}

# ---- Check the daily cap. Returns 1 and writes an escalation if exceeded.
# DAILY_USD_CAP is read from the current environment (populated by .env when
# _load-auth.sh runs, or by the caller).
safe_check_daily_budget() {
  # DAILY_USD_CAP semantics:
  #   unset / empty / "0" / any non-positive value  → cap DISABLED (always return 0)
  #   positive number                                → cap enforced; return 1 if today >= cap
  #
  # Default is disabled (no cap). Set DAILY_USD_CAP=100 in .env to re-enable.
  local cap="${DAILY_USD_CAP:-0}"
  if awk -v c="$cap" 'BEGIN { exit !(c+0 <= 0) }'; then
    # cap <= 0 means disabled — do NOT emit snapshot here, emit_cost_snapshot
    # is the separate entry point for that. Just return "OK, proceed".
    return 0
  fi

  local repo_root
  repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

  local spend_json
  spend_json="$(_ucil_today_spend_json)"

  local src total
  src=$(printf '%s' "$spend_json" | jq -r '.source // "unknown"' 2>/dev/null || echo "unknown")
  total=$(printf '%s' "$spend_json" | jq -r '.cost_usd // 0' 2>/dev/null || echo "0")

  if [[ "$src" == "jq-missing" ]]; then
    printf '[_cost-budget] WARN: jq missing; budget check skipped (returning 0)\n' >&2
    return 0
  fi
  if [[ "$src" == "no-session-dir" ]]; then
    printf '[_cost-budget] INFO: no Claude session dir yet (first run); budget=0\n' >&2
    return 0
  fi

  # Compare as floats via awk; exit with 1 iff total >= cap.
  local exceeded
  exceeded=$(awk -v t="$total" -v c="$cap" 'BEGIN { print (t+0 >= c+0) ? 1 : 0 }')

  if [[ "$exceeded" -eq 1 ]]; then
    local ts esc_file
    ts="$(date -u +%Y%m%d-%H%M)"
    esc_file="${repo_root}/ucil-build/escalations/${ts}-daily-cost-cap-exceeded.md"
    mkdir -p "$(dirname "$esc_file")"
    if [[ ! -f "$esc_file" ]]; then
      cat > "$esc_file" <<EOF
---
timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)
type: daily-cost-cap-exceeded
severity: critical
blocks_loop: true
requires_planner_action: false
---

# Daily USD cost cap exceeded

- Cap (DAILY_USD_CAP): \$${cap}
- Today's spend: \$${total}
- Source: ${src}

The autonomous loop has been halted to prevent runaway spend. To resume:
1. Review today's session usage (\`scripts/cost-summary.sh\` prints a table).
2. Raise \`DAILY_USD_CAP\` in \`.env\` if this was expected, OR wait for the
   UTC date to roll over.
3. Mark this escalation resolved (\`resolved: true\`) and re-run
   \`scripts/run-phase.sh\`.

Latest telemetry row:
\`\`\`
$(tail -1 "${repo_root}/ucil-build/telemetry/daily-spend.jsonl" 2>/dev/null || echo '(no telemetry yet)')
\`\`\`
EOF
    fi
    printf '[_cost-budget] HALT: daily cost cap $%s exceeded (spent $%s)\n' "$cap" "$total" >&2
    return 1
  fi

  return 0
}

# End of file.
