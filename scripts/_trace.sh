#!/usr/bin/env bash
# W3C trace-context propagation for UCIL launchers.
#
# Source this from any launcher that spawns `claude -p`. It:
#   1. If TRACEPARENT is already set (inherited from a parent), preserves it
#      and only mints a fresh span-id for the child span.
#   2. Else, if UCIL_WO_ID (or TARGET) is set, derives a deterministic 16-byte
#      trace-id from it so every agent working on the same work-order shares
#      a trace. Falls back to a fully random trace-id.
#   3. Exports OTEL_EXPORTER_OTLP_ENDPOINT + OTEL_RESOURCE_ATTRIBUTES from the
#      environment (populated by .env via _load-auth.sh) when OTEL_ENABLED=1.
#
# Reference: https://www.w3.org/TR/trace-context/
#   traceparent: <version>-<trace-id 16 B hex>-<parent-id 8 B hex>-<flags>
#                "00"-32hex-16hex-"01" (sampled)

if [[ -n "${_UCIL_TRACE_SOURCED:-}" ]]; then
  return 0 2>/dev/null || true
fi
_UCIL_TRACE_SOURCED=1

# ---- Hex helpers (no python/openssl dependency — /dev/urandom + od).
_ucil_rand_hex() {
  local n_bytes="$1"
  od -An -vtx1 -N "$n_bytes" /dev/urandom 2>/dev/null | tr -d ' \n'
}

# ---- Derive a 32-hex trace id from an arbitrary string (sha256, first 16 B).
_ucil_hex_from_id() {
  local id="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$id" | sha256sum | cut -c1-32
  else
    # Fallback: random when sha256sum is unavailable.
    _ucil_rand_hex 16
  fi
}

# ---- Mint or inherit a traceparent.
ucil_ensure_traceparent() {
  local trace_id span_id parent_trace

  # If TRACEPARENT is already valid-looking, extract its trace-id and mint a
  # fresh span-id for this link in the chain.
  if [[ "${TRACEPARENT:-}" =~ ^00-([0-9a-f]{32})-([0-9a-f]{16})-[0-9a-f]{2}$ ]]; then
    trace_id="${BASH_REMATCH[1]}"
    span_id="$(_ucil_rand_hex 8)"
    export TRACEPARENT="00-${trace_id}-${span_id}-01"
    export UCIL_TRACE_ID="$trace_id"
    return 0
  fi

  # Derive from WO id / target if we have one, else random.
  local seed="${UCIL_WO_ID:-${TARGET:-}}"
  if [[ -z "$seed" ]]; then
    # Last-ditch seed: cwd + pid + date — deterministic within one process only.
    seed="$(pwd)-$$-$(date +%s%N 2>/dev/null || date +%s)"
  fi
  trace_id="$(_ucil_hex_from_id "$seed")"
  span_id="$(_ucil_rand_hex 8)"

  # Guard: sha256 can include whitespace on some platforms — re-strip.
  trace_id="${trace_id//[^0-9a-f]/}"
  trace_id="${trace_id:0:32}"
  span_id="${span_id:0:16}"

  export TRACEPARENT="00-${trace_id}-${span_id}-01"
  export UCIL_TRACE_ID="$trace_id"
}

# ---- OTel: publish endpoint + resource attrs when OTEL_ENABLED=1.
ucil_export_otel_env() {
  if [[ "${OTEL_ENABLED:-0}" != "1" ]]; then
    return 0
  fi

  # Defaults — user may override in .env.
  export OTEL_EXPORTER_OTLP_ENDPOINT="${OTEL_EXPORTER_OTLP_ENDPOINT:-http://localhost:4318}"
  export OTEL_RESOURCE_ATTRIBUTES="${OTEL_RESOURCE_ATTRIBUTES:-service.name=ucil-build,phase=auto}"

  # Claude Code native OTel switch.
  export CLAUDE_CODE_ENABLE_TELEMETRY=1
}

# Run once per source.
ucil_ensure_traceparent
ucil_export_otel_env
