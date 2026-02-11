#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARTIFACT_DIR="$ROOT_DIR/artifacts/scale"
STEPS_FILE="$ARTIFACT_DIR/steps.tsv"
mkdir -p "$ARTIFACT_DIR"
printf "step\tstatus\tlog\n" >"$STEPS_FILE"

SCALE_SERVER_PORT="${SCALE_SERVER_PORT:-18080}"
SCALE_SERVER_BIND_HOST="${SCALE_SERVER_BIND_HOST:-0.0.0.0}"
SCALE_SERVER_PUBLIC_HOST="${SCALE_SERVER_PUBLIC_HOST:-127.0.0.1}"
BASE_URL="${SCALE_BASE_URL:-http://${SCALE_SERVER_PUBLIC_HOST}:${SCALE_SERVER_PORT}}"
WS_URL="${SCALE_WS_URL:-ws://${SCALE_SERVER_PUBLIC_HOST}:${SCALE_SERVER_PORT}/ws}"
MULTI_CLIENT_URL="${SCALE_MULTI_CLIENT_URL:-${BASE_URL}/client.html?mode=bench}"
SERVER_CMD="${SCALE_SERVER_CMD:-$ROOT_DIR/target/debug/massive_game_server_core}"
USE_EXISTING_SERVER="${SCALE_USE_EXISTING_SERVER:-0}"
BUILD_SERVER="${SCALE_BUILD_SERVER:-1}"

STRESS_TICKS="${STRESS_TICKS:-60}"
STRESS_BOTS="${STRESS_BOTS:-40}"
STRESS_TARGET_BOT_COUNT="${STRESS_TARGET_BOT_COUNT:-$STRESS_BOTS}"
STRESS_TICK_TIMEOUT_SECS="${STRESS_TICK_TIMEOUT_SECS:-20}"

UI_BENCH_DURATION="${UI_BENCH_DURATION:-30}"
UI_BENCH_WARMUP="${UI_BENCH_WARMUP:-5}"
UI_BENCH_URL="${UI_BENCH_URL:-${BASE_URL}/client.html?mode=stable}"
UI_BENCH_FPS_THRESHOLD="${UI_BENCH_FPS_THRESHOLD:-60}"
UI_BENCH_MAX_LONG_TASKS="${UI_BENCH_MAX_LONG_TASKS:-10000}"
UI_BENCH_MAX_HEAP_GROWTH_MB="${UI_BENCH_MAX_HEAP_GROWTH_MB:-150}"

RUN_UI_RENDER_STRESS="${RUN_UI_RENDER_STRESS:-1}"
UI_RENDER_STRESS_DURATION="${UI_RENDER_STRESS_DURATION:-10}"
UI_RENDER_STRESS_WARMUP="${UI_RENDER_STRESS_WARMUP:-3}"
UI_RENDER_STRESS_START_OBJECTS="${UI_RENDER_STRESS_START_OBJECTS:-400}"
UI_RENDER_STRESS_MAX_OBJECTS="${UI_RENDER_STRESS_MAX_OBJECTS:-2800}"
UI_RENDER_STRESS_STEP_OBJECTS="${UI_RENDER_STRESS_STEP_OBJECTS:-400}"
UI_RENDER_STRESS_REFINE="${UI_RENDER_STRESS_REFINE:-1}"
UI_RENDER_STRESS_REFINE_GRANULARITY="${UI_RENDER_STRESS_REFINE_GRANULARITY:-100}"
UI_RENDER_STRESS_FX_INTERVAL_MS="${UI_RENDER_STRESS_FX_INTERVAL_MS:-120}"
UI_RENDER_STRESS_INTENSITY_SCALE="${UI_RENDER_STRESS_INTENSITY_SCALE:-0.015}"
UI_RENDER_STRESS_MIN_INTENSITY="${UI_RENDER_STRESS_MIN_INTENSITY:-2}"
UI_RENDER_STRESS_MAX_INTENSITY="${UI_RENDER_STRESS_MAX_INTENSITY:-40}"
UI_RENDER_STRESS_MIN_FPS="${UI_RENDER_STRESS_MIN_FPS:-0}"
UI_RENDER_STRESS_MIN_FPS_RATIO="${UI_RENDER_STRESS_MIN_FPS_RATIO:-0.6}"
UI_RENDER_STRESS_MAX_LONG_TASKS="${UI_RENDER_STRESS_MAX_LONG_TASKS:--1}"
UI_RENDER_STRESS_MAX_LONG_TASK_AVG_MS="${UI_RENDER_STRESS_MAX_LONG_TASK_AVG_MS:--1}"
UI_RENDER_STRESS_MAX_HEAP_GROWTH_MB="${UI_RENDER_STRESS_MAX_HEAP_GROWTH_MB:--1}"
UI_RENDER_STRESS_MAX_SMOOTHED_FRAME_MS="${UI_RENDER_STRESS_MAX_SMOOTHED_FRAME_MS:-34}"

