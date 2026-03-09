# Massive V5 Recommendations - Next-Level Multiplayer 2D Experience

**Date:** 2026-02-22
**Scope:** Full project review with gameplay, technical, and social/competitive enhancement recommendations
**Baseline:** V4 task board complete (V4-R2 through V4-R5 done, V4-R1 120-client target at 96/120)

---

## Current State Assessment

### Architecture Grade: A- (9/10)
The server infrastructure is production-quality. Highlights:
- 60Hz tick rate with parallel stage pipeline (input/AI -> physics -> broadcast)
- Dual transport (WebRTC + QUIC) with adaptive quality scaling
- SIMD-accelerated spatial queries, sharded DashMaps, zero-copy FlatBuffers
- AoI culling + delta compression + priority-based state sync
- Comprehensive observability (Prometheus, OpenTelemetry, frame timing, deadlock detection)
- WASM-sandboxed AI arena with ELO ratings

### Performance Grade: A- (9/10)
- 400 max players per match, stable at ~96 concurrent browser clients
- 16ms tick budget with soft budget logging at 12ms
- Object pools, arena allocators, optional jemalloc
- Adaptive broadcast batching with tail-join policies
- SIMD physics (AVX2 + NEON) for spatial filtering

### Gameplay Grade: D+ (4/10)
This is the bottleneck. Critical issues remain unfixed:
- No player-player collision (players stack on same position)
- No damage falloff (Rifle dominates all ranges at 100 DPS)
- No line-of-sight validation (bots/players can damage through walls)
- Bot pathfinding is 3-angle detour hack, not real navigation
- Shotgun severely underpowered (7 dmg/pellet vs Sniper 50)
- Predictive model memory leak on player disconnect

### Security Grade: C (5/10)
- Aimbot detection is rotation-speed only (no accuracy tracking)
- No server-side LOS check before applying projectile damage
- Position validation exists but is permissive
- No client-side prediction validation

---

## Phase 1: Fix Fundamentals

These are gameplay-breaking issues. Nothing else matters until they're resolved.

### 1.1 Player-Player Collision & Soft Push

**Problem:** Players and bots occupy the same position. This breaks tactical positioning, spawn safety, melee combat, and bot formations. In a 400-player arena, large groups collapse into a single point.

**Recommendation:** Circle-circle collision with soft repulsion. When two players overlap, apply a separation force proportional to overlap depth. Players slide around each other rather than hard-blocking (avoids deadlocks in corridors).

**Implementation approach:**
- After movement processing in the physics stage, query the spatial index for nearby players within `2 * PLAYER_RADIUS`
- For each overlapping pair, compute separation vector and apply half to each player
- Cap separation velocity to prevent physics explosions
- Skip collision for recently-spawned players (within spawn protection window)

**Files affected:** `server/src/server/instance.rs` (process_player_movement_optimized), `server/src/systems/physics/collision.rs`

**Risk:** O(n) per player with spatial index, but same cost as existing wall collision. No performance concern at 400 players with the existing quadtree.

**Priority:** Critical

---

### 1.2 Per-Weapon Damage Falloff

**Problem:** Rifle deals 100 DPS at all ranges with no falloff, making it strictly superior to every other weapon. Shotgun is unusable beyond 80 units.

**Recommendation:** Add a distance-based damage multiplier to the damage pipeline. Each weapon gets an optimal range, falloff start distance, and minimum damage percentage.

**Proposed balance table:**

| Weapon | Base Damage | Optimal Range | Falloff Start | Min Damage % | Effective DPS at 300u |
|--------|------------|---------------|---------------|-------------|----------------------|
| Pistol | 8 | 0-150 | 150 | 60% | 8.0 |
| Shotgun | 12 (up from 7) | 0-80 | 40 | 10% | ~0 (pellet spread) |
| Rifle | 10 | 0-200 | 200 | 40% | 40.0 |
| Sniper | 50 | 100-600 | 600 | 80% | 41.7 |
| Melee | 30 | 0-25 | N/A | 100% | 0 (out of range) |

