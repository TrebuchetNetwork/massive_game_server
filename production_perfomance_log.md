# Production Performance Log (Refreshed 2026-02-14)

## Join-Throughput Pass Summary

### Pass objective
- Execute the next join-throughput optimization pass for the remaining `80+` launch timeout.
- Rebaseline with deterministic world generation and refreshed measurements.

### Fresh run artifacts (2026-02-14 evening)
- `artifacts/scale/multi_client_fresh_20_after_ecs_rest_20260214_131633.json`
  - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=85583`, `p95=22862.3`
- `artifacts/scale/multi_client_fresh_100_after_ecs_rest_20260214_134211_fastsample.json`
  - `100/100`, `connectedRatio=1.0`, `timedOutDuringLaunch=false`
  - `73+`: `count=28/28`, `avg=135188`, `p95=154371.4`, `max=156030`
- `artifacts/scale/multi_client_fresh_120_after_ecs_rest_20260214_133241_fastsample.json`
  - `103/120`, `connectedRatio=0.85`, `timedOutDuringLaunch=true`
  - `73+`: `count=30/48`, `avg=133160.67`, `p95=166463.7`, `max=182012`
- Runner caveat:
  - `100`/`120` used `--state-read-timeout-ms 250` to keep sampling deterministic; treat launch counts and `connectLatencyByWave` as the primary signal.

### Phase-2 backend hardening pass (2026-02-14 late PM)
- Lock-free SoA migration expanded to entity snapshots:
  - `server/src/concurrent/atomic_snapshot.rs`
  - added `ProjectileSoASnapshot`/`PickupSoASnapshot` and atomic publishers.
  - broadcast serializers now consume shared entity snapshots through unified lookup helpers.
- Join-stage tracing coverage improved:
  - signaling enqueue + channel-open hooks (`note_join_enqueued`, `note_join_channel_open`).
  - new per-wave metrics: `open_channel_wait_ms`, `send_result_ms`.
  - trace timing moved to microsecond precision internally (reported as ms), removing prior `0ms` quantization for fast paths.
- Join packet transport path tightened:
  - welcome + match-info now sent through the packet batch/coalescing path when data channel opens.
  - match-info serialization now uses zero-copy collapse path when enabled.
- SIMD collision pass expanded:
  - projectile-to-player ray checks now batch candidate target positions and SIMD-test sample points against packed target vectors.
- Smoke artifact after this pass:
  - `artifacts/scale/multi_client_smoke_20_jointrace_us_v2_20260214.json`
  - `20/20`, `connectedRatio=1.0`.
  - `serverJoinStages.wave_1_24`:
    - `open_channel_wait_ms avg=101.65, p95=197.75`
    - `queue_wait_ms avg=111.2, p95=215.7`
    - `snapshot_build_ms avg=0.1, p95=1.0`
    - `send_result_ms` is now measured with microsecond precision (still `0` in this low-load run).

### A/B isolation pass + confirmation (2026-02-14 PM)
- Added runtime toggles for targeted isolation:
  - `MGS_JOIN_DISABLE_TAIL_POLICY`
  - `MGS_JOIN_DISABLE_PACKET_BATCHING`
  - `MGS_JOIN_DISABLE_SOA_SNAPSHOT`
  - `MGS_JOIN_DISABLE_ZERO_COPY_SERIALIZATION`
- Added join-stage report endpoints and runner capture:
  - `GET /api/ops/join-stages`
  - `POST /api/ops/join-stages/reset`
  - `scripts/ui_bench/multi_client.js --join-stage-url ... --reset-join-stages`
- Deterministic `120` matrix (same profile, `600000ms` cap):
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
- Isolation conclusion:
  - tail policy is beneficial; disabling it is the worst result in this matrix.
  - packet batching / zero-copy toggles are not primary regressors.
  - SoA snapshot path is the strongest remaining regression suspect (`+10` launched clients vs `86/120` regressed release retest when disabled).
- Confirmation artifact for `10v10` with chosen setting (SoA off):
  - `artifacts/scale/multi_client_fresh_20_after_soa_off_tail_retest_20260214.json`
  - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=107570`, `p95=21146.4`
