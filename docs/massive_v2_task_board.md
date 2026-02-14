# Massive V2 Recommendation Task Board (2026-02-14)

## Source Files Ingested
- `artifacts/massive_v2_recommendation/CODE_REVIEW_LATEST.md`
- `artifacts/massive_v2_recommendation/IMPLEMENTATION_GUIDE.md`
- `artifacts/massive_v2_recommendation/LLM_BOT_ARENA_IMPLEMENTATION.md`
- `artifacts/massive_v2_recommendation/MEGA_BATTLE_EXECUTIVE_SUMMARY.md`
- `artifacts/massive_v2_recommendation/MEGA_BATTLE_GAME_DESIGN.md`
- `artifacts/massive_v2_recommendation/README_SINGLE_SERVER.md`
- `artifacts/massive_v2_recommendation/SINGLE_SERVER_PURE_RUST_OPTIMIZATIONS.md`
- `artifacts/massive_v2_recommendation/SYSTEM_ARCHITECTURE.md`
- `artifacts/massive_v2_recommendation/TREBUCHET_IMPROVEMENTS_MASTER.md`
- `artifacts/massive_v2_recommendation/massive_game_client_analysis.md`
- `artifacts/massive_v2_recommendation/massive_game_server_improvements.md`
- `artifacts/massive_v2_recommendation/scalability_analysis.md`
- `artifacts/massive_v2_recommendation/scalability_analysis_part1.md`
- `artifacts/massive_v2_recommendation/scalability_analysis_part2.md`
- `artifacts/massive_v2_recommendation/client.html`

## Immediate Priority (from `CODE_REVIEW_LATEST.md`)
- `T3-05 WASM bot sandbox`: Started and implemented MVP.
- `T3-06 code generation/validator`: Started with operational API scaffolding.
- `Human priority slot management`: Implemented in join flow with lowest-performing bot eviction.
- `T1-01 authoritative ECS ownership migration`: In progress, previous SoA/snapshot work retained.
- `T1-08 120-client tail regression`: In progress, retained benchmark instrumentation paths.

## Implemented In This Pass
- Added wasm sandbox runtime:
  - `server/src/operational/bot_sandbox.rs`
  - Deterministic duel simulator with wasm `bot_tick(i32,i32,i32,i32)->i32` contract.
  - Fuel limits and fallback runtime when wasm is missing/invalid.
- Arena execution endpoint now runs queued matches:
  - `POST /api/arena/matches/execute_next`
  - wired through `server/src/operational/arena.rs`.
- Arena wasm upload + worker scaffolding:
  - `POST /api/arena/models/upload_wasm`
  - `MGS_ARENA_WORKER_ENABLED` background queue executor in `server/src/main.rs`
  - `GET /api/arena/worker/stats` for worker run/executed/idle/failure counters
- Added code generation/validator operational routes:
  - `POST /api/arena/code/validate`
  - `POST /api/arena/code/generate`
  - `POST /api/arena/code/generate_and_compile`
  - module: `server/src/operational/code_generation.rs`
  - OpenRouter request path implemented with deterministic template fallback on provider failure/unset key.
- Added human-priority slot logic:
  - `server/src/server/instance.rs`
  - `server/src/network/signaling.rs`
  - full server joins can evict the lowest-performing bot when match is full.
  - team-aware bot eviction bias for balanced joins (`ensure_human_join_capacity_for_team`)
  - explicit eviction announcement enqueued to system chat feed for all human-priority slot reclaim events.
- Refreshed benchmark artifacts after join policy update:
  - `artifacts/scale/multi_client_fresh_20_post_team_balanced_20260214_2038.json`
    - 20/20 launched, connected ratio `1.00`, connect avg `21938.8ms`.
  - `artifacts/scale/multi_client_fresh_120_post_team_balanced_20260214_2050.json`
    - 86/120 launched within `300000ms`, connected ratio `0.7167`.
    - 73+ wave: count `14`, avg `88631.71ms`, p95 `92412.3ms`.
    - join-stage 73+: `open_channel_wait_ms.avg=356.27` (`p95=1883.85`), `queue_wait_ms.avg=9.33`.
- Updated server docs:
  - `server/README.md`

## Next Execution Batch
1. Recover 120-tail launch coverage from `86/120` toward `100+/120` within the same 300s budget.
2. Target 73+ wave channel-open contention (`open_channel_wait_ms` p95 `1883.85ms`) with join scheduler/backpressure tuning.
3. Add kill-feed-level explicit slot reclaim message (current behavior is system-chat announcement only).
4. Continue authoritative ECS ownership migration and full-state zero-copy/batching follow-through.
