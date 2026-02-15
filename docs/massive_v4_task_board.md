# Massive V4 Task Board (2026-02-15)

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
| V4-R1 | `120` launch target closure | In Progress | Strict rerun with stabilized runner captured `96/120` (`artifacts/scale/multi_client_120_v4_extreme_tail_stable_20260215_162613.json`) matching baseline launch count (`96/120`, `artifacts/scale/multi_client_120_v4_join_timing_20260215_141725.json`) but with worse tail latency/open-channel wait. Next pass should target `wave_73_plus open_channel_wait` regression before further scheduler tuning. |
| V4-R2 | Full authoritative ECS writer ownership migration | In Progress | Broadcast read path is snapshot-owned; mutation ownership is still partially split in `server/src/server/instance.rs`. |
| V4-R3 | Full-state zero-copy follow-through | In Progress | Main delta/initial paths are collapse-based, but broader serialization construction and legacy paths still need full migration/audit. |
| V4-R4 | Broader SIMD physics coverage | In Progress | Projectile collision SIMD path is active; broader movement/collision vectorization is still pending. |
| V4-R5 | `instance.rs` decomposition | In Progress | Core file remains monolithic; extract broadcast/join/snapshot modules after perf-sensitive tail work stabilizes. |

## Validation Plan
1. Re-run deterministic `10v10` and `120` tail benchmarks and capture fresh artifacts at the same timeout window (`600000ms`) for strict comparability.
2. Compare `wave_73_plus` launched count, `connectLatency avg/p95`, and client `firstState` timing.
3. Update `/Users/ivo/massive_game_server/production_perfomance_log.md` with before/after deltas.
4. Profile and reduce `wave_73_plus open_channel_wait` regression before additional extreme-tail scheduling changes.
