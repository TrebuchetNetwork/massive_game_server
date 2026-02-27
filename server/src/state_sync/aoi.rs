// massive_game_server/server/src/state_sync/aoi.rs

use crate::core::constants::{AOI_EXIT_RADIUS, AOI_RADIUS};
use crate::core::math::squared_distance_2d;
use crate::core::types::EntityId;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy)]
pub struct AoiConfig {
    pub enter_radius: f32,
    pub exit_radius: f32,
    pub max_visible_entities: usize,
}

impl Default for AoiConfig {
    fn default() -> Self {
        Self {
            enter_radius: AOI_RADIUS,
            exit_radius: AOI_EXIT_RADIUS,
            max_visible_entities: 256,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AoiMembership {
    visible: HashSet<EntityId>,
}

impl AoiMembership {
    pub fn visible(&self) -> &HashSet<EntityId> {
        &self.visible
    }

    pub fn recompute(
        &mut self,
        observer_x: f32,
        observer_y: f32,
        entities: &[(EntityId, f32, f32)],
        config: AoiConfig,
    ) {
        let enter_sq = config.enter_radius * config.enter_radius;
        let exit_sq = config.exit_radius * config.exit_radius;

        let mut candidates = Vec::with_capacity(entities.len());

        for &(id, x, y) in entities {
            let dist_sq = squared_distance_2d(observer_x, observer_y, x, y);
            let already_visible = self.visible.contains(&id);
            let should_be_visible = if already_visible {
                dist_sq <= exit_sq
            } else {
                dist_sq <= enter_sq
            };
            if should_be_visible {
                candidates.push((id, dist_sq));
            }
        }

        candidates.sort_by(|left, right| left.1.total_cmp(&right.1));

        let mut retained = HashSet::with_capacity(config.max_visible_entities);
        for (id, _) in candidates.into_iter().take(config.max_visible_entities) {
            retained.insert(id);
        }

        self.visible = retained;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hysteresis_keeps_nearby_entity_visible() {
        let config = AoiConfig {
            enter_radius: 10.0,
            exit_radius: 12.0,
            max_visible_entities: 8,
        };
        let mut membership = AoiMembership::default();
        membership.recompute(0.0, 0.0, &[(1, 9.0, 0.0)], config);
        assert!(membership.visible().contains(&1));

        membership.recompute(0.0, 0.0, &[(1, 11.0, 0.0)], config);
        assert!(membership.visible().contains(&1));
    }

    #[test]
    fn prioritizes_nearest_entities_when_capped() {
        let config = AoiConfig {
            enter_radius: 100.0,
            exit_radius: 100.0,
            max_visible_entities: 2,
        };
        let mut membership = AoiMembership::default();
        membership.recompute(
            0.0,
            0.0,
            &[(10, 90.0, 0.0), (20, 5.0, 0.0), (30, 15.0, 0.0)],
            config,
        );
        assert!(membership.visible().contains(&20));
        assert!(membership.visible().contains(&30));
        assert!(!membership.visible().contains(&10));
    }
}
