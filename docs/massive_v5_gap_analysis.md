# Massive V5 Gap Analysis

**Date:** 2026-02-23
**Baseline:** `docs/massive_v5_recommendations.md` (2026-02-22)
**Methodology:** Full code review of server modules, FlatBuffers schemas, generated JS bindings, and Pixi.js client

---

## Executive Summary

| Category | Count |
|----------|-------|
| **Done** | 17 items (14 recommendations + 3 bonus features) |
| **Partial** | 6 items |
| **Not started** | 0 items |

All 20 recommendations and all 7 bug fixes have at least partial implementation. Three Phase 6 features (progressive terrain, commander mode, dynamic mode transitions) were implemented ahead of schedule despite being classified as low-priority.

**Critical remaining gap:** The FlatBuffers wire schema lags behind server internals — V5 fields (abilities, loadouts, zones) exist in Rust types but are not replicated via FlatBuffers. The client works around this using JSON system events and client-side cooldown tracking.

---

## Phase 1: Fix Fundamentals

### 1.1 Player-Player Collision & Soft Push — DONE

**Evidence:**
- `server/src/server/instance/physics.rs`: `apply_player_soft_push_separation()` (line 621)
- Separation force proportional to overlap depth, capped at `PLAYER_BASE_SPEED * dt * 0.5`
- Wall overlap check before applying push to avoid clipping players into walls
- Called after `process_player_physics_parallel()` every physics tick

**No gaps identified.**

---

### 1.2 Per-Weapon Damage Falloff — DONE

**Evidence:**
- `server/src/systems/combat/weapons.rs`: Full `WeaponFalloffProfile` system (line 13)
- Shotgun damage: 7 → 12 per pellet as recommended
- Rifle falloff starts at 200u, min 40% — no longer dominates at range
- `apply_distance_falloff()` called in projectile hit path (`physics.rs:1725`)
- Negative damage clamped via `.max(0)` (weapons.rs:91)

**Balance table matches recommendations:**

| Weapon | Base Dmg | Falloff Start | Min % |
|--------|----------|---------------|-------|
| Pistol | 8 | 150 | 60% |
| Shotgun | 12 | 40 | 10% |
| Rifle | 10 | 200 | 40% |
| Sniper | 50 | 600 | 80% |
| Melee | 30 | N/A | 100% |

**No gaps identified.**

---

### 1.3 Line-of-Sight Raycast Validation — DONE

**Evidence:**
- `server/src/server/instance/physics.rs`: `has_clear_line_of_sight()` (line 1898)
- Uses `wall_spatial_index.query_line_segment()` + `segment_first_hit_fraction_with_aabb()` ray-AABB test
- Called before applying projectile damage (physics.rs:1714)
- Also checked in melee hit detection (`server/src/server/instance/combat_melee.rs`)

**No gaps identified.**

---

### 1.4 A* Pathfinding on Navigation Grid — DONE

**Evidence:**
- `server/src/world/navigation.rs`: Full `GridNav` struct with `find_path()` A* implementation (line 83)
- 8-directional movement, world-to-grid coordinate mapping
- `find_path_world()` API for direct Vec2 → Vec2 pathfinding
- Integrated into bot AI via `server/src/systems/ai/bot_ai.rs`

**No gaps identified.** Navmesh (alternate approach) remains optional.

---

## Phase 2: Player Expression & Game Feel

### 2.1 Movement Abilities (Dash/Dodge Roll) — DONE

**Evidence:**
- `server/src/core/types.rs`: `ability_1_cooldown_remaining`, `ability_2_cooldown_remaining`, `dash_remaining`, `dodge_roll_remaining`, `invulnerable_remaining` fields on PlayerState
- `server/src/server/instance/input_runtime.rs` (line 648): Ability slot 1 = Dash (8s CD, 0.2s duration, 2x speed), Slot 2 = Dodge Roll (12s CD, 0.3s duration, invulnerability frames)
- `server/src/core/constants.rs`: `ABILITY_DASH_*` and `ABILITY_DODGE_*` constants
- Client: `Q` = Dash, `E` = Dodge Roll, mobile ability buttons, radial cooldown HUD

**No gaps identified.**

---

### 2.2 Weapon Loadout System — DONE

**Evidence:**
- `server/src/core/types.rs`: `secondary_weapon`, `weapon_swap_progress` fields
- `server/src/server/instance/input_runtime.rs`: `start_weapon_swap_to_slot()` (line 870) with swap delay
- Client: `1`/`2` keys for weapon slot switching, mobile primary/secondary buttons

**No gaps identified.**

---

