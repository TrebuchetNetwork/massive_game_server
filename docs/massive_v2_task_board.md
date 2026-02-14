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
- Added code generation/validator operational routes:
  - `POST /api/arena/code/validate`
  - `POST /api/arena/code/generate`
  - module: `server/src/operational/code_generation.rs`
  - OpenRouter request path implemented with deterministic template fallback on provider failure/unset key.
- Added human-priority slot logic:
  - `server/src/server/instance.rs`
  - `server/src/network/signaling.rs`
  - full server joins can evict the lowest-performing bot when match is full.
- Updated server docs:
  - `server/README.md`

## Next Execution Batch
1. Wire OpenRouter provider call in `code_generation` service (currently deterministic template fallback).
2. Add wasm upload/registration API path (`model -> wasm artifact`) and execute round-robin arena workers.
3. Extend slot manager policy with reserved-team balancing and explicit eviction announcements in kill feed/system chat.
4. Continue tail-wave join regression isolation with `120` deterministic reruns.
