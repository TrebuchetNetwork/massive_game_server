// massive_game_server/server/src/concurrent/spatial_index.rs

use crate::core::simd;
use crate::core::types::{EntityId, PlayerID};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::debug;

#[derive(Debug, Clone)]
struct SpatialCell {
    player_ids: HashSet<PlayerID>,
    projectile_ids: HashSet<EntityId>,
}

impl SpatialCell {
    fn new() -> Self {
        SpatialCell {
            player_ids: HashSet::new(),
            projectile_ids: HashSet::new(),
        }
    }
}

pub struct ImprovedSpatialIndex {
    cells: Vec<RwLock<SpatialCell>>,
    grid_width: usize,
    grid_height: usize,
    cell_size: f32,
    world_min_x: f32,
    world_min_y: f32,

    // Position tracking for fast lookups
    player_positions: Arc<DashMap<PlayerID, (f32, f32)>>,
    projectile_positions: Arc<DashMap<EntityId, (f32, f32)>>,

    // Cell index tracking for efficient updates
    player_cells: Arc<DashMap<PlayerID, usize>>,
    projectile_cells: Arc<DashMap<EntityId, usize>>,
}

impl ImprovedSpatialIndex {
    pub fn new(
        world_width: f32,
        world_height: f32,
        world_min_x: f32,
        world_min_y: f32,
        cell_size: f32,
    ) -> Self {
        let grid_width = ((world_width / cell_size).ceil() as usize).max(1);
        let grid_height = ((world_height / cell_size).ceil() as usize).max(1);
        let total_cells = grid_width * grid_height;

        let mut cells = Vec::with_capacity(total_cells);
        for _ in 0..total_cells {
            cells.push(RwLock::new(SpatialCell::new()));
        }

        debug!(
            "Spatial index initialized: {}x{} grid, {} total cells, cell size: {}",
            grid_width, grid_height, total_cells, cell_size
        );

        ImprovedSpatialIndex {
            cells,
            grid_width,
            grid_height,
            cell_size,
            world_min_x,
            world_min_y,
            player_positions: Arc::new(DashMap::new()),
            projectile_positions: Arc::new(DashMap::new()),
            player_cells: Arc::new(DashMap::new()),
            projectile_cells: Arc::new(DashMap::new()),
        }
    }

    #[inline]
    fn get_cell_index(&self, x: f32, y: f32) -> usize {
        let grid_x = ((x - self.world_min_x) / self.cell_size).floor().max(0.0) as usize;
        let grid_y = ((y - self.world_min_y) / self.cell_size).floor().max(0.0) as usize;

        let clamped_x = grid_x.min(self.grid_width.saturating_sub(1));
        let clamped_y = grid_y.min(self.grid_height.saturating_sub(1));

        clamped_y * self.grid_width + clamped_x
    }

    #[inline]
    fn get_cells_in_radius(&self, center_x: f32, center_y: f32, radius: f32) -> Vec<usize> {
        let min_x = center_x - radius;
        let max_x = center_x + radius;
        let min_y = center_y - radius;
        let max_y = center_y + radius;

        let min_grid_x = ((min_x - self.world_min_x) / self.cell_size)
            .floor()
            .max(0.0) as usize;
        let max_grid_x = ((max_x - self.world_min_x) / self.cell_size)
            .ceil()
            .min(self.grid_width as f32) as usize;
        let min_grid_y = ((min_y - self.world_min_y) / self.cell_size)
            .floor()
            .max(0.0) as usize;
        let max_grid_y = ((max_y - self.world_min_y) / self.cell_size)
            .ceil()
            .min(self.grid_height as f32) as usize;

        let mut cell_indices = Vec::new();
        for y in min_grid_y..max_grid_y {
            for x in min_grid_x..max_grid_x {
                if x < self.grid_width && y < self.grid_height {
                    cell_indices.push(y * self.grid_width + x);
                }
            }
        }

        cell_indices
    }

    // Player methods
    pub fn update_player_position(&self, player_id: PlayerID, x: f32, y: f32) {
        let new_cell_idx = self.get_cell_index(x, y);

        // Check if player moved to a different cell
        let old_cell_idx = self
            .player_cells
            .get(&player_id)
            .map(|entry| *entry.value());

        if let Some(old_idx) = old_cell_idx {
            if old_idx != new_cell_idx {
                // Remove from old cell
                if let Some(old_cell) = self.cells.get(old_idx) {
                    old_cell.write().player_ids.remove(&player_id);
                }

                // Add to new cell
                if let Some(new_cell) = self.cells.get(new_cell_idx) {
                    new_cell.write().player_ids.insert(player_id.clone());
                }

                self.player_cells.insert(player_id.clone(), new_cell_idx);
            }
        } else {
            // First time tracking this player
            if let Some(new_cell) = self.cells.get(new_cell_idx) {
                new_cell.write().player_ids.insert(player_id.clone());
            }
            self.player_cells.insert(player_id.clone(), new_cell_idx);
        }

        // Always update position
        self.player_positions.insert(player_id, (x, y));
    }

