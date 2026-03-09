# Kimi Recommendation Task Board (2026-02-14)

## Source Files Ingested
- `artifacts/kimi_recommendation/IMPLEMENTATION_GUIDE.md`
- `artifacts/kimi_recommendation/LLM_BOT_ARENA_IMPLEMENTATION.md`
- `artifacts/kimi_recommendation/MEGA_BATTLE_EXECUTIVE_SUMMARY.md`
- `artifacts/kimi_recommendation/MEGA_BATTLE_GAME_DESIGN.md`
- `artifacts/kimi_recommendation/README_SINGLE_SERVER.md`
- `artifacts/kimi_recommendation/SINGLE_SERVER_PURE_RUST_OPTIMIZATIONS.md`
- `artifacts/kimi_recommendation/SYSTEM_ARCHITECTURE.md`
- `artifacts/kimi_recommendation/TREBUCHET_IMPROVEMENTS_MASTER.md`
- `artifacts/kimi_recommendation/massive_game_client_analysis.md`
- `artifacts/kimi_recommendation/massive_game_server_improvements.md`
- `artifacts/kimi_recommendation/scalability_analysis.md`
- `artifacts/kimi_recommendation/scalability_analysis_part1.md`
- `artifacts/kimi_recommendation/scalability_analysis_part2.md`

## Deduplicated Delivery Tracks
| Track | Scope | Status |
|---|---|---|
| T1 | Single-machine core loop (lock-free + SoA + SIMD + zero-copy + batching) | In Progress |
| T2 | Frontend 60 FPS + mobile UX + combat clarity | In Progress |
| T3 | LLM bot arena + leaderboard + match scheduler | In Progress |
| T4 | Infra scale-out (K8s, sharding, regions, DR) | Planned |
| T5 | Competitive gameplay systems (ranked, tournaments, social) | Planned |