**Key changes:**
- Shotgun pellet damage: 7 -> 12 (makes close-range devastating, distinct from Rifle)
- Rifle gets steep falloff beyond 200u (no longer a sniper replacement)
- Sniper maintains damage at range (clear long-range role)
- Melee range reduced from 50 -> 30 units

**Formula:** `effective_damage = base_damage * max(min_pct, 1.0 - (distance - falloff_start) / (max_range - falloff_start))`

**Files affected:** `server/src/systems/combat/damage.rs`, `server/src/systems/combat/weapons.rs`, `server/src/core/constants.rs`

**Priority:** Critical

---

### 1.3 Line-of-Sight Raycast Validation

**Problem:** Damage is applied without checking if shooter has clear line of sight to target. Bots shoot through walls. Players behind cover still take damage if they were visible when the shooter started firing.

**Recommendation:** Before applying projectile-player damage, cast a ray from projectile origin to target position. If any wall AABB intersects the ray segment, discard the hit. Use the existing `wall_spatial_index` to make this efficient.

**Implementation approach:**
- Add `fn has_line_of_sight(from: (f32, f32), to: (f32, f32), wall_index: &WallSpatialIndex) -> bool`
- Query walls in the AABB bounding box of the ray segment
- For each candidate wall, perform ray-AABB intersection test
- Call this in the projectile-player collision path and in melee hit detection

**Files affected:** `server/src/systems/physics/collision.rs` (add raycast), `server/src/server/instance.rs` (projectile hit path), `server/src/server/instance/combat_melee.rs`

**Performance:** Wall spatial index query is already O(log n). Ray-AABB test is trivial per wall. Negligible cost.

**Priority:** Critical

---

### 1.4 A* Pathfinding on Navigation Grid

**Problem:** Bot pathfinding is a 3-angle obstacle avoidance hack (`calculate_warzone_path`). Bots cannot navigate around complex wall configurations, find alternate routes, or plan multi-waypoint paths. They frequently get stuck.

**Recommendation:** Generate a grid-based navigation map from wall positions. Run A* on the grid for bot path planning. Cache paths and invalidate only when walls are destroyed.

**Implementation approach:**
- At map generation/load time, rasterize walls onto a 2D boolean grid (e.g., 10-unit cell size -> 160x120 grid for 1600x1200 world)
- Implement A* with 8-directional movement on the grid
- Cache paths per-bot with a generation counter tied to wall destruction events
- Limit A* iterations per frame (e.g., 200 nodes) and spread across ticks if needed
- The `navigation.rs` file already exists with navmesh stubs - build on this

**Files affected:** `server/src/world/navigation.rs`, `server/src/systems/ai/optimized_bot_ai.rs`, `server/src/systems/ai/bot_ai.rs`

**Performance:** A* on 160x120 grid is microseconds per path. With 80 bots recalculating every 2-5 seconds, total cost is negligible vs. physics.

**Priority:** High

---

## Phase 2: Player Expression & Game Feel

These features create the moment-to-moment decision-making that makes players want to keep playing.

### 2.1 Movement Abilities

**Problem:** Movement is WASD only. No skill expression, no escape plays, no outplay potential. Every fight is determined by aim + weapon choice.

**Recommendation:** Add 2-3 movement abilities using the existing `use_ability_slot` field in the PlayerInput schema (already defined in `game.fbs` but unused).

**Proposed abilities:**
- **Dash (slot 1):** 2x speed burst for 0.2 seconds in current movement direction. 8-second cooldown. Creates juking and escape plays.
- **Dodge Roll (slot 2):** Brief invulnerability (0.3s) with a fixed-distance roll in movement direction. 12-second cooldown. High-skill defensive option.