    pub fn remove_player(&self, player_id: &PlayerID) {
        if let Some((_, cell_idx)) = self.player_cells.remove(player_id) {
            if let Some(cell) = self.cells.get(cell_idx) {
                cell.write().player_ids.remove(player_id);
            }
        }
        self.player_positions.remove(player_id);
    }

    pub fn query_nearby_players(&self, x: f32, y: f32, radius: f32) -> Vec<PlayerID> {
        let radius_squared = radius * radius;
        let cell_indices = self.get_cells_in_radius(x, y, radius);
        let mut candidate_ids = Vec::new();
        let mut candidate_xs = Vec::new();
        let mut candidate_ys = Vec::new();
        let mut checked_players = HashSet::new();

        for cell_idx in cell_indices {
            if let Some(cell) = self.cells.get(cell_idx) {
                let cell_guard = cell.read();
                for player_id in &cell_guard.player_ids {
                    if checked_players.insert(player_id.clone()) {
                        if let Some(pos_entry) = self.player_positions.get(player_id) {
                            let (px, py) = *pos_entry.value();
                            candidate_ids.push(player_id.clone());
                            candidate_xs.push(px);
                            candidate_ys.push(py);
                        }
                    }
                }
            }
        }

        let mut matched_indices = Vec::with_capacity(candidate_ids.len());
        simd::filter_indices_within_radius(
            &candidate_xs,
            &candidate_ys,
            x,
            y,
            radius_squared,
            &mut matched_indices,
        );

        let mut nearby_players = Vec::with_capacity(matched_indices.len());
        for idx in matched_indices {
            if let Some(player_id) = candidate_ids.get(idx) {
                nearby_players.push(player_id.clone());
            }
        }
        nearby_players
    }

    /// Returns nearby players together with their cached positions from the spatial index.
    /// This avoids a second map lookup in hot collision paths.
    pub fn query_nearby_players_with_positions(
        &self,
        x: f32,
        y: f32,
        radius: f32,
    ) -> Vec<(PlayerID, f32, f32)> {
        let radius_squared = radius * radius;
        let cell_indices = self.get_cells_in_radius(x, y, radius);
        let mut candidate_ids = Vec::new();
        let mut candidate_xs = Vec::new();
        let mut candidate_ys = Vec::new();
        let mut checked_players = HashSet::new();

        for cell_idx in cell_indices {
            if let Some(cell) = self.cells.get(cell_idx) {
                let cell_guard = cell.read();
                for player_id in &cell_guard.player_ids {
                    if checked_players.insert(player_id.clone()) {
                        if let Some(pos_entry) = self.player_positions.get(player_id) {
                            let (px, py) = *pos_entry.value();
                            candidate_ids.push(player_id.clone());
                            candidate_xs.push(px);
                            candidate_ys.push(py);
                        }
                    }
                }
            }
        }

        let mut matched_indices = Vec::with_capacity(candidate_ids.len());
        simd::filter_indices_within_radius(
            &candidate_xs,
            &candidate_ys,
            x,
            y,
            radius_squared,
            &mut matched_indices,
        );

        let mut nearby_players = Vec::with_capacity(matched_indices.len());
        for idx in matched_indices {
            if let (Some(player_id), Some(px), Some(py)) = (
                candidate_ids.get(idx),
                candidate_xs.get(idx),
                candidate_ys.get(idx),
            ) {
                nearby_players.push((player_id.clone(), *px, *py));
            }
        }
        nearby_players
    }

    // Projectile methods
    pub fn update_projectile_position(&self, proj_id: EntityId, x: f32, y: f32) {
        let new_cell_idx = self.get_cell_index(x, y);

        // Check if projectile moved to a different cell
        let old_cell_idx = self
            .projectile_cells
            .get(&proj_id)
            .map(|entry| *entry.value());

        if let Some(old_idx) = old_cell_idx {
            if old_idx != new_cell_idx {
                // Remove from old cell
                if let Some(old_cell) = self.cells.get(old_idx) {
                    old_cell.write().projectile_ids.remove(&proj_id);
                }

                // Add to new cell
                if let Some(new_cell) = self.cells.get(new_cell_idx) {
                    new_cell.write().projectile_ids.insert(proj_id);
                }

                self.projectile_cells.insert(proj_id, new_cell_idx);
            }
        } else {
            // First time tracking this projectile
            if let Some(new_cell) = self.cells.get(new_cell_idx) {
                new_cell.write().projectile_ids.insert(proj_id);
            }
            self.projectile_cells.insert(proj_id, new_cell_idx);
        }

        // Always update position
        self.projectile_positions.insert(proj_id, (x, y));
    }

