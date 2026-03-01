#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
E2E_DIR="$ROOT_DIR/scripts/e2e"

cd "$E2E_DIR"

if [ ! -d node_modules ]; then
  if [ -f package-lock.json ]; then
    npm ci
  else
    npm install
  fi
fi

cd "$ROOT_DIR"
cargo build -p massive_game_server_core --bin massive_game_server_core

cd "$E2E_DIR"
E2E_SERVER_CMD="$ROOT_DIR/target/debug/massive_game_server_core" \
  npm run test
