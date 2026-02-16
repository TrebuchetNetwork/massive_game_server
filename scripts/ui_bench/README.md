# UI Benchmark

Automated UI stress benchmark using Playwright.

## Quick start
1. Run the server.
2. Load the client in the benchmark:

```bash
./scripts/ui_bench.sh --url http://localhost:8080/client.html --duration 30 --warmup 5
```

## Options
- `--url <url>`: page URL
- `--duration <seconds>`: benchmark duration
- `--warmup <seconds>`: warmup duration
- `--fps-threshold <fps>`: fail if average FPS below
- `--max-long-tasks <count>`: fail if long tasks exceed
- `--max-heap-growth-mb <mb>`: fail if heap growth exceeds
- `--headed`: show browser
- `--no-auto-connect`: do not click Connect button
- `--ws <ws_url>`: set wsUrl input before connect
- `--out <path>`: JSON output path

## Output
Results are written to `artifacts/ui_bench.json` by default and echoed to stdout.

## Multi-client scale run

```bash
node scripts/ui_bench/multi_client.js --url http://localhost:8080/client.html?mode=bench --clients 40 --duration 45 --ws ws://localhost:8080/ws --out artifacts/scale/multi_client.json
```

`mode=bench` defaults to a conservative 30 FPS cap per client to keep large fan-out runs stable.
For targeted FPS checks, override with `bench_max_fps`, for example:

```text
http://localhost:8080/client.html?mode=bench&bench_max_fps=60
```

## Reconnect chaos validation

```bash
./scripts/ui_reconnect_chaos.sh \
  --url "http://localhost:8080/client.html?mode=mass&worker_cull=1&auto_connect=1&auto_reconnect=1" \
  --ws ws://localhost:8080/ws \
  --cycles 10 \
  --mode mixed \
  --out artifacts/scale/reconnect_chaos.json
```

This run force-closes signaling/data channels and verifies automatic recovery ratio and median recovery latency.

## Mobile/browser matrix

```bash
./scripts/ui_mobile_matrix.sh \
  --url "http://localhost:8080/client.html?mode=stable&worker_cull=1&mobile=1&auto_connect=1&auto_reconnect=1" \
  --ws ws://localhost:8080/ws \
  --profiles desktop,iphone13,pixel7,ipadMini \
  --duration 12 \
  --min-fps 60 \
  --out artifacts/scale/mobile_matrix.json
```

This run validates connect/reconnect latency plus FPS gates across desktop + mobile emulation profiles.

## Live bottleneck profile

```bash
./scripts/ui_profile.sh --url http://localhost:8080/client.html --duration 30 --warmup 5 --ws ws://localhost:8080/ws --out artifacts/ui_profile_baseline.json
```

The live profile records:
- FPS, long-task count/duration, and JS heap growth
- Per-phase timing exported from `window.__e2e.perfReport`
- Ranked bottlenecks (`topBottlenecks`) with duty-cycle/share percentages

## JS flamegraph capture

```bash
./scripts/ui_flamegraph.sh \
  --url "http://localhost:8080/client.html?profile=1&mode=stable&worker_cull=1" \
  --ws ws://localhost:8080/ws \
  --warmup 5 \
  --duration 20 \
  --top-frames 20 \
  --out artifacts/scale/ui_flamegraph.cpuprofile \
  --summary-out artifacts/scale/ui_flamegraph_summary.json
```

This run records a Chromium CPU profile (`.cpuprofile`) and emits a JSON summary with:
- top self-time frames
- top inclusive-time frames
- client-only top frames (filtered to `client.html`)
- live match snapshot (players/projectiles/effects, worker/WebGPU layer state)

## Live battle probe (real combat)

```bash
node scripts/ui_bench/battle_probe.js \
  --url http://localhost:8080/client.html?mode=stable \
  --ws ws://localhost:8080/ws \
  --duration 30 \
  --warmup 5 \
  --sample-interval-ms 500 \
  --fps-threshold 60 \
  --out artifacts/scale/battle_probe.json
```

