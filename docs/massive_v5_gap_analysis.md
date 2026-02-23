# Massive V5 Gap Analysis

Date: 2026-02-23  
Baseline document: `/Users/ivo/massive_game_server/docs/massive_v5_recommendations.md` (2026-02-22)

## Scope
This gap analysis compares V5 recommendations against current implementation state in:
- `/Users/ivo/massive_game_server/server/src`
- `/Users/ivo/massive_game_server/server/schemas/game.fbs`
- `/Users/ivo/massive_game_server/static_client`

Status legend:
- `Done` = implemented and wired into runtime path
- `Partial` = scaffolded or server/client half-complete
- `Not started` = no production implementation found

## Executive Summary
- `Done`: 8 items
- `Partial`: 9 items
- `Not started`: 2 items

Critical remaining product gaps:
1. 120-client tail-wave join hardening is only partially complete (initial-state chunking/prewarm model is incomplete).
2. Progressive destructible terrain is not implemented.
3. Commander mode gameplay loop is not implemented.

## Phase Matrix

| ID | Recommendation | Status | Evidence | Gap to Close |
|---|---|---|---|---|
| 1.1 | Player-player collision + soft push | Done | `/Users/ivo/massive_game_server/server/src/server/instance/physics.rs` (`apply_player_soft_push_separation`) | None identified |
| 1.2 | Per-weapon damage falloff/balance | Done | `/Users/ivo/massive_game_server/server/src/systems/combat/weapons.rs`; projectile damage path in `/Users/ivo/massive_game_server/server/src/server/instance/physics.rs` | None identified |
| 1.3 | Server LOS raycast validation | Done | `/Users/ivo/massive_game_server/server/src/server/instance/physics.rs` (`has_clear_line_of_sight`); melee LOS check in `/Users/ivo/massive_game_server/server/src/server/instance/combat_melee.rs` | None identified |
| 1.4 | Real pathfinding replacing detour hack | Done | Grid A* in `/Users/ivo/massive_game_server/server/src/world/navigation.rs`; bot integration in `/Users/ivo/massive_game_server/server/src/systems/ai/bot_ai.rs` | Recommendation called for A* grid and this exists; navmesh remains optional |
| 2.1 | Movement abilities (dash/dodge) | Partial | Ability cooldown/runtime fields in `/Users/ivo/massive_game_server/server/src/core/types.rs`; processing in `/Users/ivo/massive_game_server/server/src/server/instance/input_runtime.rs` | Client input bindings are not wired (no key path sets `use_ability_slot` in `/Users/ivo/massive_game_server/static_client/client.html`) |
| 2.2 | Weapon loadout + swap delay | Partial | Secondary weapon/swap state in `/Users/ivo/massive_game_server/server/src/core/types.rs`; swap handling in `/Users/ivo/massive_game_server/server/src/server/instance/input_runtime.rs` | Client weapon-slot controls are not wired in `/Users/ivo/massive_game_server/static_client/client.html` |
| 2.3 | Environmental hazards (slow/damage/boost zones) | Partial | Zone generation in `/Users/ivo/massive_game_server/server/src/world/map_generator.rs`; effects in `/Users/ivo/massive_game_server/server/src/server/instance/physics.rs` | Zone schema/replication is missing in `/Users/ivo/massive_game_server/server/schemas/game.fbs`; client visualization is absent |
| 3.1 | Kill cam replay | Partial | Killcam capture + direct packet dispatch in `/Users/ivo/massive_game_server/server/src/server/instance/match_summary.rs` | Client only stores payload (no replay renderer) in `/Users/ivo/massive_game_server/static_client/client.html` |
| 3.2 | Post-match stats screen | Partial | Summary assembly + broadcast in `/Users/ivo/massive_game_server/server/src/server/instance/match_summary.rs` | Client receives event but has no full stats screen implementation in `/Users/ivo/massive_game_server/static_client/client.html` |
| 3.3 | SBMM | Partial | MMR compute in `/Users/ivo/massive_game_server/server/src/operational/auth.rs`; routing primitives in `/Users/ivo/massive_game_server/server/src/scaling/router.rs` | WebSocket join path only logs shard hint (no enforced multi-instance placement) in `/Users/ivo/massive_game_server/server/src/main.rs` |
| 4.1 | Team ping system | Done | Server ping ingestion + teammate filtering in `/Users/ivo/massive_game_server/server/src/server/instance/input_runtime.rs` and `/Users/ivo/massive_game_server/server/src/server/instance/broadcast_state.rs`; client ping send/render wiring in `/Users/ivo/massive_game_server/static_client/client.html` | None identified |
| 4.2 | Spectator mode | Done | Join gating/cap and spectator assignment in `/Users/ivo/massive_game_server/server/src/network/signaling.rs`; spectator handling through input/physics/broadcast in `/Users/ivo/massive_game_server/server/src/server/instance/*` | None identified |
| 4.3 | Public bot arena | Partial | Arena APIs + leaderboard in `/Users/ivo/massive_game_server/server/src/operational/arena.rs`; UI shell in `/Users/ivo/massive_game_server/static_client/arena.html` | Monaco-style code editor and source submission workflow are not present in current arena UI |
| 5.1 | Close 120-client gap | Partial | SDP admission gate + queue hint in `/Users/ivo/massive_game_server/server/src/network/signaling.rs`; tail-wave broadcast policies in `/Users/ivo/massive_game_server/server/src/server/instance/broadcast_loop.rs` | No true multi-frame initial-state chunking; client-state prewarm exists but not full pre-SDP warm-pool design |
| 5.2 | Persistent server-side replay recording | Done | Live replay capture + zstd persisted snapshots + retention in `/Users/ivo/massive_game_server/server/src/server/instance/replay.rs` | None blocking for current milestone |
| 5.3 | Map editor + custom maps | Partial | Editor UI exists in `/Users/ivo/massive_game_server/static_client/editor.html`; JSON loader exists in `/Users/ivo/massive_game_server/server/src/world/map_loader.rs` | Match creation flow is still tied to generated maps in `/Users/ivo/massive_game_server/server/src/server/instance.rs` |
| 5.4 | Authoritative ECS migration | Partial | Authoritative ECS default in `/Users/ivo/massive_game_server/server/src/server/ecs_bridge.rs` | Most game logic is still concentrated in `/Users/ivo/massive_game_server/server/src/server/instance.rs` |
| 6.1 | Progressive destructible terrain stages | Not started | No staged wall split/reassembly pipeline found | Implement wall health thresholds + child-wall lifecycle |
| 6.2 | Commander mode | Not started | Only predictive helper models in `/Users/ivo/massive_game_server/server/src/systems/ai/commander.rs` | Missing commander player role, waypoint/control protocol, and bot obedience pipeline |
| 6.3 | Dynamic game mode transitions | Partial | FFA->TDM->CTF transitions in `/Users/ivo/massive_game_server/server/src/server/instance/game_modes.rs` | Missing transition countdown announcements and richer transition UX |

