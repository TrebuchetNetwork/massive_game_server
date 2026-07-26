// massive_game_server/server/src/world/master_map.rs

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
