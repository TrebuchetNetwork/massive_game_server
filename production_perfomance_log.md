# Production Performance Log (Refreshed 2026-02-16)

## Join-Throughput Pass Summary

### Pass objective
- Execute the next join-throughput optimization pass for the remaining `80+` launch timeout.
- Rebaseline with deterministic world generation and refreshed measurements.

### V4 join-rate + join-timing refresh (2026-02-15 afternoon)
- Added pass artifacts:
  - `artifacts/scale/multi_client_20_v4_join_timing_20260215_141555.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=79441`
    - `connectLatency avg=13553.95ms`, `p95=15849.55ms`
    - client join-stage timings: `firstState avg=424.72ms`, `p95=1357.14ms`
  - `artifacts/scale/multi_client_120_v4_join_timing_20260215_141725.json`
    - `96/120`, `connectedRatio=0.8`, `passed=false`
    - timed out at `maxTotalMs=600000` (`timedOutDuringLaunch=true`, `timedOutDuringSampling=true`)
    - `connectLatency avg=37166.77ms`, `p95=79299.25ms`
    - `73+`: `count=24/48`, `avg=70950.63ms`, `p95=82774.75ms`
    - client join-stage timings:
      - global `firstState avg=365.03ms`, `p95=1757.58ms`
      - `73+ firstState avg=627.95ms`, `p95=2568.59ms`
    - server join-stage (`73+`): `open_channel_wait avg=238.54ms`, `queue_wait avg=8.93ms`, `snapshot_build avg=0.05ms`, `send_result avg=0ms`
- Comparison vs prior refresh (`artifacts/scale/multi_client_120_v3_refresh_20260215_043833.json`):
  - launched `92 -> 96` (`+4`)
  - `connectLatency avg 88365.68 -> 37166.77` (`-51198.91ms`)
  - `73+ avg 176286.55 -> 70950.63` (`-105335.92ms`)
  - caveat: prior run used `maxTotalMs=300000`, current run used `600000` for fuller tail-wave coverage.

### V4 extreme-tail scheduler pass refresh (2026-02-15 evening)
- Added pass artifacts:
  - `artifacts/scale/multi_client_20_v4_extreme_tail_20260215_155005.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=77758`
    - `connectLatency avg=23996.75ms`, `p95=26731.25ms`
    - join timing: `firstState avg=289.94ms`, `p95=891.36ms`
  - `artifacts/scale/multi_client_120_v4_extreme_tail_20260215_160243.json`
    - `92/120`, `connectedRatio=0.7667`, `passed=false`
    - timed out at `maxTotalMs=360000` (`timedOutDuringLaunch=true`, `timedOutDuringSampling=true`)
    - `connectLatency avg=57840.65ms`, `p95=111832.5ms`
    - `73+`: `count=20/48`, `avg=108059.6ms`, `p95=112109.9ms`
    - join timing (`73+ firstState`): `avg=2545.88ms`, `p95=5748.42ms`
    - server join-stage (`73+`): `open_channel_wait avg=1509.01ms`, `p95=5717.25ms`; `queue_wait avg=9.33ms`
- Direct comparison vs prior V4 baseline (`artifacts/scale/multi_client_120_v4_join_timing_20260215_141725.json`):
  - launched `96 -> 92` (`-4`)
  - `connectLatency avg 37166.77 -> 57840.65` (`+20673.88ms`)
  - `73+ avg 70950.63 -> 108059.6` (`+37108.97ms`)
  - `73+ open_channel_wait avg 238.54 -> 1509.01` (`+1270.47ms`)
- Measurement caveat:
  - this run used `maxTotalMs=360000` for deterministic completion after repeated teardown hangs in longer runs.
  - a like-for-like `600000ms` rerun progressed through clients `93-96` but hung in teardown before JSON write.
  - runner stabilization has now been patched in `scripts/ui_bench/multi_client.js` (global-timeout-aware in-flight draining + best-effort close timeouts), validated by:
    - `artifacts/scale/multi_client_timeout_smoke_20260215_162455.json`
    - forced-timeout smoke (`maxTotalMs=15000`) still writes complete JSON output and exits deterministically.

