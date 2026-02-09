#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
E2E_DIR="$ROOT_DIR/scripts/e2e"
SERVER_BIN="$ROOT_DIR/target/debug/massive_game_server_core"
HOST="${MGS_HOST:-127.0.0.1}"
PORT="${MGS_PORT:-18080}"
BASE_URL="http://$HOST:$PORT"
WS_URL="ws://$HOST:$PORT/ws"
SERVER_LOG="${VALIDATE_UI_SERVER_LOG:-/tmp/mgs_validate_ui_server.log}"

echo "[validate_ui] Building server binary..."
cd "$ROOT_DIR"
cargo build -p massive_game_server_core --bin massive_game_server_core >/tmp/mgs_validate_ui_build.log 2>&1

if [ ! -x "$SERVER_BIN" ]; then
  echo "[validate_ui] ERROR: server binary not found at $SERVER_BIN" >&2
  exit 1
fi

if lsof -nP -iTCP:"$PORT" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "[validate_ui] ERROR: port $PORT is already in use. Set MGS_PORT to a free port." >&2
  exit 1
fi

cleanup() {
  if [ -n "${SERVER_PID:-}" ] && kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    kill -TERM "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

echo "[validate_ui] Starting server on $BASE_URL ..."
MGS_HOST="$HOST" MGS_PORT="$PORT" "$SERVER_BIN" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

echo "[validate_ui] Waiting for server readiness..."
READY=0
for _ in {1..180}; do
  if curl -fsS "$BASE_URL/client.html" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 0.5
done

if [ "$READY" -ne 1 ]; then
  echo "[validate_ui] ERROR: server did not become ready. See $SERVER_LOG" >&2
  exit 1
fi

cd "$E2E_DIR"
if [ ! -d node_modules ]; then
  echo "[validate_ui] Installing e2e dependencies..."
  npm install
fi

if [ "${VALIDATE_UI_INSTALL_BROWSERS:-0}" = "1" ]; then
  echo "[validate_ui] Installing Playwright browsers..."
  npx playwright install chromium
fi

echo "[validate_ui] Running UI surface audit..."
E2E_SERVER_SKIP=1 \
E2E_BASE_URL="$BASE_URL" \
E2E_WS_URL="$WS_URL" \
npm run ui-audit

echo "[validate_ui] Running connect/runtime e2e checks..."
E2E_SERVER_SKIP=1 \
E2E_BASE_URL="$BASE_URL" \
E2E_WS_URL="$WS_URL" \
npx playwright test tests/connect.spec.js tests/runtime.spec.js --workers=1 --reporter=list

echo "[validate_ui] PASS"