## Task Matrix
| ID | Task | Source | Status | Notes |
|---|---|---|---|---|
| T1-01 | Lock-free ECS + SoA migration | IMPLEMENTATION_GUIDE, SINGLE_SERVER_PURE_RUST_OPTIMIZATIONS | In Progress | Core SoA snapshots plus lock-free AoI snapshot publication are wired into broadcast reads; full writer ownership migration still open. |
| T1-02 | SIMD spatial/AOI path | IMPLEMENTATION_GUIDE, TREBUCHET 1.3 | In Progress | SIMD filter + spatial index path exists; keep expanding AOI usage coverage. |
| T1-03 | Zero-copy serialization | IMPLEMENTATION_GUIDE, TREBUCHET 1.9 | In Progress | Chat/welcome/match-info paths use zero-copy bytes; initial/delta build path now caches retry payloads and removes async serializer overhead, but full authoritative snapshot construction still partially copying. |
| T1-04 | Priority packet batching/coalescing | TREBUCHET 1.6 | In Progress | Coalesced batch path now applies a unified envelope format for single and multi-packet outbound dispatch; tail wave tuning still required. |
| T1-05 | SIMD physics/collision | SINGLE_SERVER_PURE_RUST_OPTIMIZATIONS 4.1 | In Progress | Projectile-vs-player ray collision now uses SIMD batched target checks; broader physics vectorization remains. |
| T1-06 | CPU affinity and kernel tuning checklist | IMPLEMENTATION_GUIDE, README_SINGLE_SERVER | In Progress | Affinity + setup scripts exist; ops verification loop continues. |
| T1-07 | Tail-wave instrumentation (1-24/25-48/49-72/73+) | Prior pass + Kimi guidance | Done | Latency buckets and wave metrics are in benchmark artifacts. |
| T1-08 | Recover 120-client tail regression | Benchmark refresh findings | Done (target met) | Latest run reached 100/120 within 300s (`100 is fine` target met); stability and wave 1-48 latency variance tuning remain follow-ups. |
| T2-01 | WebGPU + WebGL2 instanced shader fallback | MEGA_BATTLE_GAME_DESIGN 3.1 | Done | High-player instance rendering path supports non-WebGPU fallback. |
| T2-02 | Projectile instancing and render culling | massive_game_client_analysis 1.2 | In Progress | Instanced layers in place; explicit distance LOD tiers, far-dot culling stride, and projectile pool telemetry are now wired for stress validation. |
| T2-03 | Background tab throttling | TREBUCHET 2.10 | Done | Visibility-driven throttling path implemented. |
| T2-04 | Mobile control polish (sticky/autofire/adaptive) | massive_game_client_analysis 5.x | In Progress | Mobile UX improvements partially integrated; additional tuning queued. |
| T2-05 | Combat clarity UI (hitmarkers/streak/radial/objective) | MEGA_BATTLE_EXECUTIVE_SUMMARY | In Progress | Multiple cues integrated; polish/consistency pass remains. |
| T2-06 | Performance overlay and network profiler | massive_game_client_analysis 8.x | Done | Added live network profiler overlay with rx throughput, packet/message rate, state update cadence, snapshot jitter/interpolation delay, DC buffer pressure, settings toggle, and e2e metric exposure. |
| T2-07 | Code splitting and asset streaming | massive_game_client_analysis 6.x | Planned | Monolithic client split remains future architecture item. |
| T3-01 | LLM model registry API | LLM_BOT_ARENA_IMPLEMENTATION 1.x | Done | `/api/arena/models/register`, `/api/arena/models`, heartbeat. |
| T3-02 | Match queue + round-robin scheduler API | LLM_BOT_ARENA_IMPLEMENTATION 5.x | Done | `/api/arena/matches/queue`, `/queue_round_robin`, `/claim_next`, `/pending`. |
| T3-03 | ELO leaderboard pipeline | LLM_BOT_ARENA_IMPLEMENTATION 4.x | Done | `/api/arena/leaderboard`, `/api/arena/matches/report` updates ratings. |
| T3-04 | Arena persistent store | LLM_BOT_ARENA_IMPLEMENTATION 6.x | Done | JSON persistence at `data/arena_store.json` (`MGS_ARENA_STORE_PATH` override). |
| T3-05 | WASM bot sandbox runtime | LLM_BOT_ARENA_IMPLEMENTATION 2.x | Done | `bot_sandbox.rs` executes deterministic WASM matches with fuel limits, fallback runtime, and per-tick replay capture hooks. |
| T3-06 | Code generation/validator integration | LLM_BOT_ARENA_IMPLEMENTATION 1.x, 3.x | Done | `code_generation.rs` ships validate/generate/generate+compile APIs with OpenRouter integration and deterministic fallback templates. |
| T4-01 | Redis cache expansion beyond auth | TREBUCHET 3.4 | Planned | Auth path uses Redis; gameplay caching layer pending. |
| T4-02 | Feature flags + A/B experimentation controls | scalability_analysis_part2 18 | Done | Runtime flag service and deterministic rollout evaluator added via `/api/ops/feature-flags/*`. |
| T4-03 | Match sharding/world partition authority | scalability_analysis 1-2 | Planned | Current focus remains single-machine optimization. |
| T4-04 | Kubernetes/Helm autoscaling stack | scalability_analysis 7-10 | Planned | Deferred until single-node 120+ target stabilizes. |
| T4-05 | Multi-region + DR + chaos testing | scalability_analysis 8,13,20 | Planned | Phase after stable regional baseline. |
| T5-01 | Multi-mode/ranked/tournament systems | massive_game_server_improvements 1,8,9 | Planned | Requires durable profile/match history hardening first. |
| T5-02 | Social systems (friends/party/clan) | massive_game_server_improvements 5,6,15 | Planned | API scaffolding follows identity/stats expansion. |
| T5-03 | Spectator/replay pipeline | massive_game_server_improvements 10 | Done | Event-sourced arena replay pipeline now includes per-tick replay capture, bounded replay event history, per-match event retrieval (`/api/arena/replays/{match_id}/events`), recent event feed (`/api/arena/replays/events/recent`), and live SSE stream (`/api/arena/replays/stream`). |
| T5-04 | Battle pass/challenges/progression economy | massive_game_server_improvements 2,13,14 | Planned | Use auth profile as starting anchor. |

