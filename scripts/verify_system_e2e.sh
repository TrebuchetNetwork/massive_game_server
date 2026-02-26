#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
E2E_DIR="$ROOT_DIR/scripts/e2e"

HOST="${MGS_HOST:-127.0.0.1}"
PORT="${MGS_PORT:-18081}"
BASE_URL="${E2E_BASE_URL:-http://$HOST:$PORT}"
WS_URL="${E2E_WS_URL:-ws://$HOST:$PORT/ws}"
RUNS="${RUNS:-3}"
FULL_SUITE="${FULL_SUITE:-0}"
PLAYWRIGHT_REPORTER="${PLAYWRIGHT_REPORTER:-line}"
INSTALL_BROWSERS="${INSTALL_BROWSERS:-0}"
MGS_DISABLE_STUN="${MGS_DISABLE_STUN:-1}"
MGS_TARGET_BOT_COUNT="${MGS_TARGET_BOT_COUNT:-0}"

if ! [[ "$RUNS" =~ ^[0-9]+$ ]] || [ "$RUNS" -lt 1 ]; then
  echo "[verify_system_e2e] ERROR: RUNS must be a positive integer (got '$RUNS')." >&2
  exit 1
fi

if lsof -nP -iTCP:"$PORT" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "[verify_system_e2e] ERROR: port $PORT is already in use." >&2
  echo "[verify_system_e2e] Pick another port, e.g. MGS_PORT=18082 RUNS=$RUNS $0" >&2
  exit 1
fi

echo "[verify_system_e2e] Root: $ROOT_DIR"
echo "[verify_system_e2e] Base URL: $BASE_URL"
echo "[verify_system_e2e] Runs: $RUNS"
echo "[verify_system_e2e] Full suite: $FULL_SUITE"

echo "[verify_system_e2e] Running authoritative server integration tests..."
cargo test --manifest-path "$ROOT_DIR/server/Cargo.toml" --test basic_gameplay -- --nocapture

cd "$E2E_DIR"

if [ ! -d node_modules ]; then
  echo "[verify_system_e2e] Installing e2e dependencies..."
  npm install
fi

if [ "$INSTALL_BROWSERS" = "1" ]; then
  echo "[verify_system_e2e] Installing Playwright browsers..."
  npx playwright install chromium
fi

CORE_TESTS=(
  tests/combat_projectiles.spec.js
  tests/wall_impact.spec.js
  tests/player_wall_collision.spec.js
)

FULL_TESTS=(
  tests/connect.spec.js
  tests/runtime.spec.js
  tests/combat_projectiles.spec.js
  tests/wall_impact.spec.js
  tests/player_wall_collision.spec.js
  tests/ui_performance.spec.js
)

if [ "$FULL_SUITE" = "1" ]; then
  SELECTED_TESTS=("${FULL_TESTS[@]}")
else
  SELECTED_TESTS=("${CORE_TESTS[@]}")
fi

for ((i = 1; i <= RUNS; i += 1)); do
  echo "[verify_system_e2e] ===== E2E RUN $i/$RUNS ====="
  E2E_BASE_URL="$BASE_URL" \
  E2E_WS_URL="$WS_URL" \
  MGS_HOST="$HOST" \
  MGS_PORT="$PORT" \
  MGS_DISABLE_STUN="$MGS_DISABLE_STUN" \
  MGS_TARGET_BOT_COUNT="$MGS_TARGET_BOT_COUNT" \
  npx playwright test "${SELECTED_TESTS[@]}" --workers=1 --reporter="$PLAYWRIGHT_REPORTER"
done

echo "[verify_system_e2e] PASS: all checks succeeded."
