// massive_game_server/server/src/world/boundary.rs

use crate::core::types::Vec2;

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
}