## Implemented In This Pass
- Reduced signaling hot-path setup overhead for large launch waves:
  - `server/src/network/signaling.rs`
  - introduced shared global WebRTC `API` initialization (`OnceLock`) so MediaEngine/default codec setup is reused instead of rebuilt per signaling connection.
- Fresh strict artifacts for this pass:
  - `artifacts/scale/multi_client_20_v4_shared_api_20260215_171122.json`
  - `artifacts/scale/multi_client_120_v4_shared_api_20260215_171301.json`
  - result: `120` run remains `96/120` but recovers major tail-latency regression (`73+ open_channel_wait avg 2538.7 -> 1413.62` vs prior strict run).
- Added arena operational module and routes:
  - `server/src/operational/arena.rs`
  - `server/src/operational/mod.rs`
  - `server/src/main.rs`
- Added API surface:
  - `POST /api/arena/models/register`
  - `POST /api/arena/models/heartbeat`
  - `GET /api/arena/models`
  - `POST /api/arena/matches/queue`
  - `POST /api/arena/matches/queue_round_robin`
  - `POST /api/arena/matches/claim_next`
  - `GET /api/arena/matches/pending`
  - `POST /api/arena/matches/report`
  - `GET /api/arena/leaderboard`
  - `GET /api/arena/overview`
- Added arena persistence and ELO update logic with unit tests.
- Added feature flag operational module and routes:
  - `GET /api/ops/feature-flags`
  - `POST /api/ops/feature-flags/set`
  - `POST /api/ops/feature-flags/evaluate`
  - env bootstrap via `MGS_FEATURE_FLAGS`
  - deterministic subject rollout bucketing.
- Added lock-free SoA snapshots for projectiles/pickups and wired shared broadcast lookup helpers.
- Extended join-stage tracing with signaling enqueue/open hooks + `open_channel_wait_ms` / `send_result_ms` metrics.
- Switched join-stage internal timing precision to microseconds (reported in ms).
- Expanded SIMD projectile-player collision checks with batched target vectors.
- Routed welcome + match-info join packets through packet coalescing path.
- Ran fresh benchmark refresh after human-priority balancing pass:
  - `artifacts/scale/multi_client_fresh_20_post_team_balanced_20260214_2038.json`
    - 20/20 launched, connected ratio `1.00`, connect avg `21938.8ms` (prior `18487.7ms`).
  - `artifacts/scale/multi_client_fresh_120_post_team_balanced_20260214_2050.json`
    - 86/120 launched within `300000ms` cap, connected ratio `0.7167`.
    - 73+ wave: count `14`, avg `88631.71ms`, p95 `92412.3ms`.
    - join-stage 73+: `open_channel_wait_ms.avg=356.27` (`p95=1883.85`), `queue_wait_ms.avg=9.33`.
  - Notes:
    - A first 120 run with a `420000ms` cap hung without writing an artifact; this is now tracked as tool/runtime instability during extreme tail load.
    - Tail regression remains unresolved; 120-client launch coverage is still below target.
- Added kill-feed slot reclaim visibility for human-priority joins:
  - slot reclaim now emits both system chat and kill-feed entries.
- Applied a tuned 70+ tail scheduler pass and reran `120`:
  - `artifacts/scale/multi_client_fresh_120_tail_tuned_v2_20260214_2247.json`
    - launched `87/120` (from `86/120`), connected ratio `0.725`.
    - 73+ wave avg `79709.27ms` (from `88631.71ms`), p95 `83942.1ms` (from `92412.3ms`).
    - 73+ `open_channel_wait_ms`: avg `253.99` (from `356.27`), p95 `1178.84` (from `1883.85`).
- Added authoritative AoI snapshot routing in broadcast serialization:
  - `server/src/server/instance.rs`
  - now default-on (`MGS_JOIN_DISABLE_AUTHORITATIVE_AOI_SNAPSHOT=1` to opt out).
  - broadcast admission checks now rely on shared snapshot player visibility.