RUN_WEBGPU_PROBE="${RUN_WEBGPU_PROBE:-1}"
WEBGPU_PROBE_DURATION="${WEBGPU_PROBE_DURATION:-3}"
WEBGPU_PROBE_WIDTH="${WEBGPU_PROBE_WIDTH:-1280}"
WEBGPU_PROBE_HEIGHT="${WEBGPU_PROBE_HEIGHT:-720}"
WEBGPU_PROBE_MIN_FPS="${WEBGPU_PROBE_MIN_FPS:-0}"
WEBGPU_PROBE_POWER="${WEBGPU_PROBE_POWER:-high-performance}"
WEBGPU_PROBE_FRAME_PACING="${WEBGPU_PROBE_FRAME_PACING:-uncapped}"
WEBGPU_PROBE_YIELD_EVERY_FRAMES="${WEBGPU_PROBE_YIELD_EVERY_FRAMES:-200}"
WEBGPU_PROBE_WAIT_GPU_EVERY_FRAMES="${WEBGPU_PROBE_WAIT_GPU_EVERY_FRAMES:-120}"
WEBGPU_PROBE_HEADLESS="${WEBGPU_PROBE_HEADLESS:-1}"

SCALE_CLIENTS="${SCALE_CLIENTS:-40}"
SCALE_DURATION="${SCALE_DURATION:-45}"
SCALE_SPAWN_DELAY_MS="${SCALE_SPAWN_DELAY_MS:-120}"
SCALE_CONNECT_TIMEOUT_MS="${SCALE_CONNECT_TIMEOUT_MS:-30000}"
SCALE_NAV_TIMEOUT_MS="${SCALE_NAV_TIMEOUT_MS:-60000}"
SCALE_CLICK_TIMEOUT_MS="${SCALE_CLICK_TIMEOUT_MS:-10000}"
SCALE_SAMPLE_INTERVAL_MS="${SCALE_SAMPLE_INTERVAL_MS:-2000}"
SCALE_MIN_CONNECTED_RATIO="${SCALE_MIN_CONNECTED_RATIO:-0.90}"
SCALE_MAX_ERROR_CLIENTS="${SCALE_MAX_ERROR_CLIENTS:-2}"
SCALE_CONNECT_CONCURRENCY="${SCALE_CONNECT_CONCURRENCY:-6}"
SCALE_MAX_TOTAL_MS="${SCALE_MAX_TOTAL_MS:-0}"

RUN_E2E="${RUN_E2E:-1}"
SERVER_PID=""
overall_status=0

log() {
  echo "[scale] $*"
}

slugify() {
  echo "$1" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '_'
}

record_step() {
  local name="$1"
  local status="$2"
  local logfile="$3"
  printf "%s\t%s\t%s\n" "$name" "$status" "$logfile" >>"$STEPS_FILE"
}

wait_for_http() {
  local url="$1"
  local timeout_seconds="$2"
  local started_at
  started_at="$(date +%s)"

  until curl -fsS "$url" >/dev/null 2>&1; do
    if (( "$(date +%s)" - started_at >= timeout_seconds )); then
      return 1
    fi
    sleep 1
  done
}

run_step() {
  local name="$1"
  shift

  local slug
  slug="$(slugify "$name")"
  local logfile="$ARTIFACT_DIR/${slug}.log"

  log "START: $name"
  set +e
  "$@" > >(tee "$logfile") 2>&1
  local rc=$?
  set -e

  if [[ $rc -eq 0 ]]; then
    log "PASS: $name"
    record_step "$name" "PASS" "$logfile"
  else
    log "FAIL: $name (exit=$rc)"
    record_step "$name" "FAIL" "$logfile"
    overall_status=1
  fi
}

cleanup() {
  if [[ -n "${SERVER_PID}" ]]; then
    if kill -0 "$SERVER_PID" >/dev/null 2>&1; then
      log "Stopping local scale server (pid=$SERVER_PID)"
      kill "$SERVER_PID" >/dev/null 2>&1 || true
      wait "$SERVER_PID" >/dev/null 2>&1 || true
    fi
  fi
}
trap cleanup EXIT