- Adaptive fallback tuning on top of SoA isolation:
  - first adaptive trigger (`tail-only/backlog-heavy`) was not kept:
    - `artifacts/scale/multi_client_fresh_120_after_soa_adaptive_fallback_20260214.json`
    - `87/120`, `connectedRatio=0.7250`, `73+=15/48`
  - tuned adaptive trigger (`medium+ join-pressure`, final):
    - `artifacts/scale/multi_client_fresh_120_after_soa_adaptive_fallback_v2_20260214.json`
    - `101/120`, `connectedRatio=0.8417`, `73+=29/48`, `73+ p95=87012`
  - `10v10` sanity:
    - `artifacts/scale/multi_client_fresh_20_after_soa_adaptive_fallback_v2_20260214.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=98109`

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

### Fresh release retest after lock-free/zero-copy/batching pass (2026-02-14)
- New artifacts:
  - `artifacts/scale/multi_client_fresh_20_after_20260214_lockfree_zero_copy_batch_release.json`
  - `artifacts/scale/multi_client_fresh_120_after_20260214_lockfree_zero_copy_batch_release.json`
- `10v10` check (`20` clients):
  - baseline: `artifacts/scale/multi_client_fresh_20_after_dynamic_consistency_opt.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=102680`
  - retest: `artifacts/scale/multi_client_fresh_20_after_20260214_lockfree_zero_copy_batch_release.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=106007`
    - `connectLatencyMs`: `p50=17370.5`, `p95=20401.5`, `max=20525`
  - delta: `durationMs +3327` (`+3.24%`), launch/healthy unchanged.
- Tail-join check (`120` clients, 600000ms cap):
  - baseline (previous best): `artifacts/scale/multi_client_fresh_120_after_fb_pool_20260214.json`
    - `clientsLaunched=105`, `clientsHealthyFinal=104/120`, `connectedRatio=0.8667`, `durationMs=712278`
    - `connectLatencyMs`: `p50=24616.5`, `p95=81188.4`, `max=115036`
    - `73+`: `count=32/48`, `avg=62741`, `p95=93382.05`, `max=115036`
  - retest: `artifacts/scale/multi_client_fresh_120_after_20260214_lockfree_zero_copy_batch_release.json`
    - `clientsLaunched=86`, `clientsHealthyFinal=86/120`, `connectedRatio=0.7167`, `durationMs=650563`
    - `connectLatencyMs`: `p50=36017.5`, `p95=99564.75`, `max=106510`
    - `73+`: `count=14/48`, `avg=86878.36`, `p95=103917.15`, `max=106510`
  - delta (retest vs baseline):
    - `clientsLaunched -19` (`105 -> 86`)
    - `clientsHealthyFinal -18` (`104 -> 86`)
    - `connectedRatio -0.1500` (`0.8667 -> 0.7167`)
    - `p50 +11401`, `p95 +18376.35`
    - `wave_73_plus count -18` (`32 -> 14`)
    - `wave_73_plus avg +24137.36`, `wave_73_plus p95 +10535.10`
- Conclusion: regression confirmed in the `73+` tail; this pass is not the new baseline.

### Updated boundary status
- `80` clients still clears at `80/80`.
- `120` clients remains launch-timeout limited.
- Best known run remains `clientsLaunched=105`, `clientsHealthyFinal=104/120` (`after_fb_pool_20260214`).
- Current recovered candidate is `clientsLaunched=101`, `clientsHealthyFinal=101/120` with `73+=29/48` (`multi_client_fresh_120_after_soa_adaptive_fallback_v2_20260214.json`).

### What is still missing
- Close remaining gap to prior best (`101 -> 105`) and push toward target `>=108`.
- Keep adaptive SoA fallback and further tune tail-wave (`73+`) reliability.
- Keep tail policy enabled; disabling it regresses both launched count and tail latency.
- Drive new stage metrics (`open_channel_wait_ms`, `queue_wait_ms`, `snapshot_build_ms`, `send_result_ms`) through refreshed `120` tail runs and correlate with launch failures.
- If `send_result_ms` remains near-zero in heavy runs, add deeper transport callback instrumentation around data-channel buffered amount / lower-level send completion.

### Test status
- `cargo test -p massive_game_server_core --test boundary_stress -- --nocapture` passed.
- `RUN_STRESS_TEST=1 ... --exact stress_test_game_tick --nocapture` passed (`avg=1.13ms`, `p95=1.79ms`, `max=4.34ms`).
- `RUN_STRESS_TEST=1 ... --exact stress_test_game_tick_with_bots --nocapture` passed (`avg=1.34ms`, `p95=1.87ms`, `max=2.26ms`).