- Unified packet batching follow-through:
  - `process_client_broadcast` now applies the same pending chat packet batching/resend follow-through for both initial and delta sends.
- Added lock-free AoI snapshot publication + unified outbound envelope follow-through:
  - `server/src/concurrent/atomic_snapshot.rs`
  - `server/src/server/instance.rs`
  - AoI scheduled fanout reads now use authoritative lock-free snapshot publication (`AtomicPlayerAoISnapshot`).
  - AoI snapshot publish runs after `synchronize_state` to keep broadcast reads frame-aligned.
  - Packet coalescing now supports single-packet envelopes, unifying outbound transport shape.
  - Initial snapshot bytes are cached immediately on build for retry reuse without re-serialization.
- Tightened authoritative snapshot ownership on active broadcast path:
  - Removed fanout-time SoA/AoI snapshot rebuild fallback from live world structures when snapshots are empty.
  - Initial-state wall fallback now uses shared per-broadcast wall snapshots (`active_walls_by_id`) instead of live collection.
- Fresh benchmark artifacts for this pass:
  - `artifacts/scale/multi_client_fresh_20_aoi_snapshot_20260214_2310.json`
    - 20/20 launched, connected ratio `1.00`, connect avg `21001.65ms`.
  - `artifacts/scale/multi_client_fresh_120_aoi_snapshot_20260214_2311.json`
    - 73/120 launched, connected ratio `0.6083`, 73+ count `1`.
  - `artifacts/scale/multi_client_fresh_120_aoi_snapshot_fresh_20260214_2318.json`
    - 73/120 launched, connected ratio `0.6083`, 73+ count `1`.
  - `artifacts/scale/multi_client_fresh_120_aoi_snapshot_disabled_default_20260214_2329.json`
    - 72/120 launched, connected ratio `0.60`, 73+ count `0`.
- Release recheck artifacts (same code path validation):
  - `artifacts/scale/multi_client_fresh_20_release_20260214_2342.json`
    - 20/20 launched, connected ratio `1.00`, connect avg `20654.9ms`.
  - `artifacts/scale/multi_client_fresh_120_release_recheck_20260214_2344.json`
    - 97/120 launched, connected ratio `0.8083`, 73+ count `25` (best in this batch).
  - `artifacts/scale/multi_client_fresh_120_tail_tune3_20260214_2356.json`
    - 90/120 launched, connected ratio `0.75`; experimental tail-tuning variant was not retained.
  - `artifacts/scale/multi_client_fresh_120_closed_delta_skip_20260215_0007.json`
    - 93/120 launched, connected ratio `0.775`; variant was not retained after comparison.
  - `artifacts/scale/multi_client_fresh_20_missingpass_20260215_0025.json`
    - 20/20 launched, connected ratio `1.00`, connect avg `17206.2ms`.
  - `artifacts/scale/multi_client_fresh_120_missingpass_20260215_0026.json`
    - 95/120 launched, connected ratio `0.7917`.
  - `artifacts/scale/multi_client_fresh_120_missingpass_aoi_on_20260215_0035.json`
    - 100/120 launched, connected ratio `0.8333` (phase target met).
  - `artifacts/scale/multi_client_fresh_120_phase2_20260214_195643.json`
    - 100/120 launched, connected ratio `0.8333` after lock-free AoI snapshot + unified envelope pass.
  - `artifacts/scale/multi_client_fresh_120_phase2_complete_20260214_210222.json`
    - 100/120 launched, connected ratio `0.8333` after strict snapshot-only broadcast pass.
    - connect avg `61116.58ms` (from `63885.71ms` in prior phase2 run).

## Next Execution Batch
1. Stabilize `100/120`+ coverage across repeated runs (reduce variance and improve p95 consistency).
2. Investigate wave `1-48` `open_channel_wait_ms` multi-second p95 spikes and tune signaling/open-channel backpressure.
3. Continue writer-side authoritative ownership migration (full ECS mutation boundaries) and remove remaining full-state serialization copy points.
