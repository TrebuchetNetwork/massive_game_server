// massive_game_server/server/src/core/world_bounds.rs
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
