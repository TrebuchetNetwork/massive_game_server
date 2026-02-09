#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARTIFACTS_DIR="$ROOT_DIR/artifacts/coverage"
mkdir -p "$ARTIFACTS_DIR"

cd "$ROOT_DIR"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "cargo-llvm-cov not found. Install with: cargo install cargo-llvm-cov" >&2
  exit 1
fi

cargo llvm-cov --workspace --all-features --lcov --output-path "$ARTIFACTS_DIR/lcov.info"

# Generate HTML report if llvm-cov can build it.
cargo llvm-cov --workspace --all-features --html --output-dir "$ARTIFACTS_DIR/html"

echo "Coverage artifacts written to $ARTIFACTS_DIR"
