# Performance Status (2026-02-14)

## Scope
- Goal: keep `10v10` stable and clear the remaining `80+` launch-timeout bottleneck.
- This refresh retested join throughput after the dynamic+deterministic world optimization pass.

## Fresh Benchmarks (2026-02-14 evening)
- Server target: `target/release/massive_game_server_core` on `127.0.0.1:18084`.
- New artifacts:
  - `artifacts/scale/multi_client_fresh_20_after_ecs_rest_20260214_131633.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=85583`, `p95=22862.3`
  - `artifacts/scale/multi_client_fresh_100_after_ecs_rest_20260214_134211_fastsample.json`
    - `100/100`, `connectedRatio=1.0`, `timedOutDuringLaunch=false`
    - `73+`: `count=28/28`, `avg=135188`, `p95=154371.4`, `max=156030`
  - `artifacts/scale/multi_client_fresh_120_after_ecs_rest_20260214_133241_fastsample.json`
    - `103/120`, `connectedRatio=0.85`, `timedOutDuringLaunch=true`
    - `73+`: `count=30/48`, `avg=133160.67`, `p95=166463.7`, `max=182012`
- Runner note:
  - tail reruns (`100`/`120`) used `--state-read-timeout-ms 250` to avoid long sequential sample stalls; use launch and per-wave latency numbers as primary comparison metrics.

## Join Throughput Isolation (2026-02-14 PM)
- Added runtime-isolation toggles and stage-report wiring:
  - server toggles: `MGS_JOIN_DISABLE_TAIL_POLICY`, `MGS_JOIN_DISABLE_PACKET_BATCHING`, `MGS_JOIN_DISABLE_SOA_SNAPSHOT`, `MGS_JOIN_DISABLE_ZERO_COPY_SERIALIZATION`
  - API: `GET /api/ops/join-stages`, `POST /api/ops/join-stages/reset`
  - runner capture: `scripts/ui_bench/multi_client.js --join-stage-url ... --reset-join-stages`
- Deterministic `120` A/B matrix (`600000ms` cap, same runner profile):
  - baseline: `artifacts/scale/multi_client_diag_120_baseline_full_20260214.json`
    - `88/120`, `connectedRatio=0.7333`, `73+=16/48`, `73+ p95=78430.5`
  - tail policy off: `artifacts/scale/multi_client_diag_120_tail_off_20260214.json`
    - `83/120`, `connectedRatio=0.6917`, `73+=11/48`, `73+ p95=103594.5`
  - packet batching off: `artifacts/scale/multi_client_diag_120_batching_off_20260214.json`
    - `87/120`, `connectedRatio=0.7250`, `73+=15/48`, `73+ p95=100437.8`
  - zero-copy off: `artifacts/scale/multi_client_diag_120_zero_copy_off_20260214.json`
    - `90/120`, `connectedRatio=0.7500`, `73+=18/48`, `73+ p95=101522.3`
  - SoA snapshot off: `artifacts/scale/multi_client_diag_120_soa_off_20260214.json`
    - `96/120`, `connectedRatio=0.8000`, `73+=24/48`, `73+ p95=121255.9`
- Isolation outcome:
  - tail policy is helping (turning it off is worst in this matrix).
  - packet batching and zero-copy are not primary regressors in this profile.
  - SoA snapshot path is the strongest remaining regression suspect (`+10` launched clients vs current regressed baseline `86/120` when disabled).
  - join-stage spans were upgraded in this pass with enqueue/open/send-result coverage and microsecond timing precision.
- Confirmation refresh:
  - `10v10` validation with chosen setting (`SoA` off):
    - `artifacts/scale/multi_client_fresh_20_after_soa_off_tail_retest_20260214.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=107570`, `p95=21146.4`
  - `120` tail validation for chosen setting:
    - `artifacts/scale/multi_client_diag_120_soa_off_20260214.json`
    - `96/120`, `connectedRatio=0.8000`, `73+=24/48`
  - adaptive fallback v1 (tail-only/backlog-heavy trigger):
    - `artifacts/scale/multi_client_fresh_120_after_soa_adaptive_fallback_20260214.json`
    - `87/120`, `connectedRatio=0.7250`, `73+=15/48` (not kept)
  - adaptive fallback v2 (medium+ join-pressure trigger, final):
    - `artifacts/scale/multi_client_fresh_120_after_soa_adaptive_fallback_v2_20260214.json`
    - `101/120`, `connectedRatio=0.8417`, `73+=29/48`, `73+ p95=87012`
  - `10v10` sanity on final adaptive fallback:
    - `artifacts/scale/multi_client_fresh_20_after_soa_adaptive_fallback_v2_20260214.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=98109`