**Server-side implementation:** Modify velocity for a few frames, track cooldown timer on PlayerState, add invulnerability flag for dodge roll.

**New PlayerState fields needed:**
- `ability_1_cooldown_remaining: f32`
- `ability_2_cooldown_remaining: f32`
- `is_invulnerable: bool` (for dodge roll frames)

**Files affected:** `server/src/core/types.rs` (PlayerState), `server/src/server/instance/input_runtime.rs` (process abilities), `server/schemas/game.fbs` (add fields to PlayerState), `server/src/core/constants.rs` (ability constants)

**Priority:** High

---

### 2.2 Weapon Loadout System

**Problem:** `change_weapon_slot` exists in the input schema but the system is minimal. No pre-game strategy, no in-combat weapon switching decisions.

**Recommendation:** Give each player 2 weapon slots chosen at spawn. Add a weapon-swap delay (0.3 seconds) to create tactical switching decisions.

**Design:**
- Default loadout: Rifle + Pistol
- Players select loadout on respawn (or from a simple UI)
- Weapon crate pickups replace the current slot's weapon
- Weapon swap has a 0.3s animation delay (no shooting during swap)
- Bots choose loadouts based on their AI role (defenders prefer Shotgun+Melee, attackers prefer Rifle+Sniper)

**New PlayerState fields:**
- `secondary_weapon: ServerWeaponType`
- `secondary_ammo: i32`
- `weapon_swap_progress: f32` (0.0 = not swapping, counts down from 0.3)

**Files affected:** `server/src/core/types.rs`, `server/src/server/instance/input_runtime.rs`, `server/src/systems/combat/weapons.rs`, `server/schemas/game.fbs`

**Priority:** Medium

---

### 2.3 Environmental Hazards

**Problem:** Maps are static rectangles with walls. No environmental variety, no map control incentives beyond flag positions.

**Recommendation:** Add 2-3 zone types to the map generator that create permanent map features:

- **Slow Zone:** Reduces movement speed by 40%. Placed at chokepoints to control map flow. Visual: dark ground texture.
- **Damage Zone:** 5 DPS to players inside. Placed around high-value pickups to create risk/reward. Visual: red/orange ground.
- **Boost Pad:** Launches player in a fixed direction at 2x speed for 0.5s. Placed on flanking routes. Visual: arrow on ground.

**Implementation:** Zones are axis-aligned rectangles like walls but with different collision behavior. Store in a `Vec<Zone>` alongside walls. Check player-zone overlap during movement processing using the same spatial query pattern as pickups.

**Schema addition:**
```
enum ZoneType: byte { SlowZone = 0, DamageZone = 1, BoostPad = 2 }
table Zone { id: string; x: float; y: float; width: float; height: float; zone_type: ZoneType; direction: float; }
```

**Files affected:** `server/src/core/types.rs` (Zone struct), `server/src/world/map_generator.rs` (place zones), `server/schemas/game.fbs` (Zone table), `server/src/server/instance.rs` (zone effects during physics)

**Priority:** Medium

---

## Phase 3: Retention & Competitive Systems

These features keep players coming back after the first session.

### 3.1 Kill Cam (Death Replay)

**Problem:** When a player dies, they see nothing about what happened. No learning, no "how did they do that" moment. Missing the most important retention mechanic in shooters.

**Recommendation:** On death, send the victim the last 3 seconds of their killer's state (position + rotation + weapon + shooting flag, sampled every 100ms = 30 data points). The client renders a ghost replay.

**Implementation:** The server already tracks position history in `InterpolationBuffer` (32 samples). On a kill event:
1. Collect killer's last 30 position/rotation samples
2. Package into a new `KillCamData` message
3. Send to the victim as part of the kill event

**Bandwidth:** ~30 samples * 16 bytes (x, y, rotation, shooting) = 480 bytes per death. Negligible.

