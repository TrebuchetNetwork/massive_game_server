#!/usr/bin/env bash

set -euo pipefail

DOMAIN="${1:-game.trebuchet.network}"
BASE_URL="https://${DOMAIN}"
WS_URL="wss://${DOMAIN}/ws"

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: required command '$cmd' is not installed." >&2
    exit 1
  fi
}

require_cmd node
require_cmd npm

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "${repo_root}/scripts/e2e"
if [[ ! -d node_modules ]]; then
  npm ci
fi

E2E_SERVER_SKIP=1 \
E2E_BASE_URL="${BASE_URL}" \
E2E_WS_URL="${WS_URL}" \
PUBLIC_SYNTH_DEEP_CONNECT="${PUBLIC_SYNTH_DEEP_CONNECT:-0}" \
  npx playwright test tests/public_synthetic.spec.js --project=chromium --workers=1 --reporter=list