### 2.3 Environmental Hazards — PARTIAL

**Evidence:**
- **Server generation:** `server/src/world/map_generator.rs`: `generate_environment_zones_with_seed()` creates 5 zones (1 SlowZone, 2 DamageZones, 2 BoostPads)
- **Server effects:** `server/src/server/instance/physics.rs` (line 424): Zone effects applied during movement — slow multiplier (0.6x), damage (5 DPS), boost pad (2x speed + direction override, with retrigger cooldown)
- **Constants:** `ZONE_SLOW_MULTIPLIER`, `ZONE_DAMAGE_PER_SEC`, `ZONE_BOOST_*` all defined

**Gaps:**
1. **Schema missing:** No `ZoneType` enum or `Zone` table in `server/schemas/game.fbs` — zones cannot be replicated to clients via FlatBuffers
2. **Client visualization absent:** No zone rendering in `client.html` — players experience zone effects but see no visual indication on the ground
3. **Map editor missing zones:** `static_client/editor.html` has wall/pickup tools but no zone placement

---

## Phase 3: Retention & Competitive Systems

### 3.1 Kill Cam (Death Replay) — DONE

**Evidence:**
- `server/src/server/instance/match_summary.rs`: `capture_killcam_for_victim()` (line 107) collects last 30 position samples from killer's `InterpolationBuffer`, constructs `KillCamData` with rotation interpolation
- Sent as JSON system event via `build_system_event_packet("killcam", ...)` → chat channel
- `server/src/server/instance/types.rs`: `KillCamData`, `KillCamSample` structs
- Client: `#killcamPanel` with `#killcamCanvas` (280x140), `tickKillcamPlayback()` draws killer trajectory path

**No gaps identified.** Note: Bypasses FlatBuffers schema — sent as JSON over chat channel. Functionally correct.

---

### 3.2 Post-Match Stats Screen — DONE

**Evidence:**
- `server/src/server/instance/match_summary.rs`: `capture_match_end_summary()` (line 15) compiles per-player stats including K/D, damage dealt/taken, flag captures/returns, weapon kills, MVP awards
- `server/src/server/instance/types.rs`: `MatchEndSummary`, `PlayerMatchStats` structs with full `Serialize` derive
- Client: `#postMatchPanel` with `renderPostMatchSummary()` — score/K/D/damage table with MVP row

**No gaps identified.**

---

### 3.3 SBMM — PARTIAL

**Evidence:**
- `server/src/operational/auth.rs`: `compute_mmr()` function — formula: `(K/D * 100) + (avg_score * 0.5)`. Band classification: Bronze (<100), Silver (<250), Gold (<500), Diamond (500+)
- `server/src/scaling/router.rs`: `assign_with_mmr()` prepends MMR band to routing key for rendezvous hashing

**Gaps:**
1. **Not enforced in live join path:** The WebSocket join handler in `server/src/main.rs` only logs the shard hint — does not actually route to a different instance based on MMR
2. **Inconsistent band definitions:** `auth.rs` uses 4 tiers (Bronze/Silver/Gold/Diamond), `router.rs` uses 5 tiers (rookie/bronze/silver/gold/elite) — these are never reconciled
3. **Requires multi-instance deployment** which is not yet operational

---

## Phase 4: Social & Community Features

### 4.1 Team Ping System — DONE

**Evidence:**
- `server/schemas/game.fbs`: `TeamPing = 11` in GameEventType, `ping_x`/`ping_y` fields in PlayerInput
- `server/src/server/instance/input_runtime.rs` (line 740): Ping processing with world bounds validation, 3-second cooldown (`TEAM_PING_COOLDOWN_SECS`), team-filtered `GameEvent::TeamPing`
- Generated JS has `TeamPing` enum value
- Client: Long-press opens `#pingWheel`, renders teammate pings vs commander orders with distinct styling

**No gaps identified.**

---

### 4.2 Spectator Mode — DONE

**Evidence:**
- `server/src/network/signaling.rs`: `requested_team_id == 0` triggers spectator path with `can_accept_spectator_join()` cap check
- Spectator gets: `is_spectator = true`, `team_id = 0`, free camera movement at 1.35x speed
- Input processing skips shooting/abilities for spectators
- Broadcast includes spectators with expanded AoI (no team filter)

**No gaps identified.**

---

### 4.3 Public Bot Arena — PARTIAL

**Evidence:**
- `server/src/operational/arena.rs`: Full arena APIs with ELO ladder, match scheduling, leaderboard
- `static_client/arena.html`: Exists but is a **model ladder management console** (register models, queue matches, view ELO)