**Schema addition:**
```
table KillCamSample { x: float; y: float; rotation: float; shooting: bool; timestamp: ulong; }
table KillCamData { samples: [KillCamSample]; killer_name: string; weapon: WeaponType; }
```

**Files affected:** `server/schemas/game.fbs`, `server/src/server/instance.rs` (on kill, collect killer history), serialization paths

**Priority:** High

---

### 3.2 Post-Match Stats Screen

**Problem:** Match ends with no summary. Players don't see their performance, can't compare with others, have no reward signal. This is the #1 retention gap.

**Recommendation:** At match end, compile per-player stats and send a `MatchEndSummary` message. The server already tracks kills, deaths, score. Add damage dealt/taken tracking during the match.

**Stats to include:**
- Player's kills, deaths, K/D ratio
- Damage dealt, damage taken
- Score and rank in match
- Flag captures/returns (CTF)
- MVP awards: Most Kills, Most Damage, Best K/D, Most Objectives
- Weapon breakdown: kills per weapon

**New tracking needed (per-player, per-match):**
- `damage_dealt: i32`
- `damage_taken: i32`
- `flag_captures: i32`
- `flag_returns: i32`
- `kills_per_weapon: [i32; 5]` (one per weapon type)

**Schema addition:**
```
table PlayerMatchStats {
  player_id: string; player_name: string;
  kills: int; deaths: int; score: int;
  damage_dealt: int; damage_taken: int;
  flag_captures: int; flag_returns: int;
  weapon_kills: [int];
}
table MatchEndSummary {
  players: [PlayerMatchStats];
  mvp_kills: string; mvp_damage: string; mvp_objectives: string;
  match_duration: float;
  winning_team: byte;
}
```

**Files affected:** `server/src/core/types.rs` (add tracking fields), `server/src/systems/combat/damage.rs` (track damage), `server/src/systems/objectives/ctf.rs` (track captures), `server/schemas/game.fbs`, match lifecycle in `server/src/server/lifecycle.rs`

**Priority:** High

---

### 3.3 Skill-Based Matchmaking (SBMM)

**Problem:** New players get destroyed by experienced ones. No progression feeling. No appropriate challenge level.

**Recommendation:** Compute a simple MMR from existing tracked stats. The auth system already stores `cumulative_score`, `total_kills`, `total_deaths`, `matches_played`. Use these for matchmaking.

**MMR formula (simple starting point):**
```
mmr = (total_kills / max(total_deaths, 1)) * 100 + (cumulative_score / max(matches_played, 1)) * 0.5
```

**Matchmaking flow:**
1. Player authenticates -> load MMR from profile
2. On join request, `scaling/router.rs` routes to the match instance closest to their MMR band
3. MMR bands: Bronze (0-100), Silver (100-250), Gold (250-500), Diamond (500+)
4. If no match exists in their band, create one or relax band by 1 tier

**Prerequisite:** Multi-instance deployment via `scaling/coordinator.rs` (already has rendezvous hashing and shard assignment)

**Files affected:** `server/src/operational/auth.rs` (MMR calculation), `server/src/scaling/router.rs` (MMR-based routing), `server/src/core/types.rs` (MMR on player profile)

**Priority:** Medium (depends on multi-instance deployment)

---

## Phase 4: Social & Community Features

### 4.1 Team Ping System

**Problem:** No way to communicate tactical intent quickly. Chat is too slow for real-time coordination.

**Recommendation:** Players tap a location to broadcast a "look here" marker visible to teammates for 5 seconds.

**Implementation:**
- New input field: `ping_x: float, ping_y: float` (non-zero = ping at location)
- Server validates ping position is within world bounds
- Broadcasts as a `GameEvent` with new type `TeamPing` to teammates only
- Rate limit: 1 ping per 3 seconds per player
- Client renders a pulsing marker at the pinged location

**Schema changes:** Add `TeamPing` to `GameEventType` enum. Ping position carried in existing `GameEvent.position` field.

