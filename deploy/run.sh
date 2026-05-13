#!/usr/bin/env bash
# deploy/run.sh — wrapper that validates required env, then brings up the
# deploy/docker-compose.yml stack with sane defaults.
#
# Usage:
#   export SOLVER_API_TOKEN=...
#   export SOLVER_PRIVATE_KEY=0x...
#   ./deploy/run.sh up        # build + start
#   ./deploy/run.sh down      # stop + remove
#   ./deploy/run.sh logs      # tail logs
#   ./deploy/run.sh status    # show pod state + health URL
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="$HERE/docker-compose.yml"
ACTION="${1:-up}"

# ── Helpers ──────────────────────────────────────────────────────────────
log()  { printf '\033[1;36m[deploy]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[deploy]\033[0m %s\n' "$*" >&2; }
fail() { printf '\033[1;31m[deploy]\033[0m %s\n' "$*" >&2; exit 1; }

require_env() {
    local var="$1"
    if [ -z "${!var:-}" ]; then
        fail "$var must be set before running deploy/run.sh"
    fi
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || fail "missing dependency: $1"
}

# ── Validate runtime + required env (only for `up`) ──────────────────────
require_cmd docker
# Prefer the `docker compose` plugin; fall back to the legacy
# `docker-compose` binary if that's all the host has.
if docker compose version >/dev/null 2>&1; then
    COMPOSE=(docker compose)
elif command -v docker-compose >/dev/null 2>&1; then
    COMPOSE=(docker-compose)
else
    fail "docker compose plugin (or docker-compose) is required"
fi

case "$ACTION" in
    up)
        require_env SOLVER_API_TOKEN
        require_env SOLVER_PRIVATE_KEY
        : "${PORT:=8082}"
        : "${DRY_RUN:=true}"
        log "starting taifoon-solver (PORT=$PORT, DRY_RUN=$DRY_RUN)"
        if [ "$DRY_RUN" = "false" ]; then
            warn "DRY_RUN=false — solver will broadcast fills. Ctrl-C to abort."
            sleep 3
        fi
        "${COMPOSE[@]}" -f "$COMPOSE_FILE" up -d --build
        log "container started. Health endpoint:"
        log "  curl http://127.0.0.1:${PORT}/health"
        log "tail logs with: $0 logs"
        ;;
    down)
        log "stopping taifoon-solver"
        "${COMPOSE[@]}" -f "$COMPOSE_FILE" down
        ;;
    logs)
        "${COMPOSE[@]}" -f "$COMPOSE_FILE" logs -f --tail=200
        ;;
    status)
        "${COMPOSE[@]}" -f "$COMPOSE_FILE" ps
        ;;
    *)
        fail "unknown action: $ACTION (expected: up|down|logs|status)"
        ;;
esac