    pub fn remove_projectile(&self, proj_id: &EntityId) {
        if let Some((_, cell_idx)) = self.projectile_cells.remove(proj_id) {
            if let Some(cell) = self.cells.get(cell_idx) {
                cell.write().projectile_ids.remove(proj_id);
            }
        }
        self.projectile_positions.remove(proj_id);
    }

    pub fn query_nearby_projectiles(&self, x: f32, y: f32, radius: f32) -> Vec<EntityId> {
        let radius_squared = radius * radius;
        let cell_indices = self.get_cells_in_radius(x, y, radius);
        let mut candidate_ids = Vec::new();
        let mut candidate_xs = Vec::new();
        let mut candidate_ys = Vec::new();
        let mut checked_projectiles = HashSet::new();

        for cell_idx in cell_indices {
            if let Some(cell) = self.cells.get(cell_idx) {
                let cell_guard = cell.read();
                for proj_id in &cell_guard.projectile_ids {
                    if checked_projectiles.insert(*proj_id) {
                        if let Some(pos_entry) = self.projectile_positions.get(proj_id) {
                            let (px, py) = *pos_entry.value();
                            candidate_ids.push(*proj_id);
                            candidate_xs.push(px);
                            candidate_ys.push(py);
                        }
                    }
                }
            }
        }

        let mut matched_indices = Vec::with_capacity(candidate_ids.len());
        simd::filter_indices_within_radius(
            &candidate_xs,
            &candidate_ys,
            x,
            y,
            radius_squared,
            &mut matched_indices,
        );

        let mut nearby_projectiles = Vec::with_capacity(matched_indices.len());
        for idx in matched_indices {
            if let Some(projectile_id) = candidate_ids.get(idx) {
                nearby_projectiles.push(*projectile_id);
            }
        }
        nearby_projectiles
    }

    // Batch operations for efficiency
    pub fn batch_update_projectiles(&self, updates: &[(EntityId, f32, f32)]) {
        for &(proj_id, x, y) in updates {
            self.update_projectile_position(proj_id, x, y);
        }
    }

    pub fn get_stats(&self) -> SpatialIndexStats {
        let total_players = self.player_positions.len();
        let total_projectiles = self.projectile_positions.len();
        let mut occupied_cells = 0;
        let mut max_entities_per_cell = 0;

        for cell in &self.cells {
            let cell_guard = cell.read();
            let entity_count = cell_guard.player_ids.len() + cell_guard.projectile_ids.len();
            if entity_count > 0 {
                occupied_cells += 1;
                max_entities_per_cell = max_entities_per_cell.max(entity_count);
            }
        }

        SpatialIndexStats {
            total_players,
            total_projectiles,
            occupied_cells,
            total_cells: self.cells.len(),
            max_entities_per_cell,
        }
    }
}

#[derive(Debug)]
pub struct SpatialIndexStats {
    pub total_players: usize,
    pub total_projectiles: usize,
    pub occupied_cells: usize,
    pub total_cells: usize,
    pub max_entities_per_cell: usize,
}

#[cfg(test)]
mod tests {
    use super::ImprovedSpatialIndex;
    use crate::core::types::PlayerID;
    use std::sync::Arc;

    fn pid(raw: &str) -> PlayerID {
        Arc::new(raw.to_string())
    }

    #[test]
    fn query_nearby_players_with_positions_returns_positions() {
        let index = ImprovedSpatialIndex::new(1024.0, 1024.0, 0.0, 0.0, 64.0);
        let p1 = pid("p1");
        let p2 = pid("p2");
        let p3 = pid("p3");

        index.update_player_position(p1.clone(), 10.0, 10.0);
        index.update_player_position(p2.clone(), 28.0, 24.0);
        index.update_player_position(p3, 420.0, 400.0);

        let mut nearby = index.query_nearby_players_with_positions(12.0, 12.0, 30.0);
        nearby.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

        assert_eq!(nearby.len(), 2);
        assert_eq!(nearby[0].0.as_str(), "p1");
        assert_eq!(nearby[0].1, 10.0);
        assert_eq!(nearby[0].2, 10.0);
        assert_eq!(nearby[1].0.as_str(), "p2");
        assert_eq!(nearby[1].1, 28.0);
        assert_eq!(nearby[1].2, 24.0);
    }
}
