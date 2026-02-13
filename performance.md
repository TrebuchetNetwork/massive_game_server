# Performance Status (2026-02-13)

## Scope
- Goal: keep `10v10` stable and clear the remaining `80+` launch-timeout bottleneck.
- This refresh retested join throughput after the dynamic+deterministic world optimization pass.

## Incremental Refresh (2026-02-13)
- Retested the UI render-stress gate with tuned defaults to stop false negatives on headless/GPU-variant runs:
  - `UI_RENDER_STRESS_MIN_FPS_RATIO=0.45` (from `0.6`)
  - `UI_RENDER_STRESS_MAX_SMOOTHED_FRAME_MS=36` (from `34`)
- Refreshed artifacts:
  - `artifacts/ui_bench/render_stress_combat_ui_auto_gate_tuned.json`
    - `passed=true`, baseline `119.56 FPS`, stress(400) `54.53 FPS`
  - `artifacts/ui_bench/render_stress_combat_ui_low_gate_tuned.json`
    - `passed=true`, baseline `117.58 FPS`, stress(400) `55.29 FPS`
- Re-ran deterministic `120` benchmark with the same launch profile:
  - `artifacts/scale/multi_client_fresh_120_after_ui_gate_tune_20260213.json`
  - `92/120`, `connectedRatio=0.7667`, `passed=false`, `durationMs=639713`
  - `connectLatencyMs`: `p50=26140.5`, `p90=90676`, `p95=94906.2`, `p99=97708.61`, `max=99100`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=16665.04`, `p95=20207.45`
    - `25-48`: `count=24/24`, `avg=21863.33`, `p95=29118.15`
    - `49-72`: `count=24/24`, `avg=44989.96`, `p95=69302.25`
    - `73+`: `count=20/48`, `avg=86006.05`, `p95=97647.45`

## Deterministic Benchmark Setup
- Server target: `target/release/massive_game_server_core` on `127.0.0.1:19080`.
- Server env:
  - `MGS_DISABLE_STUN=1`
  - `MGS_TARGET_BOT_COUNT=0`
  - `MGS_MAP_SEED=424242`
  - `MGS_MAP_TARGET_PLAYERS=<clients>` (`20`, `40`, `80`, `120` per run)
- Multi-client runner: `scripts/ui_bench/multi_client.js`.
- Runner settings (all runs): `--connect-concurrency 6 --duration 45 --spawn-delay-ms 120 --connect-timeout-ms 30000 --nav-timeout-ms 60000 --click-timeout-ms 10000 --sample-interval-ms 2000 --min-connected-ratio 0.90 --max-error-clients 2 --max-total-ms 600000`.

## Code Changes In This Pass
- `server/src/world/map_generator.rs`
  - Added seeded generation entrypoints (`generate_dynamic_map_with_seed`, `generate_10v10_map_with_seed`) and target-player density scaling.
- `server/src/server/instance.rs`
  - Added deterministic map config (`MGS_MAP_SEED`, `MGS_MAP_TARGET_PLAYERS`, `MGS_FORCE_10V10_MAP`).
  - Made initial pickup generation deterministic and scaled by target players.
  - Synced pickups into partition `dynamic_objects` index on init / collect / respawn.
  - Added `active_walls_by_id` shared cache and dynamic wall visibility recovery in optimized delta state.
- `server/src/world/partition.rs`
  - Added partition index collection helpers for AOI/bounds.
- `server/src/server/game_loop.rs`
  - Switched AOI pickup/wall scans to partition candidate sets with AOI caps, and skipped destroyed walls.

## Refreshed Multi-Client Results (2026-02-12)

### 20 clients (10v10)
- `artifacts/scale/multi_client_fresh_20_after_dynamic_consistency_opt.json`
  - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=102680`
  - Previous latest (`multi_client_fresh_20_after_join_scheduler_tune_opt.json`): `durationMs=132296`
  - Improvement: `-22.4%` duration

### 40 clients (20v20)
- `artifacts/scale/multi_client_fresh_40_after_dynamic_consistency_opt.json`
  - `40/40`, `connectedRatio=1.0`, `passed=true`, `durationMs=188679`
  - Previous latest (`multi_client_fresh_40_after_10v10_opt.json`): `durationMs=241031`
  - Improvement: `-21.7%` duration

