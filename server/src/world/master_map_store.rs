// massive_game_server/server/src/world/master_map_store.rs

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

/// Best-effort publish; logs a warning and skips when Redis is unavailable.
pub fn publish_master_map(redis_url: &str, map: &MasterMap) {
    let publish = || -> Result<(), String> {
        let client = redis::Client::open(redis_url.to_owned()).map_err(|e| e.to_string())?;
        let mut conn = client
            .get_connection_with_timeout(std::time::Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let json = serde_json::to_string(map).map_err(|e| e.to_string())?;
        redis::cmd("SET")
            .arg(MASTER_MAP_REDIS_KEY)
            .arg(json)
            .query::<()>(&mut conn)
            .map_err(|e| e.to_string())
    };
    if let Err(err) = publish() {
        tracing::warn!("master map Redis publish skipped: {err}");
    }
}

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
