# Scale Suite

One-command reliability and scale validation for the game server.

## Run

```bash
./scripts/scale/run.sh
```

## What it runs

1. Rust backend integration tests
2. Backend stress baseline (`stress_test_game_tick`)
3. Backend stress with bots (`stress_test_game_tick_with_bots`)
4. Playwright E2E suite (enabled by default)
5. UI performance benchmark (`scripts/ui_bench/run.js`)
6. UI real-frame render stress benchmark (`scripts/ui_bench/render_stress.js`)
7. WebGPU capability/throughput probe (`scripts/ui_bench/webgpu_probe.js`)
8. Multi-client browser benchmark (`scripts/ui_bench/multi_client.js`)

Artifacts are written to `artifacts/scale/`.

Run outputs include:

- `artifacts/scale/steps.tsv`: step-by-step pass/fail log
- `artifacts/scale/summary.json`: machine-readable summary
- `artifacts/scale/summary.md`: human-readable summary

## Useful environment variables

- `SCALE_USE_EXISTING_SERVER=1`: do not start a local server
- `SCALE_BASE_URL=http://127.0.0.1:8080`: base URL for client tests
- `SCALE_WS_URL=ws://127.0.0.1:8080/ws`: WebSocket URL injected into clients
- `RUN_E2E=0`: skip Playwright E2E stage
- `SCALE_CLIENTS=40`: number of browser clients for multi-client run
- `SCALE_DURATION=60`: multi-client sample duration (seconds)
- `STRESS_TICKS=240`: backend stress iterations
- `STRESS_BOTS=300`: bots spawned during backend stress test
- `STRESS_TARGET_BOT_COUNT=300`: target bot population during stress
- `STRESS_TICK_TIMEOUT_SECS=20`: per-tick timeout guard for deadlock/stall detection
- `UI_BENCH_URL=http://127.0.0.1:8080/client.html?mode=stable`: URL used for UI bench stage (defaults to stable mode)
- `UI_BENCH_FPS_THRESHOLD=60`: FPS threshold for UI bench
- `UI_BENCH_MAX_LONG_TASKS=10000`: max long tasks allowed in UI bench
- `UI_BENCH_MAX_HEAP_GROWTH_MB=150`: max heap growth allowed in UI bench
- `RUN_UI_RENDER_STRESS=1`: run the PIXI/WebGL real-frame render stress stage
- `UI_RENDER_STRESS_START_OBJECTS=400`: starting synthetic object pressure
- `UI_RENDER_STRESS_MAX_OBJECTS=2800`: max synthetic object pressure
- `UI_RENDER_STRESS_STEP_OBJECTS=400`: object sweep step size
- `UI_RENDER_STRESS_MIN_FPS_RATIO=0.45`: min FPS ratio vs baseline in render stress
- `UI_RENDER_STRESS_MAX_SMOOTHED_FRAME_MS=36`: max average smoothed frame-time per stage
- `RUN_WEBGPU_PROBE=1`: run the WebGPU probe stage
- `WEBGPU_PROBE_FRAME_PACING=uncapped`: `uncapped` or `raf`
- `WEBGPU_PROBE_WAIT_GPU_EVERY_FRAMES=120`: periodic queue sync cadence for stable probe metrics
- `WEBGPU_PROBE_MIN_FPS=0`: FPS threshold for WebGPU probe (`0` disables FPS gating)

Optional strict backend budgets:

- `STRESS_P95_BUDGET_MS`
- `STRESS_MAX_TICK_MS`
- `STRESS_BOT_P95_BUDGET_MS`
- `STRESS_BOT_MAX_TICK_MS`
