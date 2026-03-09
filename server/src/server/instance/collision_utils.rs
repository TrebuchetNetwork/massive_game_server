use super::*;

impl MassiveGameServer {
    pub(super) fn has_clear_line_of_sight(
        &self,
        from_x: f32,
        from_y: f32,
        to_x: f32,
        to_y: f32,
    ) -> bool {
        !self
            .wall_spatial_index
            .any_line_segment_candidate(from_x, from_y, to_x, to_y, |wall| {
                segment_first_hit_fraction_with_aabb(
                    from_x,
                    from_y,
                    to_x,
                    to_y,
                    wall.x,
                    wall.x + wall.width,
                    wall.y,
                    wall.y + wall.height,
                )
                .is_some()
            })
    }

    pub(super) fn position_overlaps_any_wall(&self, x: f32, y: f32) -> bool {
        self.wall_spatial_index
            .any_radius(x, y, PLAYER_RADIUS + 8.0, |wall| {
                let closest_x = x.clamp(wall.x, wall.x + wall.width);
                let closest_y = y.clamp(wall.y, wall.y + wall.height);
                let dx = x - closest_x;
                let dy = y - closest_y;
                dx * dx + dy * dy < PLAYER_RADIUS * PLAYER_RADIUS
            })
    }

    pub fn collect_all_walls_current_state(&self) -> Vec<Wall> {
        let mut all_walls = Vec::new();
        for partition_arc in self.world_partition_manager.get_partitions_for_processing() {
            partition_arc
                .all_walls_in_partition
                .iter()
                .for_each(|wall_entry| {
                    let wall = wall_entry.value();
                    // Send ALL walls including destroyed ones - client needs to render them as rubble/obstacles
                    all_walls.push(wall.clone());
                });
        }
        all_walls
    }
}
