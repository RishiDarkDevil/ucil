#!/usr/bin/env bash
# Optional: spin up a local OpenTelemetry collector so Claude Code's native
# OTel exporter (CLAUDE_CODE_ENABLE_TELEMETRY=1) has somewhere to ship spans.
#
# Users opt in by setting `OTEL_ENABLED=1` in .env and running this script.
# Non-blocking — the UCIL loop never depends on the collector being up.
#
# Backends supported:
#   - Langfuse cloud (preferred for agent traces) when LANGFUSE_PUBLIC_KEY /
#     LANGFUSE_SECRET_KEY are set in .env.
#   - Local Jaeger (falls back automatically when Langfuse creds are absent).
#
# Usage:
#   scripts/_otel-collector.sh start    # start collector container
#   scripts/_otel-collector.sh stop     # stop + remove
#   scripts/_otel-collector.sh status   # check health
#   scripts/_otel-collector.sh logs     # tail collector logs
#
# Reference:
#   https://docs.claude.com/en/docs/claude-code/telemetry
#   https://langfuse.com/docs/opentelemetry/get-started

set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

# Load .env for OTEL_* / LANGFUSE_* credentials.
if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi

ACTION="${1:-start}"
CONTAINER_NAME="${OTEL_COLLECTOR_CONTAINER:-ucil-otel-collector}"
JAEGER_CONTAINER_NAME="${JAEGER_CONTAINER:-ucil-jaeger}"

_log() { printf '[_otel-collector] %s\n' "$*" >&2; }

# ---- Require docker.
_require_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    _log "ERROR: docker not found. Install it (scripts/install-prereqs.sh) or"
    _log "       point OTEL_EXPORTER_OTLP_ENDPOINT at an existing collector."
    exit 3
  fi
}

# ---- Which backend? Langfuse if creds, else Jaeger.
_backend() {
  if [[ -n "${LANGFUSE_PUBLIC_KEY:-}" && -n "${LANGFUSE_SECRET_KEY:-}" ]]; then
    echo "langfuse"
  else
    echo "jaeger"
  fi
}

_write_collector_config() {
  local backend="$1"
  local cfg="/tmp/ucil-otel-collector-config.yaml"
  case "$backend" in
    langfuse)
      cat > "$cfg" <<YAML
receivers:
  otlp:
    protocols:
      http:
        endpoint: 0.0.0.0:4318
      grpc:
        endpoint: 0.0.0.0:4317

processors:
  batch:
    timeout: 5s
    send_batch_size: 1000

exporters:
  otlphttp/langfuse:
    endpoint: https://cloud.langfuse.com/api/public/otel
    headers:
      Authorization: "Basic $(printf '%s:%s' "${LANGFUSE_PUBLIC_KEY}" "${LANGFUSE_SECRET_KEY}" | base64 -w0)"
  debug:
    verbosity: basic

service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [otlphttp/langfuse, debug]
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [debug]
    logs:
      receivers: [otlp]
      processors: [batch]
      exporters: [debug]
YAML
      ;;
    jaeger)
      cat > "$cfg" <<'YAML'
receivers:
  otlp:
    protocols:
      http:
        endpoint: 0.0.0.0:4318
      grpc:
        endpoint: 0.0.0.0:4317

processors:
  batch:
    timeout: 5s
    send_batch_size: 1000

exporters:
  otlp/jaeger:
    endpoint: ucil-jaeger:4317
    tls:
      insecure: true
  debug:
    verbosity: basic

service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [otlp/jaeger, debug]
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [debug]
    logs:
      receivers: [otlp]
      processors: [batch]
      exporters: [debug]
YAML
      ;;
  esac
  echo "$cfg"
}

_start() {
  _require_docker

  local backend
  backend="$(_backend)"
  _log "Starting OTel collector (backend=${backend})"

  # Create a docker network so collector + jaeger can resolve each other.
  docker network inspect ucil-otel >/dev/null 2>&1 || \
    docker network create ucil-otel >/dev/null

  # Launch Jaeger UI when we're in jaeger-backend mode.
  if [[ "$backend" == "jaeger" ]]; then
    if ! docker ps --filter "name=^${JAEGER_CONTAINER_NAME}$" --format '{{.Names}}' | grep -qx "$JAEGER_CONTAINER_NAME"; then
      _log "Starting jaeger all-in-one (UI on http://localhost:16686)"
      docker run -d --rm \
        --name "$JAEGER_CONTAINER_NAME" \
        --network ucil-otel \
        -p 16686:16686 \
        -p 4317:14317 \
        jaegertracing/all-in-one:1.60 >/dev/null
    fi
  fi

  local cfg
  cfg="$(_write_collector_config "$backend")"

  if docker ps --filter "name=^${CONTAINER_NAME}$" --format '{{.Names}}' | grep -qx "$CONTAINER_NAME"; then
    _log "collector already running; stop with '$0 stop' first to reconfigure"
    return 0
  fi

  docker run -d --rm \
    --name "$CONTAINER_NAME" \
    --network ucil-otel \
    -p 4318:4318 \
    -v "$cfg:/etc/otelcol/config.yaml:ro" \
    otel/opentelemetry-collector-contrib:0.108.0 \
    --config /etc/otelcol/config.yaml >/dev/null

  _log "collector up on :4318 (HTTP). Set OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 in .env."
  if [[ "$backend" == "jaeger" ]]; then
    _log "Jaeger UI: http://localhost:16686"
  else
    _log "Exporting to Langfuse cloud project for ${LANGFUSE_PUBLIC_KEY}"
  fi
}

_stop() {
  _require_docker
  docker rm -f "$CONTAINER_NAME" 2>/dev/null || true
  docker rm -f "$JAEGER_CONTAINER_NAME" 2>/dev/null || true
  _log "collector + jaeger stopped"
}

_status() {
  _require_docker
  docker ps --filter "name=^${CONTAINER_NAME}$" --filter "name=^${JAEGER_CONTAINER_NAME}$" \
    --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
}

_logs() {
  _require_docker
  docker logs -f "$CONTAINER_NAME"
}

case "$ACTION" in
  start)  _start ;;
  stop)   _stop ;;
  status) _status ;;
  logs)   _logs ;;
  *)
    cat >&2 <<EOF
Usage: $0 {start|stop|status|logs}

Env (read from .env):
  OTEL_ENABLED=1                      — opt-in gate (enforced by _trace.sh)
  OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
  OTEL_RESOURCE_ATTRIBUTES=service.name=ucil-build,phase=auto
  LANGFUSE_PUBLIC_KEY / LANGFUSE_SECRET_KEY
                                      — if both set, export to Langfuse cloud
                                        instead of local Jaeger
EOF
    exit 2
    ;;
esac
