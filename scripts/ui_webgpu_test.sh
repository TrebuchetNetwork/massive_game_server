#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BENCH_DIR="$ROOT_DIR/scripts/ui_bench"

cd "$BENCH_DIR"

if [ ! -d node_modules ]; then
  echo "Installing ui bench dependencies..."
  npm install
  npx playwright install chromium
fi

node webgpu_probe.js "$@"

