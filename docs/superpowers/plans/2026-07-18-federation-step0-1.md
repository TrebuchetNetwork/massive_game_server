# Federation Step 0+1: Cleanup, Master Map, Runtime Bounds, Epoch — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clean the repo, then lay the federation foundation: a toroidal Master Map (1×1 by default), runtime world bounds, Redis-published Master Map + match epoch, and a wrap-mode boundary flag — with zero behavior change for the running single server.

**Architecture:** Follows `docs/superpowers/specs/2026-07-18-spatial-federation-design.md`. The Master Map is a small versioned document (grid of equal tiles on a torus, centered coordinates so 1×1 == today's world). Redis is the rendezvous; everything degrades to current behavior when Redis is absent.

**Tech Stack:** Rust (tokio, warp, redis 0.27 sync client, serde/serde_json), Node/Playwright (scripts/e2e), cargo test.

**Repo:** work in `/home/habitat/massive-game-server-deployment/massive_game_server` (canonical clone; push to origin/main when done).

---

## Task 1: Delete stale client variants

**Files:**
- Delete candidates: `static_client/client_fullscreen.html`, `static_client/client_fullscreen_optimized.html`, `static_client/client_mobile.html`, `static_client/client_optimized.html`, `static_client/client_optimized_fixed.html`, `static_client/client_optimized_fixed_complete.html`, `static_client/client_optimized_projectiles_fixed.html`, `static_client/client_stable.html`, `static_client/client_ultra.html`, `static_client/index_legacy.html`, `static_client/arena.html`, `static_client/editor.html`, `static_client/archive/legacy_clients/` (whole dir)

- [ ] **Step 1: Verify nothing references each candidate**

Run:
```bash
cd /home/habitat/massive-game-server-deployment/massive_game_server
for f in client_fullscreen client_fullscreen_optimized client_mobile client_optimized client_stable client_ultra index_legacy arena editor; do
  echo "== $f"; grep -rn "$f" --include="*.html" --include="*.js" --include="*.rs" --include="*.md" static_client/website static_client/index.html scripts/e2e/tests server/src 2>/dev/null | grep -v archive | grep -v "client_optimized" | head -3
done
```
Expected: only hits in the candidates themselves or docs. For any file referenced by a test or page (check `editor.html` and `arena.html` especially — `scripts/e2e/tests/custom_map.spec.js` may use the editor), REMOVE it from the delete list.

- [ ] **Step 2: Delete the unreferenced files and the legacy archive**

```bash
cd /home/habitat/massive-game-server-deployment/massive_game_server
git rm -r static_client/archive/legacy_clients
git rm static_client/client_fullscreen.html static_client/client_fullscreen_optimized.html \
  static_client/client_mobile.html static_client/client_optimized.html \
  static_client/client_optimized_fixed.html static_client/client_optimized_fixed_complete.html \
  static_client/client_optimized_projectiles_fixed.html static_client/client_stable.html \
  static_client/client_ultra.html static_client/index_legacy.html
# only if unreferenced in step 1:
# git rm static_client/arena.html static_client/editor.html
```

- [ ] **Step 3: Verify the live client still loads**

Run: `curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/client.html`
Expected: `200`

- [ ] **Step 4: Commit**

```bash
git commit -m "chore: delete stale client variants and legacy client archive"
```

## Task 2: Root hygiene

**Files:**
- Delete: `lcov.info`
- Modify: `.gitignore`

- [ ] **Step 1: Remove coverage artifact and confirm ignores**

```bash
cd /home/habitat/massive-game-server-deployment/massive_game_server
git rm -q lcov.info
grep -q "^lcov.info$" .gitignore || echo "lcov.info" >> .gitignore
git add .gitignore
```

- [ ] **Step 2: Commit**

```bash
git commit -m "chore: drop checked-in lcov.info and ignore it"
```

## Task 3: MasterMap module (torus grid math)

**Files:**
- Create: `server/src/world/master_map.rs`
- Modify: `server/src/world/mod.rs` (add `pub mod master_map;`)

Coordinates are **centered**: tile (0,0) of a 1×1 grid covers `[-800,800) × [-600,600)` — identical to today's world.

- [ ] **Step 1: Write the failing tests**

Append to the new file `server/src/world/master_map.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_tile_is_self_neighbor_in_all_directions() {
        let map = MasterMap::single_tile();
        let t = TileCoord { x: 0, y: 0 };
        for d in [Direction::North, Direction::East, Direction::South, Direction::West] {
            assert_eq!(map.neighbor(t, d), t);
        }
    }

    #[test]
    fn neighbor_wraps_on_torus() {
        let map = MasterMap { version: 1, cols: 2, rows: 2, tile_width: 1600.0, tile_height: 1200.0, map_seed: 7 };
        let t = TileCoord { x: 0, y: 0 };
        assert_eq!(map.neighbor(t, Direction::West), TileCoord { x: 1, y: 0 });
        assert_eq!(map.neighbor(t, Direction::East), TileCoord { x: 1, y: 0 });
        assert_eq!(map.neighbor(t, Direction::North), TileCoord { x: 0, y: 1 });
        assert_eq!(map.neighbor(t, Direction::South), TileCoord { x: 0, y: 1 });
        let br = TileCoord { x: 1, y: 1 };
        assert_eq!(map.neighbor(br, Direction::East), TileCoord { x: 0, y: 1 });
        assert_eq!(map.neighbor(br, Direction::South), TileCoord { x: 1, y: 0 });
    }

    #[test]
    fn growth_sequence_stays_squareish() {
        assert_eq!(MasterMap::next_growth(1, 1), (2, 1));
        assert_eq!(MasterMap::next_growth(2, 1), (2, 2));
        assert_eq!(MasterMap::next_growth(2, 2), (3, 2));
        assert_eq!(MasterMap::next_growth(3, 2), (3, 3));
        assert_eq!(MasterMap::next_growth(3, 3), (4, 3));
    }

    #[test]
    fn wrap_position_canonicalizes() {
        let map = MasterMap::single_tile();
        let (x, y) = map.wrap_position(-810.0, 610.0);
        assert!((x - 790.0).abs() < 1e-4);
        assert!((y - (-590.0)).abs() < 1e-4);
    }

    #[test]
    fn single_tile_rect_matches_legacy_world() {
        let map = MasterMap::single_tile();
        let (min_x, min_y, max_x, max_y) = map.tile_rect(TileCoord { x: 0, y: 0 });
        assert_eq!((min_x, min_y, max_x, max_y), (-800.0, -600.0, 800.0, 600.0));
    }

    #[test]
    fn tile_for_position_selects_correct_tile() {
        let map = MasterMap { version: 1, cols: 2, rows: 1, tile_width: 1600.0, tile_height: 1200.0, map_seed: 7 };
        assert_eq!(map.tile_for_position(-900.0, 0.0), TileCoord { x: 0, y: 0 });
        assert_eq!(map.tile_for_position(900.0, 0.0), TileCoord { x: 1, y: 0 });
        assert_eq!(map.tile_for_position(1599.9, 599.9), TileCoord { x: 1, y: 0 });
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/habitat/massive-game-server-deployment/massive_game_server && cargo test -p massive_game_server_core master_map 2>&1 | tail -5`
Expected: compile error — `MasterMap` not defined.

- [ ] **Step 3: Implement MasterMap**

Write `server/src/world/master_map.rs` above the tests:

```rust
use serde::{Deserialize, Serialize};

pub const DEFAULT_TILE_WIDTH: f32 = 1600.0;
pub const DEFAULT_TILE_HEIGHT: f32 = 1200.0;
pub const DEFAULT_MAP_SEED: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileCoord {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    North,
    East,
    South,
    West,
}

/// Toroidal grid of equal tiles; centered coordinates so a 1x1 grid
/// covers [-800,800) x [-600,600) exactly like the legacy world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterMap {
    pub version: u64,
    pub cols: u32,
    pub rows: u32,
    pub tile_width: f32,
    pub tile_height: f32,
    pub map_seed: u64,
}

impl MasterMap {
    pub fn single_tile() -> Self {
        Self {
            version: 1,
            cols: 1,
            rows: 1,
            tile_width: DEFAULT_TILE_WIDTH,
            tile_height: DEFAULT_TILE_HEIGHT,
            map_seed: DEFAULT_MAP_SEED,
        }
    }

    pub fn world_width(&self) -> f32 {
        self.cols as f32 * self.tile_width
    }

    pub fn world_height(&self) -> f32 {
        self.rows as f32 * self.tile_height
    }

    /// Torus neighbor; on a 1x1 grid every neighbor is the tile itself.
    pub fn neighbor(&self, tile: TileCoord, dir: Direction) -> TileCoord {
        match dir {
            Direction::North => TileCoord { x: tile.x, y: (tile.y + self.rows - 1) % self.rows },
            Direction::South => TileCoord { x: tile.x, y: (tile.y + 1) % self.rows },
            Direction::East => TileCoord { x: (tile.x + 1) % self.cols, y: tile.y },
            Direction::West => TileCoord { x: (tile.x + self.cols - 1) % self.cols, y: tile.y },
        }
    }

    /// Growth sequence 1x1 -> 2x1 -> 2x2 -> 3x2 -> 3x3 -> 4x3 -> 4x4 ...
    pub fn next_growth(cols: u32, rows: u32) -> (u32, u32) {
        if cols > rows {
            (cols, rows + 1)
        } else {
            (cols + 1, rows)
        }
    }

    /// Canonicalize a world-space position onto the torus.
    pub fn wrap_position(&self, x: f32, y: f32) -> (f32, f32) {
        let half_w = self.world_width() / 2.0;
        let half_h = self.world_height() / 2.0;
        (
            (x + half_w).rem_euclid(self.world_width()) - half_w,
            (y + half_h).rem_euclid(self.world_height()) - half_h,
        )
    }

    /// World-space rect of a tile (centered coordinates).
    pub fn tile_rect(&self, tile: TileCoord) -> (f32, f32, f32, f32) {
        let min_x = tile.x as f32 * self.tile_width - self.world_width() / 2.0;
        let min_y = tile.y as f32 * self.tile_height - self.world_height() / 2.0;
        (min_x, min_y, min_x + self.tile_width, min_y + self.tile_height)
    }

    pub fn tile_for_position(&self, x: f32, y: f32) -> TileCoord {
        let (wx, wy) = self.wrap_position(x, y);
        let tx = ((wx + self.world_width() / 2.0) / self.tile_width) as u32;
        let ty = ((wy + self.world_height() / 2.0) / self.tile_height) as u32;
        TileCoord {
            x: tx.min(self.cols - 1),
            y: ty.min(self.rows - 1),
        }
    }
}
```

And in `server/src/world/mod.rs` add:
```rust
pub mod master_map;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p massive_game_server_core master_map 2>&1 | tail -5`
Expected: `test result: ok. 6 passed`

- [ ] **Step 5: Commit**

```bash
git add server/src/world/master_map.rs server/src/world/mod.rs
git commit -m "feat(world): toroidal MasterMap with neighbor/growth/wrap math"
```

## Task 4: Federation env config

**Files:**
- Modify: `server/src/operational/config/env_registry.rs`

- [ ] **Step 1: Write the failing test**

Add to the tests module of `env_registry.rs` (create `#[cfg(test)] mod federation_env_tests` at file end if none exists):

```rust
#[cfg(test)]
mod federation_env_tests {
    use super::*;

    #[test]
    fn federation_env_defaults_to_single_tile() {
        let fed = FederationEnv::from_lookup(|_| None);
        assert_eq!(fed.grid, "1x1".to_string());
        assert_eq!(fed.region_id, "region-a".to_string());
        assert_eq!(fed.tile_width, 1600.0);
        assert_eq!(fed.tile_height, 1200.0);
        assert!(!fed.world_wrap);
    }

    #[test]
    fn federation_env_parses_overrides() {
        let fed = FederationEnv::from_lookup(|key| match key {
            "MGS_FEDERATION_GRID" => Some("2x2".to_string()),
            "MGS_REGION_ID" => Some("region-b".to_string()),
            "MGS_TILE_SIZE" => Some("800x600".to_string()),
            "MGS_MAP_SEED" => Some("42".to_string()),
            "MGS_WORLD_WRAP" => Some("1".to_string()),
            _ => None,
        });
        assert_eq!(fed.grid, "2x2".to_string());
        assert_eq!(fed.region_id, "region-b".to_string());
        assert_eq!(fed.tile_width, 800.0);
        assert_eq!(fed.tile_height, 600.0);
        assert_eq!(fed.map_seed, 42);
        assert!(fed.world_wrap);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p massive_game_server_core federation_env 2>&1 | tail -3`
Expected: compile error — `FederationEnv` not found.

- [ ] **Step 3: Implement FederationEnv**

In `env_registry.rs`, after the existing env structs, add:

```rust
#[derive(Debug, Clone)]
pub struct FederationEnv {
    pub grid: String,        // "COLSxROWS", e.g. "1x1", "2x1"
    pub region_id: String,   // this server's region name
    pub tile_width: f32,
    pub tile_height: f32,
    pub map_seed: u64,
    pub world_wrap: bool,    // wrap positions on the torus (off until ghosts exist)
}

impl FederationEnv {
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let (mut tw, mut th) = (1600.0_f32, 1200.0_f32);
        if let Some(size) = get("MGS_TILE_SIZE") {
            if let Some((w, h)) = size.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>()) {
                    tw = w;
                    th = h;
                }
            }
        }
        Self {
            grid: get("MGS_FEDERATION_GRID").unwrap_or_else(|| "1x1".to_string()),
            region_id: get("MGS_REGION_ID").unwrap_or_else(|| "region-a".to_string()),
            tile_width: tw,
            tile_height: th,
            map_seed: get("MGS_MAP_SEED")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1),
            world_wrap: matches!(get("MGS_WORLD_WRAP").as_deref(), Some("1") | Some("true")),
        }
    }

    pub fn grid_dims(&self) -> (u32, u32) {
        self.grid
            .split_once('x')
            .and_then(|(c, r)| Some((c.parse::<u32>().ok()?, r.parse::<u32>().ok()?)))
            .filter(|(c, r)| *c > 0 && *r > 0)
            .unwrap_or((1, 1))
    }
}
```

Then in `AppEnvConfig` add a field `pub federation: FederationEnv,` and in `load_app_env_config()` populate it:
```rust
federation: FederationEnv::from_lookup(|key| get_optional_trimmed(key)),
```
(Use the same optional-env helper the surrounding code uses for `MGS_MAP_PATH`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p massive_game_server_core federation_env 2>&1 | tail -3`
Expected: `test result: ok. 2 passed`

- [ ] **Step 5: Commit**

```bash
git add server/src/operational/config/env_registry.rs
git commit -m "feat(config): federation env (grid, region id, tile size, seed, wrap flag)"
```

## Task 5: Runtime world bounds accessor

**Files:**
- Create: `server/src/core/world_bounds.rs`
- Modify: `server/src/core/mod.rs` (add `pub mod world_bounds;`)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bounds_match_legacy_constants() {
        let b = world_bounds();
        assert_eq!(b.min_x, crate::core::constants::WORLD_MIN_X);
        assert_eq!(b.max_x, crate::core::constants::WORLD_MAX_X);
        assert_eq!(b.min_y, crate::core::constants::WORLD_MIN_Y);
        assert_eq!(b.max_y, crate::core::constants::WORLD_MAX_Y);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p massive_game_server_core world_bounds 2>&1 | tail -3`
Expected: compile error — unresolved module.

- [ ] **Step 3: Implement**

`server/src/core/world_bounds.rs`:

```rust
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

const DEFAULT_BOUNDS: WorldBounds = WorldBounds {
    min_x: crate::core::constants::WORLD_MIN_X,
    min_y: crate::core::constants::WORLD_MIN_Y,
    max_x: crate::core::constants::WORLD_MAX_X,
    max_y: crate::core::constants::WORLD_MAX_Y,
};

static WORLD_BOUNDS: OnceLock<WorldBounds> = OnceLock::new();

/// Called once at startup from the MasterMap + this server's tile.
/// Falls back to legacy constants when never initialized.
pub fn init_world_bounds(bounds: WorldBounds) {
    let _ = WORLD_BOUNDS.set(bounds);
}

pub fn world_bounds() -> WorldBounds {
    WORLD_BOUNDS.get().copied().unwrap_or(DEFAULT_BOUNDS)
}
```

Add `pub mod world_bounds;` to `server/src/core/mod.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p massive_game_server_core world_bounds 2>&1 | tail -3`
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Commit**

```bash
git add server/src/core/world_bounds.rs server/src/core/mod.rs
git commit -m "feat(core): runtime world bounds accessor with legacy fallback"
```

## Task 6: MasterMap assembly + Redis publish + `/api/master_map`

**Files:**
- Create: `server/src/world/master_map_store.rs`
- Create: `server/src/routes/master_map.rs`
- Modify: `server/src/world/mod.rs` (add `pub mod master_map_store;`)
- Modify: `server/src/routes/mod.rs` (export route builder)
- Modify: `server/src/main.rs` (assemble MasterMap from env, init world bounds, publish to Redis, register route)

- [ ] **Step 1: Write the failing tests**

`server/src/world/master_map_store.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::master_map::MasterMap;

    #[test]
    fn roundtrip_master_map_json() {
        let map = MasterMap::single_tile();
        let json = serde_json::to_string(&map).unwrap();
        let back: MasterMap = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cols, 1);
        assert_eq!(back.rows, 1);
    }

    #[test]
    fn from_grid_dims_builds_centered_map() {
        let map = master_map_from_config((2, 2), 1600.0, 1200.0, 42);
        assert_eq!(map.cols, 2);
        assert_eq!(map.world_width(), 3200.0);
        assert_eq!(map.map_seed, 42);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p massive_game_server_core master_map_store 2>&1 | tail -3`
Expected: compile error — module missing.

- [ ] **Step 3: Implement the store + route**

`server/src/world/master_map_store.rs`:

```rust
use crate::world::master_map::MasterMap;

pub const MASTER_MAP_REDIS_KEY: &str = "world:master_map";

pub fn master_map_from_config(
    grid: (u32, u32),
    tile_width: f32,
    tile_height: f32,
    map_seed: u64,
) -> MasterMap {
    MasterMap {
        version: 1,
        cols: grid.0,
        rows: grid.1,
        tile_width,
        tile_height,
        map_seed,
    }
}

/// Best-effort publish; silently skips when Redis is unavailable.
pub fn publish_master_map(redis_url: &str, map: &MasterMap) {
    let Ok(client) = redis::Client::open(redis_url.to_owned()) else { return };
    let Ok(mut conn) = client.get_connection() else { return };
    let Ok(json) = serde_json::to_string(map) else { return };
    let _: Result<(), redis::RedisError> =
        redis::cmd("SET").arg(MASTER_MAP_REDIS_KEY).arg(json).query(&mut conn);
}
```

`server/src/routes/master_map.rs`:

```rust
use std::sync::Arc;
use parking_lot::RwLock;
use warp::Filter;
use crate::world::master_map::MasterMap;

pub fn build_master_map_route(
    master_map: Arc<RwLock<MasterMap>>,
) -> warp::filters::BoxedFilter<(warp::reply::Response,)> {
    warp::path("api")
        .and(warp::path("master_map"))
        .and(warp::get())
        .map(move || {
            let map = master_map.read().clone();
            warp::reply::json(&map).into_response()
        })
        .boxed()
}
```

In `server/src/main.rs`, right after `load_app_env_config()` is available and before `compose_http_routes`:

```rust
let federation = &app_config.federation;
let master_map = std::sync::Arc::new(parking_lot::RwLock::new(
    world::master_map_store::master_map_from_config(
        federation.grid_dims(),
        federation.tile_width,
        federation.tile_height,
        federation.map_seed,
    ),
));
{
    let map = master_map.read();
    let (min_x, min_y, max_x, max_y) = map.tile_rect(world::master_map::TileCoord { x: 0, y: 0 });
    core::world_bounds::init_world_bounds(core::world_bounds::WorldBounds { min_x, min_y, max_x, max_y });
}
if let Some(redis_url) = redis_url_from_config(/* reuse the existing MGS_REDIS_URL lookup used for feature flags */) {
    world::master_map_store::publish_master_map(&redis_url, &master_map.read());
}
```

Then add `routes::master_map::build_master_map_route(master_map.clone())` into the `public_routes` `.or(...)` chain near `main.rs:235`, and `pub mod master_map;` in `server/src/routes/mod.rs`.

- [ ] **Step 4: Run tests and boot check**

```bash
cargo test -p massive_game_server_core master_map 2>&1 | tail -3
cargo build --release 2>&1 | tail -2
```
Expected: tests ok, build ok.

Restart the local server (kill wrapper + core, start `run-server-with-turn.js` as before), then:
```bash
curl -s http://localhost:8080/api/master_map
curl -s http://localhost:8080/healthz
```
Expected: JSON with `"cols":1,"rows":1`; healthz ok.

- [ ] **Step 5: Commit**

```bash
git add server/src/world/master_map_store.rs server/src/routes/master_map.rs \
  server/src/world/mod.rs server/src/routes/mod.rs server/src/main.rs
git commit -m "feat(federation): MasterMap assembly, Redis publish, /api/master_map route"
```

## Task 7: Match epoch publication

**Files:**
- Create: `server/src/server/epoch.rs`
- Modify: `server/src/server/mod.rs` (register module)
- Modify: `server/src/server/instance/game_modes.rs:600` (publish on match start)

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_computes_end_from_duration() {
        let epoch = MatchEpoch::new(7, 1_000_000, 237.0);
        assert_eq!(epoch.match_id, 7);
        assert_eq!(epoch.ends_at_ms, 1_000_000 + 237_000);
        assert!((epoch.time_remaining_secs(1_100_000) - 137.0).abs() < 0.01);
        assert_eq!(epoch.time_remaining_secs(2_000_000), 0.0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p massive_game_server_core epoch 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement**

`server/src/server/epoch.rs`:

```rust
use serde::{Deserialize, Serialize};

pub const EPOCH_REDIS_KEY: &str = "world:epoch";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchEpoch {
    pub match_id: u64,
    pub starts_at_ms: i64,
    pub ends_at_ms: i64,
}

impl MatchEpoch {
    pub fn new(match_id: u64, starts_at_ms: i64, duration_secs: f32) -> Self {
        Self {
            match_id,
            starts_at_ms,
            ends_at_ms: starts_at_ms + (duration_secs * 1000.0) as i64,
        }
    }

    pub fn time_remaining_secs(&self, now_ms: i64) -> f32 {
        ((self.ends_at_ms - now_ms).max(0)) as f32 / 1000.0
    }
}

/// First writer wins (SETNX); silently skips when Redis is unavailable.
pub fn publish_epoch(redis_url: &str, epoch: &MatchEpoch) {
    let Ok(client) = redis::Client::open(redis_url.to_owned()) else { return };
    let Ok(mut conn) = client.get_connection() else { return };
    let Ok(json) = serde_json::to_string(epoch) else { return };
    let _: Result<Option<()>, redis::RedisError> = redis::cmd("SET")
        .arg(EPOCH_REDIS_KEY)
        .arg(json)
        .arg("NX")
        .query(&mut conn);
}
```

At the match-start site in `game_modes.rs` (~line 597-600), inside the `fb::MatchStateType::Waiting` arm right after `match_info_guard.time_remaining = self.match_duration_secs;`, add:

```rust
let epoch = crate::server::epoch::MatchEpoch::new(
    self.frame_counter.load(AtomicOrdering::Relaxed),
    self.get_server_timestamp_ms() as i64,
    self.match_duration_secs,
);
if let Ok(url) = std::env::var("MGS_REDIS_URL") {
    crate::server::epoch::publish_epoch(&url, &epoch);
}
```
(`game_modes.rs` is `impl MassiveGameServer`; `self.frame_counter` and `self.get_server_timestamp_ms()` are already used elsewhere in the impl. `AtomicOrdering` is already imported in the file.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p massive_game_server_core epoch 2>&1 | tail -3` then `cargo build --release 2>&1 | tail -2`
Expected: `1 passed`, build ok.

- [ ] **Step 5: Commit**

```bash
git add server/src/server/epoch.rs server/src/server/mod.rs server/src/server/instance/game_modes.rs
git commit -m "feat(federation): publish match epoch to Redis (SETNX, first writer wins)"
```

## Task 8: Wrap-mode boundary flag (default off)

**Files:**
- Modify: `server/src/world/boundary.rs`

- [ ] **Step 1: Write the failing test**

Add to the tests in `boundary.rs`:

```rust
#[test]
fn wrap_position_wraps_toroidally() {
    let map = crate::world::master_map::MasterMap::single_tile();
    let (x, _y) = wrap_position_with_map(-900.0, 0.0, &map);
    assert!((x - 700.0).abs() < 1e-4);
}

#[test]
fn world_wrap_flag_defaults_off() {
    assert!(!world_wrap_enabled());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p massive_game_server_core boundary 2>&1 | tail -3`
Expected: compile error — functions missing.

- [ ] **Step 3: Implement**

`boundary.rs` currently exposes one method `clamp(&self, point: Vec2) -> Vec2` (line 21). Keep it as the legacy path and add two free helpers plus a runtime flag:

```rust
use std::sync::OnceLock;

static WORLD_WRAP_ENABLED: OnceLock<bool> = OnceLock::new();

/// Called once at startup from AppEnvConfig.federation.world_wrap
/// (mirror the configure_instance_runtime pattern in instance.rs:242).
pub fn configure_world_wrap(enabled: bool) {
    let _ = WORLD_WRAP_ENABLED.set(enabled);
}

pub fn world_wrap_enabled() -> bool {
    WORLD_WRAP_ENABLED.get().copied().unwrap_or(false)
}

pub fn wrap_position_with_map(
    x: f32,
    y: f32,
    map: &crate::world::master_map::MasterMap,
) -> (f32, f32) {
    map.wrap_position(x, y)
}

/// New boundary entry point: wraps on the torus when the flag is on,
/// otherwise behaves exactly like the legacy clamp.
pub fn bound_position(&self, point: Vec2, map: &crate::world::master_map::MasterMap) -> Vec2 {
    if world_wrap_enabled() {
        let (x, y) = wrap_position_with_map(point.x, point.y, map);
        Vec2::new(x, y)
    } else {
        self.clamp(point)
    }
}
```

Call sites of `.clamp(` for player/projectile positions switch to `.bound_position(pos, &master_map_snapshot)` only where a `MasterMap` handle is threaded through in later steps; for this task, wiring `configure_world_wrap(app_config.federation.world_wrap)` in `main.rs` next to the world-bounds init is sufficient — behavior is identical with the flag off.

Update the Task 8 test names accordingly:

```rust
#[test]
fn clamp_position_legacy_when_wrap_disabled() {
    // With wrap off, bound_position behaves exactly like the legacy clamp.
    // (Construct the boundary zone the same way existing boundary.rs tests do;
    // with flag unset, bound_position == clamp.)
}

#[test]
fn wrap_position_wraps_toroidally() {
    let map = crate::world::master_map::MasterMap::single_tile();
    let (x, _y) = wrap_position_with_map(-900.0, 0.0, &map);
    assert!((x - 700.0).abs() < 1e-4);
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p massive_game_server_core boundary 2>&1 | tail -3`
Expected: ok, both modes.

- [ ] **Step 5: Commit**

```bash
git add server/src/world/boundary.rs
git commit -m "feat(world): toroidal wrap mode behind MGS_WORLD_WRAP (default off)"
```

## Task 9: Release rebuild + e2e regression on production server

**Files:** none (verification only)

- [ ] **Step 1: Full test suite**

```bash
cd /home/habitat/massive-game-server-deployment/massive_game_server
cargo test --release -p massive_game_server_core 2>&1 | grep -E "test result|error" | tail -5
```
Expected: all ok, 0 failed.

- [ ] **Step 2: Rebuild and restart production**

```bash
cargo build --release 2>&1 | tail -2
pgrep -f "[r]un-server-with-turn" | xargs -r kill
pgrep -f "[m]assive_game_server_core" | xargs -r kill
sleep 3
cd /home/habitat/massive-game-server-deployment && (nohup node run-server-with-turn.js > logs/server.log 2>&1 &)
sleep 12
curl -s http://localhost:8080/healthz && curl -s http://localhost:8080/api/master_map
```
Expected: healthz ok; master_map JSON `cols:1, rows:1`.

- [ ] **Step 3: Run e2e regression subset against the live server**

```bash
cd /home/habitat/massive-game-server-deployment/massive_game_server/scripts/e2e
E2E_SERVER_SKIP=1 E2E_BASE_URL=http://localhost:8080 npx playwright test tests/connect.spec.js tests/wall_packet_loss_heal.spec.js tests/player_wall_collision.spec.js 2>&1 | tail -8
```
Expected: all pass (behavior unchanged on 1×1).

- [ ] **Step 4: Commit and push**

```bash
git push origin main
```

## Self-review notes

- Spec coverage: cleanup (§7) → Tasks 1–2; Master Map (§3) → Tasks 3–4, 6; runtime bounds (§4.1) → Task 5; epoch (§4.7) → Task 7; wrap flag (§3 coordinates) → Task 8; verification → Task 9. Neighbor mesh, ghosts, handoff, combat, consolidation, multiverse, dashboards are later steps/plans by design.
- Behavior safety: every new code path defaults to today's behavior (1×1, wrap off, Redis optional).
