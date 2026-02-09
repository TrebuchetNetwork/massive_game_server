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
6. Multi-client browser benchmark (`scripts/ui_bench/multi_client.js`)

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
- `UI_BENCH_FPS_THRESHOLD=0`: FPS threshold for UI bench (`0` disables FPS gating)
- `UI_BENCH_MAX_LONG_TASKS=10000`: max long tasks allowed in UI bench
- `UI_BENCH_MAX_HEAP_GROWTH_MB=150`: max heap growth allowed in UI bench

Optional strict backend budgets:

- `STRESS_P95_BUDGET_MS`
- `STRESS_MAX_TICK_MS`
- `STRESS_BOT_P95_BUDGET_MS`
- `STRESS_BOT_MAX_TICK_MS`
