#!/usr/bin/env bash
# One-shot harness initialization.
# - Installs git hooks (core.hooksPath)
# - Validates .env exists
# - Optionally creates GitHub origin via `gh repo create`
# - Does NOT seed feature-list.json (that's scripts/seed-features.sh)
set -euo pipefail

cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

log() { printf '\n[bootstrap] %s\n' "$*"; }

# 1. Git hooks
log "Wiring .githooks/"
git config core.hooksPath .githooks
chmod +x .githooks/*

# 2. .env check
if [[ ! -f .env ]]; then
  log "No .env found. Copying .env.example to .env — you must fill ANTHROPIC_API_KEY and GITHUB_TOKEN before any agent can run."
  cp .env.example .env
fi

# 3. Export env if present, without leaking to process listings
if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi

# 4. Harness directories (idempotent)
mkdir -p \
  ucil-build/work-orders \
  ucil-build/verification-reports \
  ucil-build/rejections \
  ucil-build/critic-reports \
  ucil-build/escalations \
  ucil-build/decisions \
  ucil-build/post-mortems \
  ucil-build/phase-log/00-phase-0 \
  ucil-build/phase-log/01-phase-1 \
  ucil-build/phase-log/02-phase-2 \
  ucil-build/phase-log/03-phase-3 \
  ucil-build/phase-log/03.5-phase-3.5 \
  ucil-build/phase-log/04-phase-4 \
  ucil-build/phase-log/05-phase-5 \
  ucil-build/phase-log/06-phase-6 \
  ucil-build/phase-log/07-phase-7 \
  ucil-build/phase-log/08-phase-8 \
  ucil-build/schema

# 5. Ensure scripts are executable
chmod +x scripts/*.sh scripts/gate/*.sh scripts/verify/*.sh 2>/dev/null || true
chmod +x .claude/hooks/session-start/*.sh \
         .claude/hooks/user-prompt-submit/*.sh \
         .claude/hooks/pre-tool-use/*.sh \
         .claude/hooks/post-tool-use/*.sh \
         .claude/hooks/stop/*.sh 2>/dev/null || true

# 6. Symlink for Claude to auto-load the root CLAUDE.md
if [[ -f CLAUDE.md && ! -e .claude/CLAUDE.md ]]; then
  ln -sf ../CLAUDE.md .claude/CLAUDE.md
fi

# 7. GitHub origin (optional)
if [[ -n "${GITHUB_REPO_OWNER:-}" && -n "${GITHUB_REPO_NAME:-}" ]]; then
  if ! git remote get-url origin >/dev/null 2>&1; then
    if command -v gh >/dev/null 2>&1; then
      VISIBILITY="--private"
      [[ "${GITHUB_REPO_PRIVATE:-true}" == "false" ]] && VISIBILITY="--public"
      log "Creating GitHub repo ${GITHUB_REPO_OWNER}/${GITHUB_REPO_NAME}"
      gh repo create "${GITHUB_REPO_OWNER}/${GITHUB_REPO_NAME}" $VISIBILITY --source=. --remote=origin --description "UCIL - Unified Code Intelligence Layer" || true
    else
      log "gh CLI not available — set origin manually: git remote add origin ..."
    fi
  fi
fi

# 8. Summary
log "Bootstrap complete."
echo ""
echo "  Repo root:   $(pwd)"
echo "  Git hooks:   $(git config core.hooksPath)"
echo "  Settings:    .claude/settings.json"
echo "  Agents:      $(ls .claude/agents 2>/dev/null | wc -l) subagents"
echo "  Hooks:       $(find .claude/hooks -name '*.sh' 2>/dev/null | wc -l) Claude hooks"
echo "  Skills:      $(ls .claude/skills 2>/dev/null | wc -l) slash commands"
echo "  Progress:    phase=$(jq -r .phase ucil-build/progress.json), seeded=$(jq -r .seeded ucil-build/progress.json)"
echo ""
echo "  Next:  ./scripts/verify-harness.sh"
echo "         ./scripts/seed-features.sh   (one-shot; requires ANTHROPIC_API_KEY)"