**Gaps:**
1. **No code editor:** No Monaco editor or code editing capability — this is an orchestration dashboard, not a player-facing code submission UI
2. **No source submission workflow:** No endpoint for submitting Rust source code from the browser for WASM compilation

---

## Phase 5: Technical Excellence & Scale

### 5.1 Close the 120-Client Gap — PARTIAL

**Evidence:**
- `server/src/network/signaling.rs`: SDP admission gate with `DEFAULT_SDP_ADMISSION_CONCURRENCY = 4`, semaphore-based queuing with `sdp_offer_queue` position hint sent to client
- Join rate limiter: 30/sec with burst 50
- Tail-wave broadcast policies in `server/src/server/instance/broadcast_loop.rs`
- `server/src/server/instance/broadcast_state.rs`: `InitialStateChunkBuildParams` struct with flags for walls/players/projectiles/pickups

**Gaps:**
1. **Incomplete multi-frame initial-state chunking:** `InitialStateChunkBuildParams` exists but the broadcast loop still sends a single monolithic InitialState message to new joiners — the chunk pipeline is not fully wired
2. **No pre-SDP warm pool:** `ClientState` is allocated after channel open, not pre-warmed before SDP exchange completes
3. **No 120-client validation test:** No automated pass/fail load test for the tail-wave join scenario

---

### 5.2 Server-Side Replay Recording — DONE

**Evidence:**
- `server/src/server/instance/replay.rs`: In-memory ring buffer of `LiveReplayFrame`s, one per tick
- zstd compressed persistence: `persist_match_replay_snapshot()` serializes to JSON + `zstd::encode_all(..., 3)`, files as `replay_{ts}_{reason}.json.zst`
- Retention policy: `enforce_live_replay_match_retention()` deletes oldest beyond configurable limit
- Dispute audit chain with SHA256 + HMAC-SHA256 signed chain
- Feature flags: `live_replay_enabled`, `live_replay_match_persist_enabled`, `live_replay_dispute_persist_enabled`

**No gaps identified.** Implementation exceeds the recommendation with dispute audit chain.

---

### 5.3 Map Editor & Custom Maps — PARTIAL

**Evidence:**
- `static_client/editor.html`: Functional canvas editor with wall placement, destructible toggle, pickup types, JSON import/export
- `server/src/world/map_loader.rs`: Existing `load_map_from_json()` parser

**Gaps:**
1. **Not wired to match creation:** Server's match creation path in `server/src/server/instance.rs` is still tied to procedural generation — no way to specify a custom map JSON on match start
2. **No zone authoring:** Editor has no tool for placing SlowZone/DamageZone/BoostPad
3. **No spawn point placement:** Cannot author team spawn areas

---

### 5.4 Authoritative ECS Migration — PARTIAL

**Evidence:**
- `server/src/server/ecs_bridge.rs`: Defaults to `EcsMode::Authoritative` (via `MGS_ECS_MIGRATION_ENABLED=true` default)
- `run_authoritative_systems()` integrates player and projectile positions via ECS
- `apply_authoritative_reconciliation()` writes ECS positions back to monolithic state

**Gaps:**
1. **Thin authority:** ECS only does position integration (velocity × dt). All actual game logic (damage, collisions, pickups, weapons, AoI, zone effects, abilities) remains in the monolithic `instance.rs` and sub-modules
2. **instance.rs still 6300+ lines:** No material decomposition has occurred — the ECS is a parallel position integrator, not an architectural migration

---

## Phase 6: Differentiation Features

### 6.1 Progressive Destructible Terrain — DONE (Ahead of Schedule)

**Evidence:**
- `server/src/server/instance/physics.rs`: Full staged fragmentation system
  - `build_progressive_fragment_walls()` (line 1333): Stage 1 = 2 segments with gap, Stage 2 = 4 smaller segments with wider gaps
  - `apply_progressive_wall_fragmentation()` (line 1410): Parent-child wall lifecycle with topology invalidation
  - `clear_progressive_fragments_for_parent()` (line 1300): Cleanup on wall respawn
- `server/src/server/instance/types.rs`: `ProgressiveWallFragmentState`, `ProgressiveDestructibleState` structs
- Constants: `PROGRESSIVE_WALL_STAGE1_HEALTH_RATIO = 0.50`, `PROGRESSIVE_WALL_STAGE2_HEALTH_RATIO = 0.25`, `PROGRESSIVE_WALL_MIN_FRAGMENT_LENGTH = 12.0` (defined in `instance.rs`)
- Feature flag: `progressive_destructible_enabled`

**No gaps identified.** This was classified as low-priority but is fully implemented.