**Files affected:** `server/schemas/game.fbs` (GameEventType), `server/src/server/instance/input_runtime.rs` (process ping input), broadcast filtering (team-only events)

**Priority:** Medium

---

### 4.2 Spectator Mode

**Problem:** `Team::Spectator` exists but isn't implemented. No way to watch matches, which blocks tournaments and content creation.

**Recommendation:** Spectators connect normally but:
- Don't count toward player limits
- Receive full AoI with no team filter (see all players)
- Cannot send input (movement/shooting ignored)
- Can free-camera by sending position-only input
- Have a dedicated spectator slot cap (e.g., 20)

**Implementation:** On join, if player selects spectator team:
- Skip spawn logic
- Add to broadcast list with expanded AoI
- Filter out their input in `process_network_input()`

**Files affected:** `server/src/network/signaling.rs` (spectator join flow), `server/src/server/instance/broadcast_loop.rs` (spectator AoI), `server/src/server/instance/input_runtime.rs` (skip spectator input)

**Priority:** Medium

---

### 4.3 Public Bot Arena

**Problem:** The WASM bot arena is the most unique feature in the codebase but it's an internal tool. This could be a major differentiator.

**Recommendation:** Expose the arena as a public-facing feature:
- Web-based Rust code editor (Monaco editor) for writing bot AI
- Submit to the ELO ladder via the existing arena API
- Watch your bot fight in real-time via spectator mode
- Leaderboard showing top bots by ELO

**This is primarily a frontend feature** - the backend already supports it via `operational/arena.rs` and `operational/bot_sandbox.rs`. The work is:
- Static HTML page with Monaco editor + FlatBuffers template
- REST API endpoint to submit source code (already have `code_generation.rs`)
- Compile to WASM server-side (already have the Wasmtime pipeline)
- Display ELO leaderboard from `arena_store.json`

**Files affected:** `static_client/arena.html` (frontend), `server/src/operational/arena.rs` (public API endpoints)

**Priority:** Medium

---

## Phase 5: Technical Excellence & Scale

### 5.1 Close the 120-Client Gap

**Problem:** V4-R1 is stuck at 96/120 concurrent browser clients. The bottleneck is signaling admission during tail-wave joins.

**Root cause analysis:** When 20+ clients join simultaneously:
1. Each SDP exchange blocks a signaling task
2. Initial state serialization floods the broadcast budget
3. Late joiners' data channels open after the server has moved on

**Recommendations:**
- **Join admission gate:** Max 4 concurrent SDP exchanges at a time. Queue the rest with a position indicator sent back to the client.
- **Staggered initial state:** Split InitialState into chunks (walls first, then players, then pickups) across 3 frames instead of 1 monolithic message.
- **Pre-warm client state:** Allocate `ClientState` structures in a warm pool before the SDP exchange completes, so the first broadcast frame doesn't have allocation overhead.
- **Separate signaling from game tick:** Move SDP processing to a dedicated Tokio task that doesn't compete with the game loop's IO budget.

**Files affected:** `server/src/network/signaling.rs`, `server/src/server/instance/broadcast_loop.rs`, `server/src/server/instance/broadcast_state.rs`

**Priority:** High

---

### 5.2 Server-Side Replay Recording

**Problem:** The arena system records live replays in memory but they're not persisted. No ability to review matches for anti-cheat, community highlights, or ML training data.

**Recommendation:** Write match replays to disk as compressed FlatBuffers frame streams.

**Design:**
- On match start, open a `BufWriter<File>` for the replay
- Every N frames (e.g., every 3rd frame = 20 snapshots/sec), write a compressed delta snapshot
- On match end, flush and close the file
- File format: header (match metadata) + sequence of zstd-compressed delta frames
- Retention: keep last 100 replays, delete oldest

**Storage estimate:** 20 snapshots/sec * ~2KB per snapshot * 600 sec match = ~24 MB per match (compressed ~6 MB).