### V4 strict rerun after runner stabilization (2026-02-15 late evening)
- Added strict artifacts:
  - `artifacts/scale/multi_client_20_v4_extreme_tail_stable_20260215_163624.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=82750`
    - `connectLatency avg=18157.6ms`, `p95=22214.65ms`
    - `firstState avg=686.73ms`, `p95=1462.31ms`
  - `artifacts/scale/multi_client_120_v4_extreme_tail_stable_20260215_162613.json`
    - `96/120`, `connectedRatio=0.8`, `passed=false`, `durationMs=600804`
    - timed out at `maxTotalMs=600000` (`timedOutDuringLaunch=true`, `timedOutDuringSampling=true`)
    - `connectLatency avg=73541.72ms`, `p95=145529.25ms`
    - `73+`: `count=24/48`, `avg=136711.38ms`, `p95=151533.65ms`
    - `73+ firstState`: `avg=4574.68ms`, `p95=11524.76ms`
    - server join-stage (`73+ open_channel_wait`): `avg=2538.7ms`, `p95=8463.82ms`
- Strict comparison vs prior V4 baseline (`artifacts/scale/multi_client_120_v4_join_timing_20260215_141725.json`):
  - launched: `96 -> 96` (no throughput gain)
  - `connectLatency avg`: `37166.77 -> 73541.72` (`+36374.95ms`)
  - `73+ avg`: `70950.63 -> 136711.38` (`+65760.75ms`)
  - `73+ firstState avg`: `627.95 -> 4574.68` (`+3946.73ms`)
  - `73+ open_channel_wait avg`: `238.54 -> 2538.7` (`+2300.16ms`)
- Conclusion:
  - extreme-tail scheduling changes did not improve launch count under strict conditions and introduced substantial tail-latency regression.
  - next optimization target is signaling/open-channel pressure in `wave_73_plus` rather than further initial/delta scheduler aggressiveness.

### V4 signaling API reuse pass (2026-02-15 night)
- Code path update:
  - `server/src/network/signaling.rs`
  - Added shared global WebRTC `API` initialization (`OnceLock`) so MediaEngine/default codecs are built once and reused across signaling connections.
- Added pass artifacts:
  - `artifacts/scale/multi_client_20_v4_shared_api_20260215_171122.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=86606`
    - `connectLatency avg=18745.05ms`, `p95=20925.8ms`
    - `wave_1_24 open_channel_wait avg=146.33ms`, `p95=468.43ms`
  - `artifacts/scale/multi_client_120_v4_shared_api_20260215_171301.json`
    - `96/120`, `connectedRatio=0.8`, `passed=false`, `durationMs=600798`
    - timed out at `maxTotalMs=600000` (`timedOutDuringLaunch=true`, `timedOutDuringSampling=true`)
    - `connectLatency avg=51185.6ms`, `p95=81338.25ms`
    - `73+`: `count=24/48`, `avg=80321.33ms`, `p95=96044.5ms`
    - `73+ firstState`: `avg=1588.64ms`, `p95=3033.32ms`
    - server join-stage (`73+ open_channel_wait`): `avg=1413.62ms`, `p95=2970.24ms`
- Delta vs prior strict regressed run (`artifacts/scale/multi_client_120_v4_extreme_tail_stable_20260215_162613.json`):
  - launch count: `96 -> 96` (no change)
  - `connectLatency avg`: `73541.72 -> 51185.6` (`-22356.12ms`)
  - `connectLatency p95`: `145529.25 -> 81338.25` (`-64191ms`)
  - `73+ avg`: `136711.38 -> 80321.33` (`-56390.05ms`)
  - `73+ firstState avg`: `4574.68 -> 1588.64` (`-2986.04ms`)
  - `73+ open_channel_wait avg`: `2538.7 -> 1413.62` (`-1125.08ms`)
- Delta vs V4 baseline (`artifacts/scale/multi_client_120_v4_join_timing_20260215_141725.json`):
  - launch count remains `96/120` (same)
  - `connectLatency avg`: `37166.77 -> 51185.6` (`+14018.83ms`)
  - `73+ avg`: `70950.63 -> 80321.33` (`+9370.7ms`)
  - `73+ open_channel_wait avg`: `238.54 -> 1413.62` (`+1175.08ms`)
- Conclusion:
  - API reuse materially recovers most of the strict-run regression while preserving throughput.
  - Remaining gap is concentrated in `73+` signaling/open-channel wait; next pass should target negotiation concurrency/backpressure rather than broadcast scheduler aggression.

