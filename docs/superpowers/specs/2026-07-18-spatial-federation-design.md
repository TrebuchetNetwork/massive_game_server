# VOID STRIKE — Spatial Federation Design

**Date:** 2026-07-18
**Status:** Approved for planning
**Repo:** TrebuchetNetwork/massive_game_server

## 1. Goal

Evolve the game server from a single world per process into a **federation of server processes sharing one continuous game world**:

- Multiple servers stitch together one extended map.
- Players are spawned on the best (least-loaded, later: lowest-latency) server.
- Players cross server boundaries seamlessly and fight players hosted on other servers in real time.
- Servers with no human players consolidate (reduced cost) and wake on demand.
- The world has no edges: it wraps in every direction and grows tile by tile.
- Universes (distinct themed torus worlds) connect through a playable transit hub — **the Between** — with travel as a real, risky traversal.
- Every universe (and the Between) exposes a public live dashboard.

**Deployment target:** single site (one datacenter/machine) for the first milestone, with protocol and identity models that don't block geo-distribution later.

**Non-goals (first milestone):** elastic/dynamic space growth (the grid is static per deployment), match-per-server sharding, cross-datacenter consistency, client multi-connection.

## 2. Architecture overview (Approach A: server-side mirroring, single client connection)

Each server process is **authoritative over exactly one region tile** of a shared toroidal world. Clients keep exactly one WebRTC/WebSocket connection to their home server. Servers run a **neighbor mesh** (persistent control+state channels to their 4 grid neighbors) over which they exchange heartbeats, **ghost (read-only mirror) entities** for the border band, **handoff requests**, and **combat claims**.

Redis (already deployed) is the rendezvous: it holds the Master Map, region heartbeats/load metrics, and the shared match epoch. No consensus algorithm — "consensus" = all servers and clients agree on topology, epoch, and ownership via Redis + a static grid.

## 3. The Master Map (topology)