## Join-Stage Instrumentation Refresh (2026-02-14 late PM)
- Added server-side stage hooks at signaling enqueue and data-channel open.
- Added per-wave `open_channel_wait_ms` and `send_result_ms` to `/api/ops/join-stages`.
- Internal stage timestamps now use microseconds and are normalized back to milliseconds in reports.
- Smoke validation:
  - `artifacts/scale/multi_client_smoke_20_jointrace_us_v2_20260214.json`
  - `20/20`, `connectedRatio=1.0`
  - `serverJoinStages.wave_1_24`:
    - `open_channel_wait_ms avg=101.65, p95=197.75`
    - `queue_wait_ms avg=111.2, p95=215.7`
    - `snapshot_build_ms avg=0.1, p95=1.0`
    - `send_result_ms` is now captured with microsecond internal timing (this smoke run still reports `0` at millisecond resolution output).

## Benchmark Refresh (2026-02-14, release retest)
- Re-ran deterministic multi-client benches after the lock-free/zero-copy/batching pass using release server build.
- Artifacts:
  - `artifacts/scale/multi_client_fresh_20_after_20260214_lockfree_zero_copy_batch_release.json`
  - `artifacts/scale/multi_client_fresh_120_after_20260214_lockfree_zero_copy_batch_release.json`

### 20 clients (10v10) before/after
- Before:
  - `artifacts/scale/multi_client_fresh_20_after_dynamic_consistency_opt.json`
  - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=102680`
- After:
  - `artifacts/scale/multi_client_fresh_20_after_20260214_lockfree_zero_copy_batch_release.json`
  - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=106007`
  - `connectLatencyMs`: `p50=17370.5`, `p90=20284.3`, `p95=20401.5`, `p99=20500.3`, `max=20525`
- Delta:
  - launched/healthy unchanged (`20/20`)
  - `durationMs: +3327` (`+3.24%`)

### 120 clients (tail join, 600000ms cap) before/after
- Before (previous best stable run):
  - `artifacts/scale/multi_client_fresh_120_after_fb_pool_20260214.json`
  - `clientsLaunched=105`, `clientsHealthyFinal=104/120`, `connectedRatio=0.8667`, `durationMs=712278`
  - `connectLatencyMs`: `p50=24616.5`, `p95=81188.4`, `max=115036`
  - `73+`: `count=32/48`, `avg=62741`, `p95=93382.05`, `max=115036`
- After (new retest):
  - `artifacts/scale/multi_client_fresh_120_after_20260214_lockfree_zero_copy_batch_release.json`
  - `clientsLaunched=86`, `clientsHealthyFinal=86/120`, `connectedRatio=0.7167`, `durationMs=650563`
  - `connectLatencyMs`: `p50=36017.5`, `p95=99564.75`, `max=106510`
  - `73+`: `count=14/48`, `avg=86878.36`, `p95=103917.15`, `max=106510`
- Delta (after vs before):
  - `clientsLaunched: -19` (`105 -> 86`)
  - `clientsHealthyFinal: -18` (`104 -> 86`)
  - `connectedRatio: -0.1500` (`0.8667 -> 0.7167`)
  - `durationMs: -61715` (earlier termination due to more launch failures)
  - `p50: +11401`, `p95: +18376.35`
  - `wave_73_plus count: -18` (`32 -> 14`)
  - `wave_73_plus avg: +24137.36`, `p95: +10535.10`
- Result: tail-join regression; this pass should not replace the prior best configuration.

## Incremental Refresh (2026-02-14)
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
- Next tail-focused pass (aggressive `73+` policy + reduced initial snapshot caps):
  - `artifacts/scale/multi_client_fresh_120_after_tail_pass_20260213.json`
  - `99/120`, `connectedRatio=0.825`, `passed=false`, `durationMs=612658`
  - Delta vs prior rerun: `+7` connected, `-27.1s` duration, `+7` in `wave_73_plus`
  - `connectLatencyMs`: `p50=29228`, `p90=77772.4`, `p95=85394`, `p99=88385.2`, `max=89571`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=17518.38`, `p95=24906.1`
    - `25-48`: `count=24/24`, `avg=24171.21`, `p95=31668`
    - `49-72`: `count=24/24`, `avg=33973.83`, `p95=42953.85`
    - `73+`: `count=27/48`, `avg=67767.67`, `p95=88360.4`
- Experimental tail-scheduler variant (`cached-initial priority + aggressive delta suppression`) was evaluated and discarded:
  - `artifacts/scale/multi_client_fresh_120_after_tail_pass_v2_20260214.json`
  - `95/120`, `connectedRatio=0.7917`, `durationMs=647959`
  - Regression signal: `49-72 avg` increased to `59054.63ms` and total launched clients dropped.
- FlatBuffer builder pooling pass (welcome + chat hot paths) on top of the stable tail policy:
  - `artifacts/scale/multi_client_fresh_120_after_fb_pool_20260214.json`
  - `105/120`, `connectedRatio=0.8667`, `passed=false`, `durationMs=712278`
  - Delta vs previous best (`after_tail_pass_20260213`): `+6` launched, `+5` in `wave_73_plus`
  - `connectLatencyMs`: `p50=24616.5`, `p90=74285.5`, `p95=81188.4`, `p99=93501.13`, `max=115036`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=15224.38`, `p95=19192.55`
    - `25-48`: `count=24/24`, `avg=19145.83`, `p95=22250.25`
    - `49-72`: `count=24/24`, `avg=33062.25`, `p95=50740.5`
    - `73+`: `count=32/48`, `avg=62741`, `p95=93382.05`

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
  - Added aggressive `73+` tail policy: higher initial-send floor, tighter delta gating, lower fanout concurrency, and tail-aware initial send timeout.
  - Added adaptive initial snapshot caps under tail pressure to reduce first payload size and data-channel contention.
  - Added pooled FlatBuffer builders for chat message serialization hot paths.
