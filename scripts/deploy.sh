#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/docker/docker-compose.yml"
DOCKER_ENV_FILE="$ROOT_DIR/docker/.env"
DOCKER_ENV_EXAMPLE="$ROOT_DIR/docker/.env.example"

ACTION="${1:-up}"
MODE="${DEPLOY_MODE:-docker}"

if [[ "$MODE" != "docker" && "$MODE" != "native" ]]; then
  echo "Invalid DEPLOY_MODE='$MODE'. Use 'docker' or 'native'." >&2
  exit 1
fi

print_usage() {
  cat <<'EOF'
Usage:
  DEPLOY_MODE=docker ./scripts/deploy.sh up
  DEPLOY_MODE=docker ./scripts/deploy.sh validate
  DEPLOY_MODE=docker ./scripts/deploy.sh down
  DEPLOY_MODE=docker ./scripts/deploy.sh logs
  DEPLOY_MODE=docker ./scripts/deploy.sh status

  DEPLOY_MODE=native ./scripts/deploy.sh up

Environment:
  DEPLOY_MODE   docker|native (default: docker)
  MGS_HOST      bind host (default: 0.0.0.0)
  MGS_PORT      bind port (default: 8080)
  MGS_DISABLE_STUN (default: 1)
  MGS_TARGET_BOT_COUNT (default: 0)
  RUST_LOG      (default: massive_game_server_core=warn,warp=warn,webrtc=warn)
EOF
}

ensure_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    echo "docker is not installed." >&2
    exit 1
  fi
}

ensure_docker_env() {
  if [[ -f "$DOCKER_ENV_FILE" ]]; then
    return 0
  fi
  if [[ ! -f "$DOCKER_ENV_EXAMPLE" ]]; then
    echo "Missing docker/.env.example. Cannot auto-bootstrap docker/.env." >&2
    exit 1
  fi

  cp "$DOCKER_ENV_EXAMPLE" "$DOCKER_ENV_FILE"
  echo "[deploy] created docker/.env from docker/.env.example"
  echo "[deploy] update docker/.env before production rollout."
}

health_check() {
  local host="${MGS_HOST:-0.0.0.0}"
  local port="${MGS_PORT:-8080}"
  local check_host="$host"
  if [[ "$check_host" == "0.0.0.0" ]]; then
    check_host="127.0.0.1"
  fi
  echo "[deploy] waiting for health endpoint at http://${check_host}:${port}/healthz ..."
  for _ in {1..30}; do
    if curl -fsS "http://${check_host}:${port}/healthz" >/dev/null 2>&1; then
      echo "[deploy] server is healthy."
      return 0
    fi
    sleep 1
  done
  echo "[deploy] health check did not pass in time." >&2
  return 1
}

docker_up() {
  ensure_docker
  ensure_docker_env
  docker compose -f "$COMPOSE_FILE" up -d --build
  health_check
}

docker_validate() {
  ensure_docker
  ensure_docker_env
  docker compose -f "$COMPOSE_FILE" config >/dev/null
  docker run --rm \
    -v "$ROOT_DIR/docker/nginx.conf:/etc/nginx/nginx.conf:ro" \
    nginx:1.27-alpine nginx -t >/dev/null
  echo "[deploy] docker-compose and nginx configuration validated."
}

docker_down() {
  ensure_docker
  docker compose -f "$COMPOSE_FILE" down
}

docker_logs() {
  ensure_docker
  docker compose -f "$COMPOSE_FILE" logs -f --tail=200
}

docker_status() {
  ensure_docker
  docker compose -f "$COMPOSE_FILE" ps
}

native_up() {
  local host="${MGS_HOST:-0.0.0.0}"
  local port="${MGS_PORT:-8080}"
  local disable_stun="${MGS_DISABLE_STUN:-1}"
  local bot_count="${MGS_TARGET_BOT_COUNT:-0}"
  local rust_log="${RUST_LOG:-massive_game_server_core=warn,warp=warn,webrtc=warn}"

  (cd "$ROOT_DIR" && cargo build --release -p massive_game_server_core --bin massive_game_server_core)

  echo "[deploy] starting native server on ${host}:${port}"
  exec env \
    MGS_HOST="$host" \
    MGS_PORT="$port" \
    MGS_DISABLE_STUN="$disable_stun" \
    MGS_TARGET_BOT_COUNT="$bot_count" \
    RUST_LOG="$rust_log" \
    "$ROOT_DIR/target/release/massive_game_server_core"
}

case "$ACTION" in
  up)
    if [[ "$MODE" == "docker" ]]; then
      docker_up
    else
      native_up
    fi
    ;;
  validate)
    if [[ "$MODE" == "docker" ]]; then
      docker_validate
    else
      echo "validate action is only supported in DEPLOY_MODE=docker." >&2
      exit 1
    fi
    ;;
  down)
    if [[ "$MODE" == "docker" ]]; then
      docker_down
    else
      echo "down action is only supported in DEPLOY_MODE=docker." >&2
      exit 1
    fi
    ;;
  logs)
    if [[ "$MODE" == "docker" ]]; then
      docker_logs
    else
      echo "logs action is only supported in DEPLOY_MODE=docker." >&2
      exit 1
    fi
    ;;
  status)
    if [[ "$MODE" == "docker" ]]; then
      docker_status
    else
      echo "status action is only supported in DEPLOY_MODE=docker." >&2
      exit 1
    fi
    ;;
  help|-h|--help)
    print_usage
    ;;
  *)
    echo "Unknown action: $ACTION" >&2
    print_usage
    exit 1
    ;;
esac