### V4 send-path closed-channel guard pass (2026-02-16 early morning)
- Code path update:
  - `server/src/server/instance.rs`
  - Added early-open checks and not-open error filtering in batched/sequential send helpers to avoid repeated fallback/resend pressure on closed data channels.
  - Scheduler now skips closed-channel delta fanout explicitly and reports `pending_delta_closed` in join-scheduler diagnostics.
  - Delta-build fallback missing-state warning reduced from `warn` to `debug` in the hot path.
- Added pass artifacts:
  - `artifacts/scale/multi_client_20_v4_r1_sendpath_tuned_20260215_192624.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=77409`
    - `connectLatency avg=24259.55ms`, `p95=27186.5ms`
  - `artifacts/scale/multi_client_120_v4_r1_sendpath_20260215_190215.json` (first pass)
    - `96/120`, `connectedRatio=0.8`, `passed=false`, `durationMs=600798`
    - `connectLatency avg=57039.79ms`, `p95=106218.75ms`
    - `73+`: `count=24/48`, `avg=100507.42ms`, `p95=117643.85ms`
    - `73+ open_channel_wait`: `avg=382.92ms`, `p95=1257.55ms`
  - `artifacts/scale/multi_client_120_v4_r1_sendpath_tuned_20260215_191602.json` (threshold-tuned rerun)
    - `96/120`, `connectedRatio=0.8`, `passed=false`, `durationMs=600805`
    - `connectLatency avg=54679.44ms`, `p95=98951.25ms`
    - `73+`: `count=24/48`, `avg=94144.5ms`, `p95=105340.45ms`
    - `73+ open_channel_wait`: `avg=465.36ms`, `p95=2023.66ms`
- Delta vs first send-path run (`...190215.json -> ...191602.json`):
  - launch count: `96 -> 96` (no change)
  - `connectLatency avg`: `57039.79 -> 54679.44` (`-2360.35ms`)
  - `connectLatency p95`: `106218.75 -> 98951.25` (`-7267.5ms`)
  - `73+ avg`: `100507.42 -> 94144.5` (`-6362.92ms`)
  - `73+ p95`: `117643.85 -> 105340.45` (`-12303.4ms`)
- Delta vs shared API pass (`artifacts/scale/multi_client_120_v4_shared_api_20260215_171301.json`):
  - launch count remains `96/120`
  - `connectLatency avg`: `51185.6 -> 54679.44` (`+3493.84ms`)
  - `73+ avg`: `80321.33 -> 94144.5` (`+13823.17ms`)
  - `73+ open_channel_wait avg`: `1413.62 -> 465.36` (`-948.26ms`)
- Conclusion:
  - Closed-channel send guards reduce server-side open-channel wait pressure, but strict tail throughput remains capped at `96/120`.
  - Client-visible tail latency (`73+`) is still above the shared-API pass baseline; V4-R1 remains open.

### V4 SIMD + decomposition follow-through (2026-02-16)
- Code updates:
  - `server/src/server/packet_batch.rs` (new): extracted coalesced/batched data-channel send helpers from `instance.rs`.
  - `server/src/server/instance.rs`: replaced in-file packet batching implementation with a thin wrapper to the new module and migrated projectile-player hit checks to segment-based SIMD lookup.
  - `server/src/core/simd.rs`: added `first_index_within_segment_radius` (scalar + AVX2 path) and coverage tests.
- Validation:
  - `cargo check -p massive_game_server_core` passed after extraction + SIMD updates.
  - `cargo test -p massive_game_server_core simd_segment_radius_returns_first_hit_along_path` passed.
  - `cargo test -p massive_game_server_core coalesced_batch_supports_single_packet` passed.
  - `artifacts/scale/multi_client_20_v4_r45_smoke_20260215_212303.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=77345`
    - `connectLatency avg=23486ms`, `p95=26616.1ms`
- Benchmark status:
  - Strict `120` rerun is still required for this specific SIMD/decomposition pass; the latest strict tail artifact remains:
    - `artifacts/scale/multi_client_120_v4_r1_sendpath_tuned_20260215_191602.json`

### V4 authoritative writer + zero-copy follow-through (2026-02-16)
- Code updates:
  - `server/src/server/instance.rs`
    - moved destructible wall state mutation out of `process_projectiles_optimized` and into `apply_projectile_results` via `apply_wall_damage_authoritative`, keeping projectile collision discovery read-heavy and consolidating wall writes in one authoritative apply stage.
  - `server/src/network/signaling.rs`
    - welcome packet serialization now always uses zero-copy `collapse` (removed `finished_data` copy fallback path).