This probe samples a live match and reports:
- real `requestAnimationFrame` FPS during combat
- visible players/projectiles and active effects
- `projectileActiveRatio` to confirm ongoing shooting activity

For high-object stress, use worker-cull mass mode:

```text
http://localhost:8080/client.html?mode=mass&worker_cull=1
```

Optional worker tuning:
- `worker_cull_interval_ms=<ms>`: worker update cadence (default `33`)
- `worker_wasm_url=<url>`: optional WASM kernel URL (worker falls back to JS if unavailable)

Connection reliability URL params (useful for real mobile/browser sessions):
- `auto_connect=1`: auto-press Connect on page load
- `auto_reconnect=1`: retry after WebRTC/signaling drops (enabled by default on mobile mode)
- `auto_reconnect_max=<n>`: reconnect attempt cap
- `stun=<stun_url[,stun_url2]>`: override client STUN URLs
- `turn=<turn_url[,turn_url2]>`: add TURN relay URLs (credentials must come from `window.__MGS_TURN_CONFIG` or `mgs_turn_username` / `mgs_turn_credential` storage keys)
- `ice=<urls_csv|username|credential>`: add advanced ICE entries; can be repeated or `;`-separated

## Real-frame render stress (PIXI/WebGL)

```bash
./scripts/ui_render_stress.sh \
  --url http://localhost:8080/client.html?profile=1&mode=bench \
  --ws ws://localhost:8080/ws \
  --auto-connect \
  --duration 10 \
  --start-objects 400 \
  --max-objects 2800 \
  --step-objects 400 \
  --min-fps-ratio 0.6 \
  --max-smoothed-frame-ms 34 \
  --out artifacts/ui_bench/render_stress.json
```

This benchmark:
- runs the actual PIXI/WebGL frame loop (`requestAnimationFrame`) under synthetic battle load
- scales object pressure (projectiles + FX) and optionally refines the pass/fail boundary
- reports sustainable object count, FPS, long-task metrics, heap growth, and smoothed frame-time

## Projectile capacity benchmark

```bash
./scripts/ui_projectile_bench.sh \
  --url http://localhost:8080/client.html?mode=bench \
  --duration 8 \
  --start 200 \
  --max 3000 \
  --step 200 \
  --min-fps-ratio 0.7 \
  --out artifacts/ui_bench/projectile_capacity.json
```

This benchmark injects synthetic projectile objects via `window.__e2e.setSyntheticProjectileCount` and reports:
- maximum sustainable projectile count under the configured thresholds
- visible projectile cap and observed map projectile count
- per-stage FPS, long-task, and heap growth data

## Full FX battle stress

```bash
./scripts/ui_full_battle_test.sh \
  --url http://localhost:8080/client.html?profile=1 \
  --ws ws://localhost:8080/ws \
  --auto-connect \
  --duration 12 \
  --start-intensity 4 \
  --max-intensity 28 \
  --step-intensity 4 \
  --synthetic-projectiles 1400 \
  --out artifacts/ui_bench/full_fx_battle.json
```

This benchmark enables full visual FX and stress-runs:
- weapon fire, muzzle flashes, impacts, explosions, and damage numbers
- periodic screen shake/flash and celebratory FX bursts
- synthetic projectile pressure while connected to a live match

## WebGPU probe

```bash
./scripts/ui_webgpu_test.sh \
  --url http://localhost:8080/client.html?mode=webgpu&profile=1 \
  --duration 5 \
  --width 1920 \
  --height 1080 \
  --frame-pacing uncapped \
  --wait-gpu-every-frames 120 \
  --min-fps 60 \
  --out artifacts/ui_bench/webgpu_probe.json
```

This benchmark:
- enables Chromium WebGPU flags
- calls `window.__e2e.runWebGPUTest(...)` in the client
- supports `uncapped` probe pacing for headless-friendly throughput sampling
- includes `submitFps` and queue-drain timing for uncapped runs
- reports adapter info, FPS, frame timing, and pass/fail status