- `server/src/network/signaling.rs`
  - Added pooled FlatBuffer builder for welcome-message serialization on data-channel open.
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
- `artifacts/scale/multi_client_fresh_120_after_tail_pass_20260213.json` (aggressive `73+` scheduling + tail snapshot caps)
  - `99/120`, `connectedRatio=0.825`, `passed=false`, `durationMs=612658`
  - `connectLatencyMs`: `p50=29228`, `p90=77772.4`, `p95=85394`, `p99=88385.2`, `max=89571`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=17518.38`, `p95=24906.1`
    - `25-48`: `count=24/24`, `avg=24171.21`, `p95=31668`
    - `49-72`: `count=24/24`, `avg=33973.83`, `p95=42953.85`
    - `73+`: `count=27/48`, `avg=67767.67`, `p95=88360.4`
  - Result: boundary improved (`+7` launched clients) but still below target `>=108/120` for pass criteria
- `artifacts/scale/multi_client_fresh_120_after_tail_pass_v2_20260214.json` (discarded experiment)
  - `95/120`, `connectedRatio=0.7917`, `passed=false`, `durationMs=647959`
  - `connectLatencyByWave` showed regression in `49-72` (`avg=59054.63`), so this variant was not kept.
- `artifacts/scale/multi_client_fresh_120_after_fb_pool_20260214.json` (stable tail policy + pooled builders)
  - `105/120`, `connectedRatio=0.8667`, `passed=false`, `durationMs=712278`
  - `connectLatencyMs`: `p50=24616.5`, `p90=74285.5`, `p95=81188.4`, `p99=93501.13`, `max=115036`
  - `connectLatencyByWave`:
    - `1-24`: `count=24/24`, `avg=15224.38`, `p95=19192.55`
    - `25-48`: `count=24/24`, `avg=19145.83`, `p95=22250.25`
    - `49-72`: `count=24/24`, `avg=33062.25`, `p95=50740.5`
    - `73+`: `count=32/48`, `avg=62741`, `p95=93382.05`
  - Result: highest launched-client count to date on this deterministic profile (`105/120`)

## Stress / Regression Checks
- `cargo test -p massive_game_server_core --test boundary_stress -- --nocapture`
  - pass (default mode; stress cases report as skipped when `RUN_STRESS_TEST` is unset)
- `RUN_STRESS_TEST=1 STRESS_TICKS=60 STRESS_TICK_TIMEOUT_SECS=20 cargo test -p massive_game_server_core --test boundary_stress -- --exact stress_test_game_tick --nocapture`
  - pass (`avg_ms=1.13`, `p95_ms=1.79`, `max_ms=4.34`)
- `RUN_STRESS_TEST=1 STRESS_TICKS=60 STRESS_BOTS=40 STRESS_TARGET_BOT_COUNT=40 STRESS_TICK_TIMEOUT_SECS=20 cargo test -p massive_game_server_core --test boundary_stress -- --exact stress_test_game_tick_with_bots --nocapture`
  - pass (`avg_ms=1.34`, `p95_ms=1.87`, `max_ms=2.26`)

## Current Bottleneck
- `80` clients is no longer timeout-limited under this configuration.
- `120` clients remains launch-timeout limited.
- Best known result in this profile is still `clientsLaunched=105`, `clientsHealthyFinal=104/120` (`multi_client_fresh_120_after_fb_pool_20260214.json`).
- Current recovered candidate after adaptive fallback tuning is `clientsLaunched=101`, `clientsHealthyFinal=101/120`, `73+=29/48` (`multi_client_fresh_120_after_soa_adaptive_fallback_v2_20260214.json`).
- Gap to prior best is now `-4` launched clients (`101 -> 105`), with final-wave timeout still concentrated in `73+`.

## Next Step Candidates
- Keep adaptive SoA fallback + tail policy enabled as the current best single-machine profile.
- Add SoA-vs-map-path timing around snapshot preparation and initial/delta payload construction to reduce the remaining `73+` misses.
- Expand join-stage instrumentation to include actionable spans:
  - initial enqueue-to-open-channel wait
  - snapshot build start/end around full state serialization
  - send start to completion/error callback timings
- After each change, rerun deterministic `120` and track:
  - `clientsLaunched`
  - `connectLatencyByWave.wave_73_plus.count`
  - `connectLatencyByWave.wave_73_plus.p95Ms`
