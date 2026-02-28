#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/docker/docker-compose.yml"
DOCKER_ENV_FILE="$ROOT_DIR/docker/.env"
DOCKER_ENV_EXAMPLE="$ROOT_DIR/docker/.env.example"
DOCKER_DIR="$ROOT_DIR/docker"
ROLLBACK_IMAGE_TAG="massive-game-server:rollback"

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
  DEPLOY_MODE=docker ./scripts/deploy.sh rollback
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

docker_compose() {
  docker compose --env-file "$DOCKER_ENV_FILE" -f "$COMPOSE_FILE" "$@"
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

resolve_deploy_env() {
  local key="$1"
  if [[ -n "${!key:-}" ]]; then
    printf '%s' "${!key}"
    return 0
  fi

  local from_file
  from_file="$(grep -E "^${key}=" "$DOCKER_ENV_FILE" 2>/dev/null | tail -n 1 | cut -d'=' -f2- || true)"
  from_file="${from_file%\"}"
  from_file="${from_file#\"}"
  from_file="${from_file%\'}"
  from_file="${from_file#\'}"
  printf '%s' "$from_file"
}

resolve_secret_path() {
  local value="$1"
  if [[ -z "$value" ]]; then
    printf '%s' ""
    return 0
  fi
  if [[ "$value" = /* ]]; then
    printf '%s' "$value"
  else
    printf '%s' "$DOCKER_DIR/${value#./}"
  fi
}

ensure_docker_secrets() {
  local openrouter_path
  local grafana_user_path
  local grafana_password_path

  openrouter_path="$(resolve_secret_path "$(resolve_deploy_env OPENROUTER_API_KEY_SECRET_FILE)")"
  grafana_user_path="$(resolve_secret_path "$(resolve_deploy_env GRAFANA_ADMIN_USER_SECRET_FILE)")"
  grafana_password_path="$(resolve_secret_path "$(resolve_deploy_env GRAFANA_ADMIN_PASSWORD_SECRET_FILE)")"

  openrouter_path="${openrouter_path:-$DOCKER_DIR/secrets/openrouter_api_key}"
  grafana_user_path="${grafana_user_path:-$DOCKER_DIR/secrets/grafana_admin_user}"
  grafana_password_path="${grafana_password_path:-$DOCKER_DIR/secrets/grafana_admin_password}"

  local missing=0
  for path in "$openrouter_path" "$grafana_user_path" "$grafana_password_path"; do
    if [[ ! -f "$path" ]]; then
      echo "[deploy] missing required secret file: $path" >&2
      missing=1
      continue
    fi
    if [[ ! -s "$path" ]]; then
      echo "[deploy] secret file is empty: $path" >&2
      missing=1
    fi
  done

  if [[ $missing -ne 0 ]]; then
    echo "[deploy] see docker/secrets/README.md for setup instructions." >&2
    exit 1
  fi
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

snapshot_rollback_image() {
  local current_image_id
  current_image_id="$(docker_compose images -q massive-game-server | head -n 1)"
  if [[ -z "$current_image_id" ]]; then
    echo "[deploy] no running massive-game-server image found for rollback snapshot."
    return 0
  fi
  if ! docker image inspect "$current_image_id" >/dev/null 2>&1; then
    echo "[deploy] unable to inspect running image id '$current_image_id'; skipping rollback snapshot."
    return 0
  fi
  docker tag "$current_image_id" "$ROLLBACK_IMAGE_TAG"
  echo "[deploy] rollback snapshot captured as $ROLLBACK_IMAGE_TAG"
}

docker_rollback_service() {
  if ! docker image inspect "$ROLLBACK_IMAGE_TAG" >/dev/null 2>&1; then
    echo "[deploy] rollback image '$ROLLBACK_IMAGE_TAG' not found." >&2
    return 1
  fi
  docker tag "$ROLLBACK_IMAGE_TAG" massive-game-server:latest
  docker_compose up -d --no-build massive-game-server
  health_check
}

docker_up() {
  ensure_docker
  ensure_docker_env
  ensure_docker_secrets
  snapshot_rollback_image
  docker_compose up -d --build
  if ! health_check; then
    echo "[deploy] health check failed; attempting rollback."
    if docker_rollback_service; then
      echo "[deploy] rollback succeeded; deployment reverted to previous image." >&2
    else
      echo "[deploy] rollback failed; manual intervention required." >&2
    fi
    return 1
  fi
}

docker_validate() {
  ensure_docker
  ensure_docker_env
  docker_compose config >/dev/null
  docker run --rm \
    -v "$ROOT_DIR/docker/nginx.conf:/etc/nginx/nginx.conf:ro" \
    nginx:1.27-alpine nginx -t >/dev/null
  echo "[deploy] docker-compose and nginx configuration validated."
}

docker_rollback() {
  ensure_docker
  ensure_docker_env
  if ! docker_rollback_service; then
    echo "[deploy] rollback command failed." >&2
    return 1
  fi
  echo "[deploy] rollback command completed."
}

docker_down() {
  ensure_docker
  docker_compose down
}

docker_logs() {
  ensure_docker
  docker_compose logs -f --tail=200
}

docker_status() {
  ensure_docker
  docker_compose ps
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
  rollback)
    if [[ "$MODE" == "docker" ]]; then
      docker_rollback
    else
      echo "rollback action is only supported in DEPLOY_MODE=docker." >&2
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
