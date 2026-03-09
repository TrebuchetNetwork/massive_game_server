# Massive V4 Task Board (2026-02-16)

## Scope
- Stabilize `120`-client tail launch throughput for single-machine runs.
- Close remaining hot-path serialization and join-scheduler overhead.
- Ship frontend network profiler visibility for tail-wave diagnosis.

## Implemented In This Pass
| ID | Item | Status | Notes |
|---|---|---|---|
| V4-01 | 90+ extreme tail join mode | Done | Added extreme tail policy (`90+` clients + open-initial backlog) with stronger initial-priority scheduling, tighter delta throttling, stricter fanout cap, and reduced initial snapshot caps. |
| V4-02 | Initial-state join hot-path log reduction | Done | Downgraded per-client initial-state serialization logs from `info` to `debug` to reduce join-storm logging overhead. |
| V4-03 | Hot-path entity-id serialization alloc trim | Done | Added `itoa`-backed `fb_safe_entity_id` and replaced repeated `u64 -> String` heap allocations in active initial/delta serialization paths. |
| V4-04 | Frontend network profiler overlay | Done | Added live profiler panel (rx KB/s, packet/message rates, state update rate, snapshot Hz/jitter, interpolation delay, DC buffered bytes), toggle in settings, and e2e metrics export. |

## Current Remaining Items
| ID | Item | Status | Notes |
|---|---|---|---|
| V4-R1 | `120` launch target closure | In Progress | Latest strict rerun with closed-channel send guards + tuned scheduler thresholds (`artifacts/scale/multi_client_120_v4_r1_sendpath_tuned_20260215_191602.json`) remains `96/120`; vs first send-path run it reduced tail latency (`connect avg 57039.79 -> 54679.44`, `73+ avg 100507.42 -> 94144.5`) and kept lower server open-wait pressure (`73+ open_channel_wait avg=465.36`), but it still does not recover `100+`. |
| V4-R2 | Full authoritative ECS writer ownership migration | Done | Completed authoritative apply-stage ownership for both projectile and pickup hot paths: projectile writes stay centralized in `apply_projectile_results`, and pickup collection now runs as read-only candidate discovery + authoritative apply (`collect_pickup_collection_candidates` -> `apply_pickup_collection_authoritative`). |
| V4-R3 | Full-state zero-copy follow-through | Done | Active serialization paths now consistently use zero-copy `collapse`; removed unused legacy static send/serialization methods (`build_delta_state_static`, `process_client_broadcast_static`, `send_initial_state_to_client`, `send_delta_state_to_client`, related static chat/state helpers), reducing duplicate copy-heavy code paths. |
| V4-R4 | Broader SIMD physics coverage | Done | SIMD now covers both projectile segment-hit checks (`first_index_within_segment_radius`, AVX2 + NEON) and pickup candidate detection (`collect_pickup_candidates` uses `first_index_within_radius` over active pickup SoA vectors). |
| V4-R5 | `instance.rs` decomposition | Done | Decomposition landed across focused modules: packet batching (`packet_batch.rs`), event mapping (`event_mapping.rs`), and pickup pipeline (`pickup_pipeline.rs`), plus dead legacy helper removal reduced `instance.rs` from `7721` to `6329` lines. |

## Validation Plan
1. Keep strict `600000ms` benchmark profile and run at least 3 repeated `120` passes to separate code impact from run variance.
2. Add explicit signaling admission/backpressure controls for `73+` wave (offer handling window + per-wave join gate), then rerun.
3. Correlate client-side join stage (`dataChannelOpen`/`firstState`) with server `open_channel_wait_ms` per wave to isolate browser-side tail saturation.
4. Continue targeting `100+` launched under strict cap without regressing `10v10` latency envelope.
