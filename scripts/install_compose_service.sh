#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVICE_NAME="${SERVICE_NAME:-massive-game-server}"
SERVICE_PATH="/etc/systemd/system/${SERVICE_NAME}.service"
COMPOSE_FILE="$ROOT_DIR/docker/docker-compose.yml"
ENV_FILE="$ROOT_DIR/docker/.env"

usage() {
  cat <<EOF
Usage:
  ./scripts/install_compose_service.sh

Environment:
  SERVICE_NAME   systemd unit name (default: massive-game-server)

This creates and enables:
  $SERVICE_PATH

The service runs:
  docker compose --env-file docker/.env -f docker/docker-compose.yml up -d
EOF
}

run_privileged() {
  if [[ "${EUID:-0}" -eq 0 ]]; then
    "$@"
    return
  fi
  if command -v sudo >/dev/null 2>&1; then
    sudo "$@"
    return
  fi
  echo "error: root privileges required and sudo not found." >&2
  exit 1
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "error: systemd install is only supported on Linux." >&2
  exit 1
fi
if ! command -v docker >/dev/null 2>&1; then
  echo "error: docker is required but not installed." >&2
  exit 1
fi
if [[ ! -f "$COMPOSE_FILE" ]]; then
  echo "error: missing compose file at $COMPOSE_FILE" >&2
  exit 1
fi
if [[ ! -f "$ENV_FILE" ]]; then
  echo "error: missing env file at $ENV_FILE (copy from docker/.env.example first)." >&2
  exit 1
fi

unit_tmp="$(mktemp)"
trap 'rm -f "$unit_tmp"' EXIT

cat > "$unit_tmp" <<EOF
[Unit]
Description=Massive Game Server Docker Compose Stack
Requires=docker.service
After=docker.service network-online.target
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=$ROOT_DIR
ExecStart=/usr/bin/docker compose --env-file $ENV_FILE -f $COMPOSE_FILE up -d
ExecStop=/usr/bin/docker compose --env-file $ENV_FILE -f $COMPOSE_FILE down
ExecReload=/usr/bin/docker compose --env-file $ENV_FILE -f $COMPOSE_FILE up -d
TimeoutStartSec=0

[Install]
WantedBy=multi-user.target
EOF

run_privileged install -m 0644 "$unit_tmp" "$SERVICE_PATH"
run_privileged systemctl daemon-reload
run_privileged systemctl enable --now "${SERVICE_NAME}.service"
run_privileged systemctl status "${SERVICE_NAME}.service" --no-pager

echo "Installed and enabled ${SERVICE_NAME}.service"
