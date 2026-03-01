#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BENCH_DIR="$ROOT_DIR/scripts/ui_bench"

cd "$BENCH_DIR"

if [ ! -d node_modules ]; then
  echo "Installing ui bench dependencies..."
  if [ -f package-lock.json ]; then
    npm ci
  else
    npm install
  fi
  npx playwright install chromium
fi

node run.js "$@"