- **Tiles:** the world is a grid of equal-sized tiles; tile size is fixed at 1600×1200 (today's map = one tile). Each tile is owned by one region/server.
- **Torus wrap ("round vertically and horizontally"):** no outer walls. The north edge of the top row wraps to the south edge of the bottom row; west of the left column to east of the right column. Crossing any edge always continues the world — *there is always continuation*.
- **Neighbor rule (mechanical):** for tile `(x, y)` in a `cols × rows` grid, `neighbor(N) = (x, (y-1) mod rows)`, `neighbor(S) = (x, (y+1) mod rows)`, `neighbor(E) = ((x+1) mod cols, y)`, `neighbor(W) = ((x-1) mod cols, y)`. Every tile always has 4 neighbors, including a 1×1 world (it is its own neighbor).
- **The "cushion":** crossing is not a teleport pop. The 200px ghost band across every border provides visual continuity; authority handoff happens underneath while the player keeps moving. Wrap edges use the identical mechanism as interior borders.
- **Master Map as data:** the grid layout (`tile → region → server endpoint`) is one small versioned document stored in Redis (`world:master_map`), seeded from config (`MGS_FEDERATION_GRID`, `MGS_REGION_ID`, `MGS_REGION_PEERS`, `MGS_TILE_SIZE`). Servers watch it; clients can fetch it via `GET /api/master_map`.
- **Growth sequence ("square as much as possible"):** the grid grows one tile at a time along a square-preserving sequence: 1×1 → 2×1 → 2×2 → 3×2 → 3×3 → 4×3 → 4×4 … Each joining server claims the next tile; wrap edges recompute from the modular rule. (Growth is an ops action in this milestone, not automatic.)
- **Coordinates:** one global space of `cols·tile_w × rows·tile_h`; positions canonicalize modulo world size. A ghost at `x = -50` in a 3200-wide world renders at `x = 3150` on the neighbor.

## 4. Components

### 4.1 Region runtime (per server)
- World bounds become **runtime values** derived from the Master Map (`WORLD_MIN/MAX` compile-time constants are replaced in region-aware code paths; boundary clamping wraps instead of clamping).
- Deterministic map regeneration from shared seed (`MGS_MAP_SEED`) so any server can rebuild any tile's static geometry after a crash.

### 4.2 Placement service
- Join requests hit any server's `/ws`. The server reads region loads from Redis (`region:<id>:load`: humans, tick_ms, draining flag; 1s updates, 5s TTL) and either accepts the player or answers with `Redirect { ws_url, reason, token }` naming the least-loaded healthy region.
- `Redirect` carries a short-lived HMAC token (`MGS_FEDERATION_SECRET`) so clients can't be bounced to hostile servers.
- Client change: honor `Redirect` (reconnect signaling + data channel to the given URL). Only protocol change required on the client.

### 4.3 Neighbor mesh
- Each server maintains a persistent QUIC (fallback: WebSocket) channel to each of its 4 neighbors.
- Message kinds: `Heartbeat`, `GhostUpdate`, `HandoffRequest`/`HandoffAck`, `HitClaim`, `RegionStatus`.

### 4.4 Ghost mirroring (seamless view)
- Each server tracks entities (players, projectiles) within a **ghost band** (default 200px) inside its borders — including across wrap edges — and sends the neighbor compact ghost updates at 20Hz (id, pos, rotation, velocity, health, weapon, team).
- Neighbors render ghosts as read-only proxy entities (`is_ghost` flag) through the existing interpolation/AoI path; no local simulation touches them.
- The wall-state resync (2026-07-18 fix) keeps ghost wall state consistent under packet loss.

### 4.5 Crossing / handoff
When a simulated player crosses a border (25px hysteresis):
1. Home server serializes player state (position, velocity, health, ammo, loadout, input queue) → `HandoffRequest` to the neighbor.
2. Neighbor spawns the player as a full local player, acks with the player-id mapping, and the client receives `Redirect` to reconnect to the new home server (~1s blip, covered by the existing reconnect path; ghost rendering prevents visible popping).
3. Old server downgrades the player to a ghost mirrored from the neighbor (roles invert), dropping them when out of band.
- Two-phase commit: the old server keeps authority until ack; on timeout the player stays with a soft push-back. A player can never exist authoritatively on two servers.

### 4.6 Cross-server combat
Authority follows the **target**, not the shooter:
- Ghost projectiles in the band are mirrored, so incoming fire from another server is visible in real time.
- When a shot would hit a ghost, the shooter's server forwards `HitClaim { projectile_id, target_ghost_id, position, direction, timestamp }` to the owner, which validates (range, rate, LoS sanity) and applies damage authoritatively, broadcasting results on both servers' deltas.
- Destructible walls are region-owned; damage to them near borders uses the same claim path.
- Kill events are emitted by the owner and merged into both servers' kill feeds.

**Bandwidth sanity:** ~30 players in a band × 20Hz × ~40B ≈ 24KB/s per neighbor link — trivial in one DC.

### 4.7 Match epoch
All servers run the same match timeline. Start/end timestamps live in Redis (`world:epoch`); regions tick the same `time_remaining` and restart together.

### 4.8 Empty-region consolidation
- 0 humans for 60s → region publishes `draining`, stops bot respawns and pickups, drops to a 20Hz tick; placement stops routing players there.
- Tile space remains (static grid); crossing still works; the region wakes to 60Hz when a human enters or is routed in.

### 4.9 Multiverse: the Universe Graph and the Between

Two levels of map. Level 1 is the Master Map (tiles of one torus world). Level 2 is the **Universe Graph**: each universe is its own toroidal federated world — own theme, map seed, optionally its own rule flavor — and the graph defines how they connect. Stored as one small versioned doc in Redis (`multiverse:graph`).

**The Between (hub universe).** Every gate leads into the Between; every exit in the Between leads out to a universe. Travel is always *universe → Between → universe*:
- The Between is itself a small world (one or two tiles) with its own server(s) in the same federation. It is a **live free-for-all world**: combat is allowed — travel is risky by design — but there are no bots and no match timer.
- Layout: a ring/lobby of **gate platforms**, one per linked universe, each marked with the destination's theme and live population. Travelers physically traverse to the exit they want — a few seconds of real traversal in a real place, crossing paths with other travelers.
- Dying in the Between respawns at the gate platform you entered from, so travel is always recoverable.
- Consolidation applies normally (sleeps when truly empty, wakes on arrival), but placement keeps at least one Between region registered whenever any universe is up.

**Gates are world objects.** A gate is a zone entity with a `link_id`, placed deterministically from the universe's map seed so all players share the same landmarks. Entering a gate starts a ~1.5s channel (broken by damage outside the Between), then the **same two-phase handoff as region crossing** runs — full player state (position, health, ammo, loadout, identity) serialized to the target server. No new mechanism, just longer distance between endpoints.

**Carry-over:** identity, loadout, health, and persistent stats (auth layer). Match score and kill feed are per-universe. Arrival lands at the destination universe's linked gate, on the least-loaded region via placement.

**Transit failure:** failed handoff returns the player to the origin gate with 3s spawn protection — never lost in the void. If the Between is down, gates in all universes show as closed instead of dropping players into limbo.

**Phase-2 (not v1):** see-through gates (ghost-render the action on the other side near the gate) and temporary event universes (tournaments) that light up in the graph.

### 4.10 World dashboards

- **Per-universe status endpoint:** public-safe `GET /api/universe/status` on any region server (aggregated via Redis): tile loads, tick rates, player counts, match state, draining/wake status, federation link health (ghost traffic, handoffs/min, claim latency), and per-gate entries/exits with destinations.
- **Dashboard page:** lightweight `/dashboard.html` in the existing `website/` UI (no framework) rendering the universe's torus grid as a live tile map (colored by load/health) plus a vitals panel, auto-refreshing every few seconds. The Between gets the same dashboard with travelers-in-transit and per-gate throughput.
- **Multiverse overview:** `/universes.html` reads the Universe Graph and shows every universe as a card (theme, population, status, dashboard link) — doubles as the player's travel guide.
- **Public vs ops:** dashboards are public and read-only; deep ops detail (CPU, memory, tick histograms, alerts) stays in the existing Grafana/Prometheus stack.

## 5. Error handling

- **Neighbor link down:** ghosts freeze ≤2s, then fade out; peer marked unhealthy via heartbeat TTL.
- **Server crash:** its players disconnect and rejoin via placement; on restart the server reclaims its tile and regenerates geometry from the shared seed.
- **Handoff timeout:** player stays on the old server with soft push-back (no dual authority).
- **Redis down:** servers keep last-known Master Map and epoch (degraded but playable); placement falls back to accept-locally.

## 6. Testing

- **Rust integration tests:** handoff serialization round-trip, hit-claim validation, epoch sync, placement logic, wrap-edge neighbor math (modular arithmetic incl. 1×1 self-neighbor).
- **e2e (`scripts/e2e`):** boot two servers (region-a/region-b) and verify: placement redirect, ghost visibility across the border (spectator oracle on both), crossing (player ends up hosted on B with <25px position jump), cross-border wall damage, packet-loss heal on ghost state (extends `wall_packet_loss_heal.spec.js` pattern).
- **Load:** stress-client bots straddling a border; ghost bandwidth measurement.

## 7. Repo cleanup (workstream 0, independent PR)

- Delete 9 stale client variants in `static_client/` (`client_optimized*.html`, `client_ultra.html`, `client_stable.html`, `index_legacy.html`, `arena.html`, `editor.html` as applicable) and `static_client/archive/legacy_clients/` (15 files).
- Remove stray root artifacts (`lcov.info`, stale review docs at root — keep `docs/`), add missing `node_modules/` ignores.
- Document the canonical client tree (`client_logic/` JS vs `client_logic_ts/` TS) and archive the redundant one.
- Land as a small reviewable PR before federation work begins.

## 8. Rollout

0. **Cleanup PR** (section 7).
1. **Runtime world bounds + Master Map config + Redis epoch** (single-server behavior unchanged on a 1×1 torus).
2. **Neighbor mesh + ghost mirroring** (observability only: ghosts rendered, no handoff).
3. **Crossing / handoff** (with client `Redirect` support).
4. **Cross-server combat claims.**
5. **Consolidation + growth runbook** (adding a tile).
6. **Multiverse** (Universe Graph, the Between hub, gates as zone objects, travel as long-distance handoff).
7. **World dashboards** (status endpoints, `/dashboard.html`, `/universes.html`).

Each step ships working software; steps 2–7 each get their own implementation plan.

## 9. Open risks

- Handoff edge cases (mid-dash/mid-reload state) — mitigate by serializing full player state and e2e testing those states.
- Ghost/projectile timing skew between servers — mitigated by shared epoch and validating hit claims on the owner with tolerance windows.
