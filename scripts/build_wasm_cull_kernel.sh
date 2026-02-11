#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KERNEL_DIR="$ROOT_DIR/tools/wasm_cull_kernel"
OUT_WASM="$ROOT_DIR/static_client/workers/entity_cull_kernel.wasm"

if ! rustup target list --installed | grep -q '^wasm32-unknown-unknown$'; then
  echo "[wasm-cull] installing wasm32-unknown-unknown target..."
  rustup target add wasm32-unknown-unknown
fi

echo "[wasm-cull] building kernel..."
cargo build \
  --manifest-path "$KERNEL_DIR/Cargo.toml" \
  --target wasm32-unknown-unknown \
  --release

SRC_WASM="$KERNEL_DIR/target/wasm32-unknown-unknown/release/wasm_cull_kernel.wasm"
if [[ ! -f "$SRC_WASM" ]]; then
  echo "[wasm-cull] build succeeded but wasm artifact missing: $SRC_WASM" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUT_WASM")"
cp "$SRC_WASM" "$OUT_WASM"
echo "[wasm-cull] wrote $OUT_WASM"

