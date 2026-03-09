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
- `T1-01 authoritative ECS ownership migration`: In progress, lock-free AoI + SoA snapshots now wired into broadcast reads; full writer-only ownership remains.
- `T1-08 120-client tail regression`: Reached 100/120 launch threshold in latest pass; stabilization continues.

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
  - explicit eviction announcements now emit to both system chat and kill feed for all human-priority slot reclaim events.
- Refreshed benchmark artifacts after join policy update:
  - `artifacts/scale/multi_client_fresh_20_post_team_balanced_20260214_2038.json`
    - 20/20 launched, connected ratio `1.00`, connect avg `21938.8ms`.
  - `artifacts/scale/multi_client_fresh_120_post_team_balanced_20260214_2050.json`
    - 86/120 launched within `300000ms`, connected ratio `0.7167`.
    - 73+ wave: count `14`, avg `88631.71ms`, p95 `92412.3ms`.
    - join-stage 73+: `open_channel_wait_ms.avg=356.27` (`p95=1883.85`), `queue_wait_ms.avg=9.33`.
  - `artifacts/scale/multi_client_fresh_120_tail_tuned_v2_20260214_2247.json`
    - 87/120 launched within `300000ms`, connected ratio `0.725` (delta `+1` launched).
    - 73+ wave: count `15`, avg `79709.27ms`, p95 `83942.1ms`.
    - join-stage 73+: `open_channel_wait_ms.avg=253.99` (`p95=1178.84`) after tail-policy tuning.
- Added authoritative AoI ownership path in broadcast serialization:
  - `server/src/server/instance.rs`
  - now default-on (`MGS_JOIN_DISABLE_AUTHORITATIVE_AOI_SNAPSHOT=1` to opt out).
  - `process_client_broadcast` now gates player existence from the per-broadcast shared snapshot view.
  - unified chat packet follow-through for both initial and delta dispatch paths (single batched send path + residual resend path).
- Extended single-machine ownership and transport follow-through:
  - `server/src/concurrent/atomic_snapshot.rs`
  - `server/src/server/instance.rs`
  - Added lock-free `PlayerAoISnapshot`/`AtomicPlayerAoISnapshot` and switched scheduled broadcast AoI lookup to authoritative snapshot reads.
  - AoI snapshot publish now runs after `synchronize_state` to keep broadcast on latest frame visibility.
  - Initial/delta state serializers are sync-path (no async state machine allocation) and initial bytes are cached on build for retry reuse.
  - Packet coalescing now supports single-packet envelopes so all outbound packets use the same transport batch format.
  - Active broadcast snapshot path no longer rebuilds empty SoA/AoI snapshots from live world structures during fanout preparation (strict snapshot-owned read path).
  - Initial-state wall fallback now resolves from shared broadcast snapshots instead of live world collection.
- Fresh benchmark artifacts from this pass:
  - `artifacts/scale/multi_client_fresh_20_aoi_snapshot_20260214_2310.json`
    - 20/20 launched, connected ratio `1.00`, connect avg `21001.65ms`.
  - `artifacts/scale/multi_client_fresh_120_aoi_snapshot_20260214_2311.json`
    - 73/120 launched, connected ratio `0.6083`, 73+ wave count `1`.
  - `artifacts/scale/multi_client_fresh_120_aoi_snapshot_fresh_20260214_2318.json`
    - 73/120 launched, connected ratio `0.6083`, 73+ wave count `1`.
  - `artifacts/scale/multi_client_fresh_120_aoi_snapshot_disabled_default_20260214_2329.json`
    - 72/120 launched, connected ratio `0.60`, 73+ wave count `0`.
- Revalidated release-binary benchmarks and compared tail variants:
  - `artifacts/scale/multi_client_fresh_20_release_20260214_2342.json`
    - 20/20 launched, connected ratio `1.00`, connect avg `20654.9ms`.
  - `artifacts/scale/multi_client_fresh_120_release_recheck_20260214_2344.json`
    - 97/120 launched, connected ratio `0.8083`, 73+ wave count `25` (best in this recheck batch).
  - `artifacts/scale/multi_client_fresh_120_tail_tune3_20260214_2356.json`
    - 90/120 launched, connected ratio `0.75`; experimental scheduler tuning underperformed and was rolled back.
  - `artifacts/scale/multi_client_fresh_120_closed_delta_skip_20260215_0007.json`
    - 93/120 launched, connected ratio `0.775`; variant was not retained after comparison.
  - `artifacts/scale/multi_client_fresh_20_missingpass_20260215_0025.json`
    - 20/20 launched, connected ratio `1.00`, connect avg `17206.2ms`.
  - `artifacts/scale/multi_client_fresh_120_missingpass_20260215_0026.json`
    - 95/120 launched, connected ratio `0.7917`.
  - `artifacts/scale/multi_client_fresh_120_missingpass_aoi_on_20260215_0035.json`
    - 100/120 launched, connected ratio `0.8333` (threshold target met for this phase).
  - `artifacts/scale/multi_client_fresh_120_phase2_20260214_195643.json`
    - 100/120 launched, connected ratio `0.8333` after lock-free AoI snapshot + unified envelope pass.
  - `artifacts/scale/multi_client_fresh_120_phase2_complete_20260214_210222.json`
    - 100/120 launched, connected ratio `0.8333` after strict snapshot-only broadcast read pass.
    - connect avg `61116.58ms` (improved from `63885.71ms` in prior phase2 run).
- Updated server docs:
  - `server/README.md`

## Next Execution Batch
1. Stabilize `100/120`+ launch coverage across repeated runs (reduce variance between 95-100 results).
2. Continue reducing wave `1-48` `open_channel_wait_ms` spikes via signaling/open-channel backpressure tuning.
3. Continue writer-side authoritative ECS migration (mutation ownership boundaries) and reduce remaining full-state serialization copy points.
