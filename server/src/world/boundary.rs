// massive_game_server/server/src/world/boundary.rs

use std::sync::OnceLock;

use crate::core::types::Vec2;

static WORLD_WRAP_ENABLED: OnceLock<bool> = OnceLock::new();

/// Called once at startup from AppEnvConfig.federation.world_wrap
/// (mirrors the configure_instance_runtime pattern in instance.rs).
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

#[derive(Debug, Clone, Copy)]
pub struct WorldBoundary {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

impl WorldBoundary {
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.min_x
            && point.x <= self.max_x
            && point.y >= self.min_y
            && point.y <= self.max_y
    }

    pub fn clamp(&self, point: Vec2) -> Vec2 {
        Vec2::new(
            point.x.clamp(self.min_x, self.max_x),
            point.y.clamp(self.min_y, self.max_y),
        )
    }

    /// Boundary entry point: wraps on the torus when the flag is on,
    /// otherwise behaves exactly like the legacy clamp.
    pub fn bound_position(
        &self,
        point: Vec2,
        map: &crate::world::master_map::MasterMap,
    ) -> Vec2 {
        if world_wrap_enabled() {
            let (x, y) = wrap_position_with_map(point.x, point.y, map);
            Vec2::new(x, y)
        } else {
            self.clamp(point)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
