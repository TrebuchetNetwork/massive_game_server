#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v python3 >/dev/null 2>&1; then
  echo "[bench-regression] python3 is required" >&2
  exit 1
fi

WARMUP_TIME="${MGS_BENCH_WARMUP_TIME:-0.2}"
MEASUREMENT_TIME="${MGS_BENCH_MEASUREMENT_TIME:-0.6}"
SAMPLE_SIZE="${MGS_BENCH_SAMPLE_SIZE:-20}"
NRESAMPLES="${MGS_BENCH_NRESAMPLES:-2000}"

BENCH_ARGS=(
  --warm-up-time "$WARMUP_TIME"
  --measurement-time "$MEASUREMENT_TIME"
  --sample-size "$SAMPLE_SIZE"
  --nresamples "$NRESAMPLES"
)

echo "[bench-regression] running criterion benches with args: ${BENCH_ARGS[*]}"
cargo bench -p massive_game_server_core --bench physics --locked -- "${BENCH_ARGS[@]}"
cargo bench -p massive_game_server_core --bench serialization --locked -- "${BENCH_ARGS[@]}"
cargo bench -p massive_game_server_core --bench spatial_index --locked -- "${BENCH_ARGS[@]}"

python3 scripts/check_benchmark_thresholds.py \
  --thresholds scripts/bench_thresholds.json \
  --criterion-dir target/criterion