---

### 6.2 Commander Mode — DONE (Ahead of Schedule)

**Evidence:**
- `server/src/server/instance/input_runtime.rs`: Full commander system
  - `register_commander_waypoint()` (line 179): Up to 3 waypoints per team, 20s TTL
  - `spawn_commander_supply_drop()` (line 126): 6 pickups at waypoint location, 60s cooldown
  - `refresh_commander_runtime_state()` (line 19): Auto-assignment preferring human players, fallback to bots
- `server/src/systems/ai/optimized_bot_ai.rs`: Bots follow commander waypoints with 72% probability, commander attack bias shifts defend/attack role thresholds
- `server/schemas/game.fbs`: Commander fields in MatchInfo (commander IDs, waypoints, attack bias)
- Client: Commander HUD role indicator, `C` key for commander orders, waypoint position display
- `server/src/server/instance/types.rs`: `CommanderRuntimeState`, `CommanderWaypoint` structs
- Constants: `COMMANDER_MAX_WAYPOINTS_PER_TEAM = 3`, `COMMANDER_WAYPOINT_TTL_MS = 20_000`, `COMMANDER_SUPPLY_DROP_COOLDOWN_MS = 60_000`, `COMMANDER_SUPPLY_DROP_PICKUPS = 6` (defined in `instance.rs`)

**No gaps identified.**

---

### 6.3 Dynamic Game Mode Transitions — DONE (Ahead of Schedule)

**Evidence:**
- `server/src/server/instance/game_modes.rs`: `update_match_state_authoritative()` (line 33)
  - FFA for first 2 minutes → TDM transition at 120s elapsed → CTF transition with 70s remaining
  - Countdown events at 15s/20s/10s/5s before each transition
  - `broadcast_dynamic_mode_event()` sends JSON system event with phase/from/to/countdown
- Feature flag: `MGS_DYNAMIC_MODE_TRANSITIONS` env var
- Client: `mode_transition` event handler with urgency display

**No gaps identified.**

---

## Bug Fixes

| # | Bug | Status | Evidence |
|---|-----|--------|----------|
| 1 | Predictive model memory leak on disconnect | **Done** | `prune_runtime_tracking_state()` in `input_runtime.rs:4` retains only connected players for `player_position_history`, `aim_anomaly_states`, `direct_packets` |
| 2 | Bot defender stuck false-positives | **Done** | `check_stuck_status()` in `optimized_bot_ai.rs` skips stuck check when `Defending` within 220u of flag base, or when at target position |
| 3 | `partial_cmp().unwrap()` panic on NaN | **Done** | No `partial_cmp().unwrap()` remains in `bot_ai.rs` |
| 4 | Path timer repurposed for weapon switching | **Done** | Dedicated `last_weapon_switch_time: Instant` field on `BotController` in `types.rs:61` |
| 5 | Prediction timestamp clamp | **Done** | `future_dt.min(2000.0)` in `commander.rs:46` |
| 6 | Empty physics module files | **Done** | `movement.rs` has `integrate_velocity()`, `ballistics.rs` has `projectile_step()` — minimal but populated |
| 7 | Negative damage healing | **Done** | `amount.max(0)` in `damage.rs:24`, `.max(0)` in `types.rs` `apply_damage()` |

---

## Cross-Cutting: FlatBuffers Schema Gap

The server's `game.fbs` schema has not been updated to include V5 wire-protocol fields. The server works around this by sending V5 data as JSON system events over the chat channel.

### Fields on `PlayerState` (Rust) missing from `PlayerState` (FlatBuffers)

| Field | Purpose | Client Impact |
|-------|---------|---------------|
| `ability_1_cooldown_remaining` | Dash cooldown | Client tracks cooldown locally — visual-only divergence possible |
| `ability_2_cooldown_remaining` | Dodge cooldown | Same as above |
| `dash_remaining` | Dash active state | Client infers from local keypress timing |
| `dodge_roll_remaining` | Dodge active state | Same as above |
| `invulnerable_remaining` | Dodge invulnerability | **Not visible to other players** — no dodge roll visual on opponents |
| `secondary_weapon` | Loadout slot 2 | Other players can't see opponent's secondary weapon |
| `weapon_swap_progress` | Swap animation | No swap animation visible to other players |
| `is_spectator` | Spectator flag | Encoded as `team_id == 0` — works but implicit |

### Tables/enums missing from schema entirely