if [[ "$USE_EXISTING_SERVER" != "1" ]]; then
  if command -v lsof >/dev/null 2>&1; then
    if lsof -nP -iTCP:"$SCALE_SERVER_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
      log "Port $SCALE_SERVER_PORT is already in use. Stop the existing listener or set SCALE_SERVER_PORT."
      exit 1
    fi
  fi

  if [[ "$BUILD_SERVER" == "1" ]]; then
    run_step "Build server binary" \
      cargo build -p massive_game_server_core --bin massive_game_server_core
    if [[ "$overall_status" -ne 0 ]]; then
      exit "$overall_status"
    fi
  fi

  log "Starting server: $SERVER_CMD"
  (
    cd "$ROOT_DIR"
    MGS_HOST="$SCALE_SERVER_BIND_HOST" \
    MGS_PORT="$SCALE_SERVER_PORT" \
    MGS_DISABLE_STUN="${MGS_DISABLE_STUN:-1}" \
    MGS_TARGET_BOT_COUNT="${MGS_TARGET_BOT_COUNT:-0}" \
    RUST_LOG="${SCALE_RUST_LOG:-massive_game_server_core=warn,warp=warn,webrtc=warn}" \
      bash -lc "$SERVER_CMD"
  ) >"$ARTIFACT_DIR/server.log" 2>&1 &
  SERVER_PID="$!"

  if wait_for_http "$BASE_URL/client.html" 90; then
    if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
      log "Local scale server process exited unexpectedly during startup. See $ARTIFACT_DIR/server.log"
      exit 1
    fi
    log "Server is ready at $BASE_URL"
  else
    log "Server failed to start within timeout. See $ARTIFACT_DIR/server.log"
    exit 1
  fi
fi

run_step "Backend integration tests" \
  cargo test -p massive_game_server_core --test walls_integration --test basic_gameplay

run_step "Backend stress baseline" \
  env \
    RUN_STRESS_TEST=1 \
    STRESS_TICKS="$STRESS_TICKS" \
    STRESS_TICK_TIMEOUT_SECS="$STRESS_TICK_TIMEOUT_SECS" \
    STRESS_P95_BUDGET_MS="${STRESS_P95_BUDGET_MS:-}" \
    STRESS_MAX_TICK_MS="${STRESS_MAX_TICK_MS:-}" \
    cargo test -p massive_game_server_core --test boundary_stress -- --exact stress_test_game_tick --nocapture

run_step "Backend stress bots" \
  env \
    RUN_STRESS_TEST=1 \
    STRESS_TICKS="$STRESS_TICKS" \
    STRESS_BOTS="$STRESS_BOTS" \
    STRESS_TARGET_BOT_COUNT="$STRESS_TARGET_BOT_COUNT" \
    STRESS_TICK_TIMEOUT_SECS="$STRESS_TICK_TIMEOUT_SECS" \
    STRESS_BOT_P95_BUDGET_MS="${STRESS_BOT_P95_BUDGET_MS:-}" \
    STRESS_BOT_MAX_TICK_MS="${STRESS_BOT_MAX_TICK_MS:-}" \
    cargo test -p massive_game_server_core --test boundary_stress -- --exact stress_test_game_tick_with_bots --nocapture

if [[ "$RUN_E2E" == "1" ]]; then
  run_step "Playwright E2E suite" \
    env \
      E2E_SERVER_SKIP=1 \
      E2E_BASE_URL="$BASE_URL" \
      E2E_WS_URL="$WS_URL" \
      bash "$ROOT_DIR/scripts/e2e/run.sh"
fi

run_step "UI benchmark" \
  "$ROOT_DIR/scripts/ui_bench.sh" \
    --url "$UI_BENCH_URL" \
    --duration "$UI_BENCH_DURATION" \
    --warmup "$UI_BENCH_WARMUP" \
    --fps-threshold "$UI_BENCH_FPS_THRESHOLD" \
    --max-long-tasks "$UI_BENCH_MAX_LONG_TASKS" \
    --max-heap-growth-mb "$UI_BENCH_MAX_HEAP_GROWTH_MB" \
    --ws "$WS_URL" \
    --out "$ARTIFACT_DIR/ui_bench.json"