**Files affected:** `server/src/operational/backup.rs` (replay storage), `server/src/server/lifecycle.rs` (start/stop recording), new `server/src/server/instance/replay_writer.rs`

**Priority:** Medium

---

### 5.3 Map Editor & Custom Maps

**Problem:** Only procedural maps are available. User-generated content is the highest-ROI content system for a 2D multiplayer game.

**Recommendation:** Build a simple web-based map editor that outputs the JSON format already supported by `map_loader.rs`.

**Design:**
- Canvas-based editor in a static HTML page
- Place/resize/delete walls and pickups with mouse
- Toggle destructible flag on walls
- Export as JSON matching the `MapFile` schema
- Import existing JSON maps for editing
- Server accepts custom map JSON on match creation

**The backend already supports this** - `load_map_from_json()` parses the exact format the editor would produce. The work is entirely frontend.

**Files affected:** New `static_client/editor.html`, potentially `server/src/network/signaling.rs` (accept map JSON on match creation)

**Priority:** Low

---

### 5.4 Authoritative ECS Migration

**Problem:** Game logic is spread across `instance.rs` (6300+ lines) and various `systems/` modules. The hecs ECS bridge exists with `Authoritative` mode but isn't the default. This makes adding new entity types expensive.

**Recommendation:** Incrementally migrate game logic into ECS systems:
1. Start with pickup collection (already decomposed into `pickup_pipeline.rs`)
2. Move projectile physics into an ECS system
3. Move player movement into an ECS system
4. Eventually make `Authoritative` the default mode

**Benefits:**
- Trivial to add new entity types (turrets, vehicles, NPCs)
- System-level parallelism via hecs query scheduling
- Cleaner separation of concerns
- Easier testing (systems are pure functions on components)

**This is a multi-version effort.** Recommend one system per version cycle.

**Files affected:** `server/src/server/ecs_bridge.rs`, `server/src/server/instance.rs`, `server/src/systems/`

**Priority:** Low (foundational, not urgent)

---

## Phase 6: Differentiation Features

These features would make the game stand out from other 2D multiplayer shooters.

### 6.1 Progressive Destructible Terrain

**Problem:** Walls are destructible but binary (alive/dead). No visual progression of destruction. No emergent cover creation.

**Recommendation:** Walls break into smaller pieces when damaged:
- Wall at 75% health -> surface cracks (visual only)
- Wall at 50% health -> splits into 2 smaller walls with a gap
- Wall at 25% health -> fragments further, gaps widen
- Wall at 0% health -> fully destroyed

**This creates emergent gameplay** - 400 players gradually reshape the battlefield. Cover that existed at match start is gone by the end. New sightlines and flanking routes open up organically.

**Implementation:** On wall damage below 50%, replace the wall entity with 2 smaller wall entities with a gap. Track "parent wall ID" for respawn purposes. When parent respawns, remove children and restore original.

**Files affected:** `server/src/systems/physics/collision.rs` (wall splitting), `server/src/core/types.rs` (parent wall tracking), `server/src/systems/respawn.rs` (wall reassembly)

**Priority:** Low

---

### 6.2 Commander Mode

**Problem:** Large-scale matches (100+ players) have no strategic coordination. Individual players can't influence the overall flow of battle.

**Recommendation:** One player per team can opt into a top-down strategic view:
- See all teammate positions on a zoomed-out map
- Place up to 3 waypoint markers (attack/defend/rally) visible to all teammates and bots
- Adjust bot role distribution (attack/defend ratio slider)
- Call in supply drops (spawn a cluster of pickups at a location, 60s cooldown)

**The `commander.rs` file already has tactical coordination stubs.** Bots already have role-based behavior (AttackEnemyFlag, DefendOwnFlag, etc.). The commander just controls the role distribution and adds waypoints.