| Missing | Impact |
|---------|--------|
| `ZoneType` enum + `Zone` table | Zones cannot be sent to client — no visual rendering |
| `KillCamData` / `KillCamSample` | Sent as JSON instead — works but inconsistent with protocol |
| `PlayerMatchStats` / `MatchEndSummary` | Sent as JSON instead — works but inconsistent |

### Schema divergence between files

No active divergence identified for TeamPing or ping input fields between:
- `/Users/ivo/massive_game_server/protocol/schemas/game.fbs`
- `/Users/ivo/massive_game_server/server/schemas/game.fbs`

---

## Client Rendering Gaps

| Feature | Server State | Client Rendering | Gap |
|---------|-------------|-----------------|-----|
| Environmental zones | Fully simulated (slow/damage/boost effects) | **No rendering** | Players experience effects with zero visual feedback |
| Opponent dodge roll | Server tracks `invulnerable_remaining` | **Not transmitted** | Other players can't see invulnerability flicker |
| Opponent secondary weapon | Server tracks `secondary_weapon` | **Not transmitted** | Can't see what weapon opponents have holstered |
| Wall fragmentation stages | Server splits walls into child fragments | Walls appear/disappear as IDs change | **Partial** — no crack visual at 75% health, gap creation works via wall ID lifecycle |
| Zone editor preview | Zones generated server-side | Editor has no zone tools | Cannot preview zones while designing maps |

---

## Remaining Work — Prioritized

### P0: Ship-Blocking

1. **Complete 120-client join hardening** (`server/src/server/instance/broadcast_state.rs`, `broadcast_loop.rs`)
   - Wire `InitialStateChunkBuildParams` into actual multi-frame delivery
   - Implement `ClientState` pre-warm pool allocated before SDP exchange
   - Run and pass a repeatable 120-client tail-wave load test

### P1: Core Quality

2. **Add Zone schema + client visualization** (`server/schemas/game.fbs`, `static_client/client.html`)
   - Add `ZoneType` enum and `Zone` table to schema
   - Add zones to `InitialStateMessage`
   - Render slow zones as dark overlay, damage zones as red pulse, boost pads as directional arrows

3. **Wire custom maps to match creation** (`server/src/server/instance.rs`, `server/src/network/signaling.rs`)
   - Accept map JSON payload on match creation endpoint
   - Fall back to procedural generation when no custom map provided

4. **Enforce SBMM in live join routing** (`server/src/main.rs`, `server/src/scaling/router.rs`)
   - Reconcile MMR band definitions (4-tier vs 5-tier)
   - Route join requests through `assign_with_mmr()` when multi-instance is available

### P2: Polish

5. **Add ability/loadout fields to FlatBuffers PlayerState** (`server/schemas/game.fbs`)
   - Enables opponents to see dodge roll invulnerability, weapon swap animations

6. **Expand map editor with zone tools** (`static_client/editor.html`)
   - Zone placement/resize/delete with type selector and direction for boost pads

7. **Build public bot arena code submission UI** (`static_client/arena.html`)
   - Monaco editor for Rust bot AI source
   - Source submission → server-side WASM compilation pipeline

8. **Continue ECS migration** (`server/src/server/ecs_bridge.rs`, `server/src/server/instance.rs`)
   - Migrate pickup collection into ECS system (already decomposed into `pickup_pipeline.rs`)
   - Migrate projectile physics into ECS system
   - Goal: reduce `instance.rs` from 6300+ lines

### P3: Hardening

9. **Extract commander/progressive-wall constants to `constants.rs`**
    - Currently defined as module-level `const` in `instance.rs` (lines 173-179)
    - Move to `core/constants.rs` for consistency with other gameplay constants

10. **Add zone authoring to editor and spawn point placement**

11. **Automated load test harness** for 120-client validation with pass/fail criteria

---

## New Findings (Not in Original Recommendations)

These features were implemented without being in the original V5 recommendations:

1. **Dispute audit chain** (`replay.rs`): SHA256-hashed replay frames with HMAC-SHA256 chain signatures for anti-cheat dispute resolution — exceeds the simple "write replays to disk" recommendation

2. **Join stage tracing** (`types.rs`): Detailed per-client join latency instrumentation (`JoinStageTrace`, `JoinStageWaveSummary`, `JoinStageReport`) for diagnosing the 120-client gap — valuable observability addition

3. **Lock-free boundary zones** (`world/partition.rs`): `LockFreeBoundaryZone` with `crossbeam_epoch`-based safe pointer swapping for inter-partition communication — infrastructure investment not in recommendations

4. **Lag compensation** (`physics.rs`): `get_rewound_player_position()` + `lag_compensation_ms` for server-side hit registration at the shooter's perceived time — significant FPS quality improvement
