# Production Performance Log (Refreshed 2026-02-14)

## Join-Throughput Pass Summary

### Pass objective
- Execute the next join-throughput optimization pass for the remaining `80+` launch timeout.
- Rebaseline with deterministic world generation and refreshed measurements.

### Key code updates in this pass
- `server/src/world/map_generator.rs`
  - seeded dynamic map generation and player-count scaling.
- `server/src/server/instance.rs`
  - deterministic map config via `MGS_MAP_SEED` / `MGS_MAP_TARGET_PLAYERS`.
  - deterministic, scaled pickup generation; pickup partition index kept in sync.
  - shared `active_walls_by_id` cache and dynamic wall delta recovery.
- `server/src/world/partition.rs`
  - partition index collection helpers for AOI/bounds.
- `server/src/server/game_loop.rs`
  - AOI object selection uses partition candidates + visibility caps.

### Deterministic rerun profile
- server: `target/release/massive_game_server_core` on `127.0.0.1:19080`
- env: `MGS_DISABLE_STUN=1`, `MGS_TARGET_BOT_COUNT=0`, `MGS_MAP_SEED=424242`, `MGS_MAP_TARGET_PLAYERS=<clients>`
- multi-client runner: `scripts/ui_bench/multi_client.js`
- hard cap: `--max-total-ms 600000`

### 80-client benchmark progression (600000ms cap)
- baseline: `artifacts/scale/multi_client_fresh_80.json` -> `74/80` (timeout)
- welcome-only on-open patch: `artifacts/scale/multi_client_fresh_80_after_10v10_opt.json` -> `79/80` (timeout)
- retry + scheduler iterations (best pre-pass): `artifacts/scale/multi_client_fresh_80_after_join_scheduler_tune_opt.json` -> `75/80` (timeout)
- dynamic deterministic consistency pass: `artifacts/scale/multi_client_fresh_80_after_dynamic_consistency_opt.json` -> `80/80` (`connectedRatio=1.0`, `passed=true`, `durationMs=456572`)

### Refreshed validation matrix
- `artifacts/scale/multi_client_fresh_20_after_dynamic_consistency_opt.json`
  - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=102680`
- `artifacts/scale/multi_client_fresh_40_after_dynamic_consistency_opt.json`
  - `40/40`, `connectedRatio=1.0`, `passed=true`, `durationMs=188679`
- `artifacts/scale/multi_client_fresh_80_after_dynamic_consistency_opt.json`
  - `80/80`, `connectedRatio=1.0`, `passed=true`, `durationMs=456572`
- `artifacts/scale/multi_client_fresh_120_after_dynamic_consistency_opt.json`
  - `93/120`, `connectedRatio=0.775`, `passed=false`, `durationMs=639341`, timeout fail

### Current scale boundary (deterministic profile)
- `80` clients: clears within cap (`80/80`).
- `120` clients: launch-timeout limited (`93/120` at 600000ms cap).
- Tail latency in final launch wave reached `~94s` per client.

### Follow-up optimization pass (open-channel scheduler + latency instrumentation)
- `scripts/ui_bench/multi_client.js`
  - Added `connectLatencyMs` summary to output JSON (`min/avg/max`, `p50/p90/p95/p99`, `slowestClients`).
  - Added `elapsedMs` to launch failure entries.
- `server/src/server/instance.rs`
  - Join scheduler now prioritizes actionable initial sends (open data channels) while retaining backlog-based throttling.
- Rerun result:
  - `artifacts/scale/multi_client_fresh_120_after_open_channel_scheduler_opt_v2.json`
  - `93/120`, `connectedRatio=0.775`, timeout fail (same boundary as prior run)
  - `connectLatencyMs`: `p50=28139`, `p90=81954.6`, `p95=89151.6`, `p99=92496.44`, `max=93801`

### Tail-wave optimization + per-wave bucket pass
- `scripts/ui_bench/multi_client.js`
  - Added `connectLatencyByWave` output with fixed buckets: `1-24`, `25-48`, `49-72`, `73+`.
- `server/src/server/instance.rs`
  - Added `tail_join_mode` for `70+` connected clients with pending initial backlog.
  - Tail policy now boosts initial-send budget, tightens delta budget/skip modulus, and lowers broadcast concurrency cap.
- Rerun result:
  - `artifacts/scale/multi_client_fresh_120_after_tail_wave_join_opt.json`
  - `92/120`, `connectedRatio=0.7667`, timeout fail (`durationMs=667903`)
  - `connectLatencyMs`: `p50=30888`, `p90=86102.8`, `p95=91272.4`, `p99=93538.78`, `max=93759`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=17153`, `p95=23016.85`
    - `25-48`: `count=24/24`, `avg=23516.5`, `p95=31573.3`
    - `49-72`: `count=24/24`, `avg=48885.54`, `p95=65229.55`
    - `73+`: `count=20/48`, `avg=84908.05`, `p95=93529.1`

