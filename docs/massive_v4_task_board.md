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
| V4-R2 | Full authoritative ECS writer ownership migration | In Progress | Moved destructible wall mutation out of `process_projectiles_optimized` and into authoritative apply stage (`apply_projectile_results` via `apply_wall_damage_authoritative`); broader ownership split (player/wall/pickup mutation surfaces) is still not fully migrated. |
| V4-R3 | Full-state zero-copy follow-through | In Progress | Welcome-message serialization now always uses zero-copy `collapse` in `server/src/network/signaling.rs`; initial/delta paths already use collapse, but remaining serialization/audit follow-through is still open. |
| V4-R4 | Broader SIMD physics coverage | In Progress | Added segment-distance SIMD helper (`core::simd::first_index_within_segment_radius`) and switched projectile-player hot path in `server/src/server/instance.rs` from per-ray-step scans to segment hit checks; broader movement/collision vectorization is still pending. |
| V4-R5 | `instance.rs` decomposition | In Progress | Extracted packet batching/coalesced send logic into `server/src/server/packet_batch.rs` and left a thin wrapper in `instance.rs`; remaining broadcast/join/snapshot extraction is still pending. |

## Validation Plan
1. Keep strict `600000ms` benchmark profile and run at least 3 repeated `120` passes to separate code impact from run variance.
2. Add explicit signaling admission/backpressure controls for `73+` wave (offer handling window + per-wave join gate), then rerun.
3. Correlate client-side join stage (`dataChannelOpen`/`firstState`) with server `open_channel_wait_ms` per wave to isolate browser-side tail saturation.
4. Continue targeting `100+` launched under strict cap without regressing `10v10` latency envelope.