## Bug Fixes From Prior Review

| Recommendation | Status | Evidence |
|---|---|---|
| Predictive model memory growth on disconnect | Done | Runtime pruning via retain in `/Users/ivo/massive_game_server/server/src/systems/ai/optimized_bot_ai.rs` and `/Users/ivo/massive_game_server/server/src/server/instance/input_runtime.rs` |
| Defender stuck false positives | Done | Defender/path-aware bypass logic in `/Users/ivo/massive_game_server/server/src/systems/ai/optimized_bot_ai.rs` |
| `partial_cmp().unwrap()` panic risk | Done | No `partial_cmp().unwrap()` remains in `/Users/ivo/massive_game_server/server/src/systems/ai/bot_ai.rs` |
| Path timer reused for weapon swap | Done | Dedicated `last_weapon_switch_time` field in `/Users/ivo/massive_game_server/server/src/server/instance/types.rs` |
| Prediction timestamp clamp | Done | `future_dt.min(2000.0)` in `/Users/ivo/massive_game_server/server/src/systems/ai/commander.rs` |
| Empty physics modules | Done | Minimal movement/ballistics modules now populated: `/Users/ivo/massive_game_server/server/src/systems/physics/movement.rs`, `/Users/ivo/massive_game_server/server/src/systems/physics/ballistics.rs` |
| Negative damage healing edge case | Done | Damage clamping in `/Users/ivo/massive_game_server/server/src/systems/combat/damage.rs` and weapon falloff path |

## Priority Cut (What Is Left)

### P0 (finish first)
1. Wire team ping end-to-end client path:
   - set `inputState.ping_x/ping_y` from ping wheel action and clear after send
   - render `TeamPing` game events in minimap/world HUD
2. Complete 120-client join hardening:
   - split initial snapshot across ordered chunks
   - finalize prewarm strategy for per-peer state before heavy serialization
   - run repeatable 120-client tail-wave load test and capture pass/fail

### P1
1. Finish client integration for abilities/loadout controls and HUD feedback.
2. Implement proper killcam replay player and post-match summary panel.
3. Wire map editor output to match creation path (custom map selection/import).

### P2
1. Implement progressive destructible terrain stages.
2. Implement commander mode gameplay loop (waypoints, bot role mix, supply drops).
3. Add transition countdown broadcast/events for dynamic mode shifts.