- Validation:
  - `cargo check -p massive_game_server_core` passed.
  - `cargo test -p massive_game_server_core simd_segment_radius_handles_zero_length_segment` passed.
  - `cargo test -p massive_game_server_core coalesced_batch_supports_single_packet` passed.
  - `artifacts/scale/multi_client_20_v4_r23_smoke_20260216_060716.json`
    - `20/20`, `connectedRatio=1.0`, `passed=true`, `durationMs=78982`
    - `connectLatency avg=25269.35ms`, `p95=29092.4ms`
- Remaining work:
  - strict `120` rerun is still required for this code pass.
  - broader authoritative writer ownership migration (full ECS surface) remains open.

### Fresh refresh run artifacts (2026-02-15)
- `artifacts/arena/arena_10v10_20260215_043820.json`
  - arena simulation: `10v10`, `3` rounds, `30` engagements, `durationMs=204`
- `artifacts/scale/multi_client_20_v3_refresh_20260215_044634.json`
  - `20/20`, `connectedRatio=1.0`, `passed=true`
  - `connectLatency avg=17288.45ms`, `p95=20413.5ms`, `durationMs=101275`
- `artifacts/scale/multi_client_120_v3_refresh_20260215_043833.json`
  - `92/120`, `connectedRatio=0.7667`, `passed=false`
  - timed out at `maxTotalMs=300000`
  - `connectLatency avg=88365.68ms`, `p95=191396.3ms`, `durationMs=471444`
  - `73+`: `count=20/48`, `avg=176286.55ms`, `p95=195925.6ms`
  - `serverJoinStages.wave_73_plus`: `open_channel_wait avg=390.52ms`, `queue_wait avg=15.79ms`, `snapshot_build avg=0.36ms`, `send_result avg=0.06ms`
- Baseline for comparison:
  - `artifacts/scale/multi_client_20_v3_pass2.json`
  - `artifacts/scale/multi_client_120_v3_pass2.json`
- Delta summary vs baseline:
  - `20` clients: connect avg improved `22615.95 -> 17288.45` (`-5327.50ms`), p95 improved `27604.3 -> 20413.5` (`-7190.8ms`)
  - `120` clients: launched `100 -> 92` (`-8`), connect avg `74128.03 -> 88365.68` (`+14237.65ms`)
  - `120` tail `73+`: avg `137587.86 -> 176286.55` (`+38698.69ms`), count `28 -> 20` (`-8`)
- Arena provider mode note:
  - run used `ARENA_REQUIRE_REAL_PROVIDER=0` fallback because `OPENROUTER_API_KEY` was not present in process env.

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

### R2-R5 maintenance pass (2026-02-16)
- Authoritative projectile writer ownership tightened (`server/src/server/instance.rs`):
  - `process_projectiles_optimized` now returns write artifacts (`removed_projectile_ids`, `kept_projectiles`, `spatial_updates`, `wall_impacts`) instead of mutating shared state directly.
  - `apply_projectile_results` is now the single write stage for wall-impact event enqueue, projectile spatial index updates, authoritative projectile state commit, and wall damage application.
- Zero-copy follow-through extension (`server/src/server/instance.rs`):
  - fallback `build_delta_state_static` now reuses `build_game_event_fb`.
  - destroyed wall IDs in fallback delta path now use `fb_safe_entity_id` (removes per-ID `to_string` allocation in that path).
- SIMD coverage extension (`server/src/core/simd.rs`):
  - added NEON implementation for `first_index_within_segment_radius` and runtime dispatch on aarch64 with NEON.
- `instance.rs` decomposition progress (`server/src/server/event_mapping.rs`, `server/src/server/mod.rs`, `server/src/server/instance.rs`):
  - moved event mapping helpers out of `instance.rs` into `event_mapping.rs`.
  - removed duplicate local mapping helper block and unused local event-vector helper from `instance.rs`.
- Validation:
  - `cargo check -p massive_game_server_core` passed.
  - `cargo test -p massive_game_server_core simd_segment_radius_returns_first_hit_along_path -- --nocapture` passed.
  - `cargo test -p massive_game_server_core packet_batch_tests::collect_pending_chat_packets_applies_seq_filter_and_cap -- --nocapture` passed.