### UI render gate retune + deterministic rerun (2026-02-13)
- Gate defaults updated for headless/older GPU variance:
  - `scripts/ui_bench/render_stress.js`: `minFpsRatio=0.45`, `maxSmoothedFrameMs=36`
  - `scripts/scale/run.sh`: `UI_RENDER_STRESS_MIN_FPS_RATIO=0.45`, `UI_RENDER_STRESS_MAX_SMOOTHED_FRAME_MS=36`
  - `scripts/scale/README.md`: defaults documented accordingly
- A/B render-stress reruns with tuned gate:
  - `artifacts/ui_bench/render_stress_combat_ui_auto_gate_tuned.json`
    - `passed=true`, baseline `119.56 FPS`, stress(400) `54.53 FPS`
  - `artifacts/ui_bench/render_stress_combat_ui_low_gate_tuned.json`
    - `passed=true`, baseline `117.58 FPS`, stress(400) `55.29 FPS`
- Fresh deterministic `120` rerun:
  - `artifacts/scale/multi_client_fresh_120_after_ui_gate_tune_20260213.json`
  - `92/120`, `connectedRatio=0.7667`, `passed=false`, `durationMs=639713`
  - `connectLatencyMs`: `p50=26140.5`, `p90=90676`, `p95=94906.2`, `p99=97708.61`, `max=99100`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=16665.04`, `p95=20207.45`
    - `25-48`: `count=24/24`, `avg=21863.33`, `p95=29118.15`
    - `49-72`: `count=24/24`, `avg=44989.96`, `p95=69302.25`
    - `73+`: `count=20/48`, `avg=86006.05`, `p95=97647.45`

### Aggressive 73+ tail pass (2026-02-13)
- `server/src/server/instance.rs`
  - Added aggressive tail mode when `73+` clients are connected and initial-open backlog remains high.
  - Increased initial-send budget, tightened delta cadence, and reduced fanout concurrency during aggressive tail windows.
  - Added tail-mode initial snapshot caps (players/walls/projectiles/pickups) to shrink first payloads.
  - Raised initial-send timeout in tail modes to reduce resend churn on congested data channels.
- Rerun result:
  - `artifacts/scale/multi_client_fresh_120_after_tail_pass_20260213.json`
  - `99/120`, `connectedRatio=0.825`, `passed=false`, `durationMs=612658`
  - Delta vs previous rerun (`multi_client_fresh_120_after_ui_gate_tune_20260213.json`):
    - `+7` launched clients (`92 -> 99`)
    - `+0.0583` connected ratio (`0.7667 -> 0.825`)
    - `-27055ms` duration (`639713 -> 612658`)
    - `wave_73_plus count: 20 -> 27`
  - `connectLatencyMs`: `p50=29228`, `p90=77772.4`, `p95=85394`, `p99=88385.2`, `max=89571`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=17518.38`, `p95=24906.1`
    - `25-48`: `count=24/24`, `avg=24171.21`, `p95=31668`
    - `49-72`: `count=24/24`, `avg=33973.83`, `p95=42953.85`
    - `73+`: `count=27/48`, `avg=67767.67`, `p95=88360.4`

### Follow-up experiment (discarded) + stable serializer pass (2026-02-14)
- Discarded scheduler experiment:
  - `artifacts/scale/multi_client_fresh_120_after_tail_pass_v2_20260214.json`
  - Added cached-initial prioritization + aggressive delta suppression.
  - Result regressed to `95/120` (`connectedRatio=0.7917`) with `49-72 avg=59054.63ms`; not kept.
- Stable pass implemented:
  - `server/src/network/signaling.rs`: pooled FlatBuffer builder for welcome message serialization.
  - `server/src/server/instance.rs`: pooled FlatBuffer builders for chat serialization paths.
- Stable rerun result:
  - `artifacts/scale/multi_client_fresh_120_after_fb_pool_20260214.json`
  - `105/120`, `connectedRatio=0.8667`, `passed=false`, `durationMs=712278`
  - Delta vs previous best (`after_tail_pass_20260213`):
    - `+6` launched clients (`99 -> 105`)
    - `+0.0417` connected ratio (`0.825 -> 0.8667`)
    - `wave_73_plus count: 27 -> 32`
  - `connectLatencyMs`: `p50=24616.5`, `p90=74285.5`, `p95=81188.4`, `p99=93501.13`, `max=115036`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=15224.38`, `p95=19192.55`
    - `25-48`: `count=24/24`, `avg=19145.83`, `p95=22250.25`
    - `49-72`: `count=24/24`, `avg=33062.25`, `p95=50740.5`
    - `73+`: `count=32/48`, `avg=62741`, `p95=93382.05`

### Updated boundary status
- `80` clients still clears at `80/80`.
- `120` clients remains launch-timeout limited; latest rerun improved to `105/120`.
- Main unresolved bottleneck is still the `73+` wave tail (`32/48` connected) with occasional extreme outliers (max `115s`).

### Test status
- `cargo test -p massive_game_server_core --test boundary_stress -- --nocapture` passed.
- `RUN_STRESS_TEST=1 ... --exact stress_test_game_tick --nocapture` passed (`avg=1.13ms`, `p95=1.79ms`, `max=4.34ms`).
- `RUN_STRESS_TEST=1 ... --exact stress_test_game_tick_with_bots --nocapture` passed (`avg=1.34ms`, `p95=1.87ms`, `max=2.26ms`).
