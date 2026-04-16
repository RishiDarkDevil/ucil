#!/usr/bin/env bash
# Install all prerequisites for the UCIL autonomous build on Ubuntu/Debian.
# Idempotent — safe to re-run.
set -euo pipefail

log() { printf '\n[install-prereqs] %s\n' "$*"; }

if [[ "${SKIP_SUDO:-0}" != "1" ]]; then
  log "[1/10] System packages (apt)"
  sudo apt-get update
  sudo apt-get install -y build-essential pkg-config libssl-dev clang cmake \
    git curl jq ripgrep fd-find hyperfine pipx ca-certificates gnupg
fi

log "[2/10] Rust toolchain"
if ! command -v rustup >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
source "$HOME/.cargo/env"
rustup toolchain install stable nightly
rustup component add rustfmt clippy rust-src rust-analyzer
for bin in cargo-nextest cargo-mutants cargo-criterion cargo-deny cargo-audit cargo-llvm-cov; do
  if ! command -v "$bin" >/dev/null; then
    cargo install "$bin" --locked || true
  fi
done
# sccache + shared target-dir + sccache wrapper (scripts/setup-build-cache.sh
# installs sccache if missing and writes .cargo/config.toml).
if [[ -x scripts/setup-build-cache.sh ]]; then
  scripts/setup-build-cache.sh || log "WARN: setup-build-cache.sh failed — non-fatal, continuing"
fi

log "[3/10] Node 20 + pnpm + Biome + vitest"
if ! command -v node >/dev/null; then
  if [[ "${SKIP_SUDO:-0}" != "1" ]]; then
    curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
    sudo apt-get install -y nodejs
  fi
fi
if command -v npm >/dev/null; then
  npm i -g pnpm@latest typescript vitest @playwright/test @biomejs/biome || true
fi

log "[4/10] Python 3.11+ + uv"
# Install whatever python3 the distro ships — uv will manage the actual 3.11+
# runtime used by UCIL's ml/ pipeline independently.
if [[ "${SKIP_SUDO:-0}" != "1" ]]; then
  sudo apt-get install -y python3 python3-venv python3-dev python3-pip || true
fi
if ! command -v uv >/dev/null; then
  curl -LsSf https://astral.sh/uv/install.sh | sh
fi
export PATH="$HOME/.local/bin:$PATH"
# Let uv fetch a pinned Python 3.11 into ~/.local (doesn't touch apt).
if command -v uv >/dev/null 2>&1; then
  uv python install 3.11 || true
fi
for tool in ruff mypy pytest hypothesis; do
  uv tool install "$tool" 2>/dev/null || true
done

log "[5/10] Docker"
if ! command -v docker >/dev/null; then
  if [[ "${SKIP_SUDO:-0}" != "1" ]]; then
    curl -fsSL https://get.docker.com | sh
    sudo usermod -aG docker "$USER" || true
    log "Note: log out and back in for docker group membership to take effect."
  fi
fi

log "[6/10] Ollama (optional — for UCIL's runtime agent-layer tests)"
if ! command -v ollama >/dev/null; then
  curl -fsSL https://ollama.com/install.sh | sh || true
fi
# Pull a small model in the background; non-fatal if it fails
(ollama pull qwen2.5-coder:7b 2>/dev/null || true) &

log "[7/10] GitHub CLI"
if ! command -v gh >/dev/null; then
  if [[ "${SKIP_SUDO:-0}" != "1" ]]; then
    curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg
    sudo chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null
    sudo apt-get update
    sudo apt-get install -y gh
  fi
fi

log "[8/10] Gitleaks + TruffleHog (secret scanners)"
if ! command -v gitleaks >/dev/null; then
  GITLEAKS_VER=$(curl -s https://api.github.com/repos/gitleaks/gitleaks/releases/latest | jq -r .tag_name 2>/dev/null || echo "v8.21.2")
  GITLEAKS_VER_CLEAN="${GITLEAKS_VER#v}"
  curl -sSL "https://github.com/gitleaks/gitleaks/releases/download/${GITLEAKS_VER}/gitleaks_${GITLEAKS_VER_CLEAN}_linux_x64.tar.gz" | sudo tar -xz -C /usr/local/bin gitleaks 2>/dev/null || true
fi
if ! command -v trufflehog >/dev/null; then
  curl -sSfL https://raw.githubusercontent.com/trufflesecurity/trufflehog/main/scripts/install.sh | sudo sh -s -- -b /usr/local/bin 2>/dev/null || true
fi

log "[9/10] Semgrep + Trivy (optional, for security-reviewer agent)"
pipx install semgrep 2>/dev/null || python3 -m pip install --user --break-system-packages semgrep 2>/dev/null || true
if ! command -v trivy >/dev/null; then
  TRIVY_VER=$(curl -s https://api.github.com/repos/aquasecurity/trivy/releases/latest | jq -r .tag_name 2>/dev/null || echo "v0.58.1")
  TRIVY_VER_CLEAN="${TRIVY_VER#v}"
  curl -sSL "https://github.com/aquasecurity/trivy/releases/download/${TRIVY_VER}/trivy_${TRIVY_VER_CLEAN}_Linux-64bit.tar.gz" | sudo tar -xz -C /usr/local/bin trivy 2>/dev/null || true
fi

log "[10/10] ast-grep (structural pattern match for critic)"
if ! command -v ast-grep >/dev/null; then
  cargo install ast-grep --locked 2>/dev/null || true
fi

log "Done. Next steps:"
echo "  1. cp .env.example .env && edit .env (ANTHROPIC_API_KEY, GITHUB_TOKEN)"
echo "  2. ./scripts/bootstrap.sh"
echo "  3. ./scripts/verify-harness.sh"
echo "  4. ./scripts/seed-features.sh   # one-shot planner over master plan"
echo "  5. Review ucil-build/feature-list.json, commit 'freeze: feature oracle v1.0.0'"
echo "  6. In Claude Code: /phase-start 0"
