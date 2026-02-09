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
node scripts/ui_bench/multi_client.js --url http://localhost:8080/client.html --clients 24 --duration 45 --ws ws://localhost:8080/ws --out artifacts/scale/multi_client.json
```

## Live bottleneck profile

```bash
./scripts/ui_profile.sh --url http://localhost:8080/client.html --duration 30 --warmup 5 --ws ws://localhost:8080/ws --out artifacts/ui_profile_baseline.json
```

The live profile records:
- FPS, long-task count/duration, and JS heap growth
- Per-phase timing exported from `window.__e2e.perfReport`
- Ranked bottlenecks (`topBottlenecks`) with duty-cycle/share percentages

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