**Files affected:** `server/src/systems/ai/commander.rs` (waypoint system), `server/src/systems/ai/optimized_bot_ai.rs` (follow waypoints), `server/schemas/game.fbs` (commander input/state), `server/src/server/instance/input_runtime.rs` (commander input processing)

**Priority:** Low

---

### 6.3 Dynamic Game Mode Transitions

**Problem:** Game mode is static for the entire match. Long matches become monotonous.

**Recommendation:** Mid-match mode shifts based on conditions:
- Match starts as **FreeForAll** (warm-up, 2 minutes)
- When player count stabilizes, transition to **TeamDeathmatch** (teams assigned by score proximity)
- Final phase: **CaptureTheFlag** (5-minute round, flags spawn at team bases)
- Transition announcements with 10-second countdown

**The schema already supports this** - `GameModeType` has all three modes, and `MatchStateType` can signal transitions. The work is game logic in the match lifecycle.

**Files affected:** `server/src/server/lifecycle.rs` (mode transition logic), `server/src/systems/objectives/` (mode-specific scoring reset), `server/schemas/game.fbs` (transition event type)

**Priority:** Low

---

## Bug Fixes (From Existing GAME_SYSTEMS_REVIEW.md)

These should be addressed alongside Phase 1 work:

| Bug | File | Fix |
|-----|------|-----|
| Predictive model DashMap grows unbounded on player disconnect | `optimized_bot_ai.rs:78` | Add cleanup in player disconnect handler, prune entries older than 60s |
| Bot stuck detection false-positives on defenders | `optimized_bot_ai.rs:913` | Skip stuck check when behavior state is `Defending` and position is near flag |
| `partial_cmp().unwrap()` panics on NaN distances | `bot_ai.rs:371` | Use `partial_cmp().unwrap_or(Ordering::Equal)` |
| `path_recalculation_timer` repurposed for weapon switching | `optimized_bot_ai.rs:574` | Rename or use separate timer field |
| No clamp on `future_timestamp_ms` in prediction | `commander.rs:26` | Clamp to max 2 seconds ahead |
| Empty physics module files | `movement.rs`, `ballistics.rs` | Either populate with extracted logic or remove |
| Negative damage could heal targets | `damage.rs:12` | Already clamped with `.max(0)` but `_weapon` param unused |

---

## Implementation Priority Matrix

| Phase | Items | Effort | Impact | Dependencies |
|-------|-------|--------|--------|-------------|
| **1: Fundamentals** | 1.1-1.4 | 2-3 weeks | Critical | None |
| **2: Expression** | 2.1-2.3 | 2-3 weeks | High | Phase 1 (collision needed for abilities) |
| **3: Retention** | 3.1-3.3 | 2-3 weeks | High | Phase 1 (balance needed for fair stats) |
| **4: Social** | 4.1-4.3 | 2-3 weeks | Medium | Phase 1-2 |
| **5: Technical** | 5.1-5.4 | 4-6 weeks | Medium-High | 5.1 is independent, rest can parallel |
| **6: Differentiation** | 6.1-6.3 | 4-6 weeks | Medium | Phase 1-2 |

**Recommended execution order:** Phase 1 -> Phase 5.1 (120-client, can parallel) -> Phase 2 -> Phase 3 -> Phase 4+5 interleaved -> Phase 6

---

## Success Metrics

| Metric | Current | Target | How to Measure |
|--------|---------|--------|----------------|
| Concurrent browser clients | 96 stable | 120+ stable | Scale test scripts |
| Average session length | Unknown | 15+ minutes | Server-side session tracking |
| Return rate (same player, next day) | Unknown | 30%+ | Auth session analysis |
| Weapon usage distribution | ~80% Rifle | <40% any single weapon | Kill feed analysis |
| Bot navigation success rate | ~60% (frequent stucks) | 95%+ | Stuck detection counter |
| Player-reported "fairness" | N/A | Qualitative from playtests | Survey |