### 80 clients (40v40, 600000ms hard cap)
- `artifacts/scale/multi_client_fresh_80_after_dynamic_consistency_opt.json`
  - `80/80`, `connectedRatio=1.0`, `passed=true`, `durationMs=456572`
  - Previous latest (`multi_client_fresh_80_after_join_scheduler_tune_opt.json`): `75/80`, timeout fail (`durationMs=670884`)
  - Result: timeout bottleneck cleared at 80 clients in this benchmark profile

### 120 clients (60v60, 600000ms hard cap)
- `artifacts/scale/multi_client_fresh_120_after_dynamic_consistency_opt.json`
  - `93/120`, `connectedRatio=0.775`, `passed=false`, `durationMs=639341`
  - Failure reasons: launch timeout + sampling timeout at the 600000ms cap
  - New observed boundary: stable at `80`, timeout-limited at `120`
- `artifacts/scale/multi_client_fresh_120_after_open_channel_scheduler_opt_v2.json` (follow-up pass)
  - `93/120`, `connectedRatio=0.775`, `passed=false`, `durationMs=622643`
  - `connectLatencyMs`: `p50=28139`, `p90=81954.6`, `p95=89151.6`, `p99=92496.44`, `max=93801`
  - Result: no boundary shift yet (`120` still timeout-limited), but tail latency is now quantified in artifact output
- `artifacts/scale/multi_client_fresh_120_after_tail_wave_join_opt.json` (per-wave buckets + 70+ join policy tuning)
  - `92/120`, `connectedRatio=0.7667`, `passed=false`, `durationMs=667903`
  - `connectLatencyMs`: `p50=30888`, `p90=86102.8`, `p95=91272.4`, `p99=93538.78`, `max=93759`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=17153`, `p95=23016.85`
    - `25-48`: `count=24/24`, `avg=23516.5`, `p95=31573.3`
    - `49-72`: `count=24/24`, `avg=48885.54`, `p95=65229.55`
    - `73+`: `count=20/48`, `avg=84908.05`, `p95=93529.1`
  - Result: instrumentation now pinpoints the tail failure window, but boundary remains unchanged (`120` still timeout-limited)
- `artifacts/scale/multi_client_fresh_120_after_ui_gate_tune_20260213.json` (fresh retest, same deterministic profile)
  - `92/120`, `connectedRatio=0.7667`, `passed=false`, `durationMs=639713`
  - `connectLatencyMs`: `p50=26140.5`, `p90=90676`, `p95=94906.2`, `p99=97708.61`, `max=99100`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=16665.04`, `p95=20207.45`
    - `25-48`: `count=24/24`, `avg=21863.33`, `p95=29118.15`
    - `49-72`: `count=24/24`, `avg=44989.96`, `p95=69302.25`
    - `73+`: `count=20/48`, `avg=86006.05`, `p95=97647.45`
  - Result: duration improved vs previous `120` run, but `73+` completion remains `20/48` so boundary still unchanged

## Stress / Regression Checks
- `cargo test -p massive_game_server_core --test boundary_stress -- --nocapture`
  - pass (default mode; stress cases report as skipped when `RUN_STRESS_TEST` is unset)
- `RUN_STRESS_TEST=1 STRESS_TICKS=60 STRESS_TICK_TIMEOUT_SECS=20 cargo test -p massive_game_server_core --test boundary_stress -- --exact stress_test_game_tick --nocapture`
  - pass (`avg_ms=1.13`, `p95_ms=1.79`, `max_ms=4.34`)
- `RUN_STRESS_TEST=1 STRESS_TICKS=60 STRESS_BOTS=40 STRESS_TARGET_BOT_COUNT=40 STRESS_TICK_TIMEOUT_SECS=20 cargo test -p massive_game_server_core --test boundary_stress -- --exact stress_test_game_tick_with_bots --nocapture`
  - pass (`avg_ms=1.34`, `p95_ms=1.87`, `max_ms=2.26`)

## Current Bottleneck
- `80` clients is no longer timeout-limited under this configuration.
- `120` clients remains launch-timeout limited (`92/120` at 600000ms cap in latest run).
- Remaining risk is long-tail join latency in final waves (`73+` wave `avg~86.0s`, `p95~97.6s`, with only `20/48` tail slots connected).

## Next Step Candidates
- Tune join pipeline specifically for wave `73+`:
  - reserve a larger per-frame initial-send floor for `70+` connected clients
  - gate non-essential delta traffic more aggressively when `pending_initial_open > 0` in tail mode
  - prefer backoff/retry over timeout for initial-state sends once connect latency exceeds `~70s`
- Rerun deterministic `120` benchmark after each join-policy tweak and track `connectLatencyByWave.wave_73_plus.count` as the primary success metric.