if [[ "$RUN_UI_RENDER_STRESS" == "1" ]]; then
  RENDER_STRESS_REFINE_FLAG="--refine"
  if [[ "$UI_RENDER_STRESS_REFINE" != "1" ]]; then
    RENDER_STRESS_REFINE_FLAG="--no-refine"
  fi

  run_step "UI real-frame render stress" \
    "$ROOT_DIR/scripts/ui_render_stress.sh" \
      --url "$BASE_URL/client.html?profile=1&mode=bench" \
      --ws "$WS_URL" \
      --auto-connect \
      --duration "$UI_RENDER_STRESS_DURATION" \
      --warmup "$UI_RENDER_STRESS_WARMUP" \
      --start-objects "$UI_RENDER_STRESS_START_OBJECTS" \
      --max-objects "$UI_RENDER_STRESS_MAX_OBJECTS" \
      --step-objects "$UI_RENDER_STRESS_STEP_OBJECTS" \
      "$RENDER_STRESS_REFINE_FLAG" \
      --refine-granularity "$UI_RENDER_STRESS_REFINE_GRANULARITY" \
      --fx-interval-ms "$UI_RENDER_STRESS_FX_INTERVAL_MS" \
      --intensity-scale "$UI_RENDER_STRESS_INTENSITY_SCALE" \
      --min-intensity "$UI_RENDER_STRESS_MIN_INTENSITY" \
      --max-intensity "$UI_RENDER_STRESS_MAX_INTENSITY" \
      --min-fps "$UI_RENDER_STRESS_MIN_FPS" \
      --min-fps-ratio "$UI_RENDER_STRESS_MIN_FPS_RATIO" \
      --max-long-tasks "$UI_RENDER_STRESS_MAX_LONG_TASKS" \
      --max-long-task-avg-ms "$UI_RENDER_STRESS_MAX_LONG_TASK_AVG_MS" \
      --max-heap-growth-mb "$UI_RENDER_STRESS_MAX_HEAP_GROWTH_MB" \
      --max-smoothed-frame-ms "$UI_RENDER_STRESS_MAX_SMOOTHED_FRAME_MS" \
      --out "$ARTIFACT_DIR/ui_render_stress.json"
fi

if [[ "$RUN_WEBGPU_PROBE" == "1" ]]; then
  WEBGPU_PROBE_HEADLESS_FLAG="--headless"
  if [[ "$WEBGPU_PROBE_HEADLESS" != "1" ]]; then
    WEBGPU_PROBE_HEADLESS_FLAG="--headed"
  fi

  run_step "WebGPU probe benchmark" \
    "$ROOT_DIR/scripts/ui_webgpu_test.sh" \
      "$WEBGPU_PROBE_HEADLESS_FLAG" \
      --url "$BASE_URL/client.html?mode=webgpu&profile=1" \
      --duration "$WEBGPU_PROBE_DURATION" \
      --width "$WEBGPU_PROBE_WIDTH" \
      --height "$WEBGPU_PROBE_HEIGHT" \
      --power "$WEBGPU_PROBE_POWER" \
      --frame-pacing "$WEBGPU_PROBE_FRAME_PACING" \
      --yield-every-frames "$WEBGPU_PROBE_YIELD_EVERY_FRAMES" \
      --wait-gpu-every-frames "$WEBGPU_PROBE_WAIT_GPU_EVERY_FRAMES" \
      --min-fps "$WEBGPU_PROBE_MIN_FPS" \
      --out "$ARTIFACT_DIR/webgpu_probe.json"
fi

run_step "Multi-client scale benchmark" \
  node "$ROOT_DIR/scripts/ui_bench/multi_client.js" \
    --url "$MULTI_CLIENT_URL" \
    --ws "$WS_URL" \
    --clients "$SCALE_CLIENTS" \
    --connect-concurrency "$SCALE_CONNECT_CONCURRENCY" \
    --duration "$SCALE_DURATION" \
    --spawn-delay-ms "$SCALE_SPAWN_DELAY_MS" \
    --connect-timeout-ms "$SCALE_CONNECT_TIMEOUT_MS" \
    --nav-timeout-ms "$SCALE_NAV_TIMEOUT_MS" \
    --click-timeout-ms "$SCALE_CLICK_TIMEOUT_MS" \
    --sample-interval-ms "$SCALE_SAMPLE_INTERVAL_MS" \
    --min-connected-ratio "$SCALE_MIN_CONNECTED_RATIO" \
    --max-error-clients "$SCALE_MAX_ERROR_CLIENTS" \
    --max-total-ms "$SCALE_MAX_TOTAL_MS" \
    --out "$ARTIFACT_DIR/multi_client.json"

run_step "Generate scale summary" \
  node "$ROOT_DIR/scripts/scale/report.js" \
    --steps "$STEPS_FILE" \
    --ui "$ARTIFACT_DIR/ui_bench.json" \
    --render "$ARTIFACT_DIR/ui_render_stress.json" \
    --webgpu "$ARTIFACT_DIR/webgpu_probe.json" \
    --multi "$ARTIFACT_DIR/multi_client.json" \
    --out "$ARTIFACT_DIR/summary.json" \
    --md "$ARTIFACT_DIR/summary.md"

if [[ "$overall_status" -ne 0 ]]; then
  log "Scale suite finished with failures. Artifacts: $ARTIFACT_DIR"
  exit "$overall_status"
fi

log "Scale suite finished successfully. Artifacts: $ARTIFACT_DIR"
