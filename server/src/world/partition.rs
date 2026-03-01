// massive_game_server/server/src/world/partition.rs
use crate::core::constants::BOUNDARY_ZONE_WIDTH; // Removed unused constants
use crate::core::types::{
    BoundaryAction, BoundarySnapshot, BoundaryUpdate, Direction, EntityId, PartitionBounds, Pickup,
    PlayerID, Vec2, Wall,
};
use arc_swap::ArcSwap;
use crossbeam_queue::ArrayQueue;
use dashmap::{DashMap, DashSet};
// Removed unused: use parking_lot::RwLock;
// Removed unused: use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::debug; // Removed unused warn, error

#[derive(Debug)]
pub struct LockFreeBoundaryZone {
    width: f32,
    channels: [Arc<ArrayQueue<BoundaryUpdate>>; 4],
    snapshots: [Arc<ArcSwap<BoundarySnapshot>>; 4],
}

impl LockFreeBoundaryZone {
    pub fn new(capacity_per_channel: usize, boundary_width: f32) -> Self {
        Self {
            width: boundary_width,
            channels: [
                Arc::new(ArrayQueue::new(capacity_per_channel)),
                Arc::new(ArrayQueue::new(capacity_per_channel)),
                Arc::new(ArrayQueue::new(capacity_per_channel)),
                Arc::new(ArrayQueue::new(capacity_per_channel)),
            ],
            snapshots: [
                Arc::new(ArcSwap::from_pointee(BoundarySnapshot::default())),
                Arc::new(ArcSwap::from_pointee(BoundarySnapshot::default())),
                Arc::new(ArcSwap::from_pointee(BoundarySnapshot::default())),
                Arc::new(ArcSwap::from_pointee(BoundarySnapshot::default())),
            ],
        }
    }

    pub fn update_player_boundary_status(
        &self,
        player_id: PlayerID,
        x: f32,
        y: f32,
        partition_bounds: &PartitionBounds,
        action: BoundaryAction,
    ) {
        let update = BoundaryUpdate {
            player_id,
            action,
            position: (x, y),
        };
        if y - partition_bounds.min_y < self.width {
            if let Some(idx) = Direction::North.cardinal_channel_index() {
                self.enqueue_boundary_update(idx, &update);
            }
        }
        if partition_bounds.max_x - x < self.width {
            if let Some(idx) = Direction::East.cardinal_channel_index() {
                self.enqueue_boundary_update(idx, &update);
            }
        }
        if partition_bounds.max_y - y < self.width {
            if let Some(idx) = Direction::South.cardinal_channel_index() {
                self.enqueue_boundary_update(idx, &update);
            }
        }
        if x - partition_bounds.min_x < self.width {
            if let Some(idx) = Direction::West.cardinal_channel_index() {
                self.enqueue_boundary_update(idx, &update);
            }
        }
    }

    #[inline]
    fn enqueue_boundary_update(&self, channel_idx: usize, update: &BoundaryUpdate) {
        if self.channels[channel_idx].push(update.clone()).is_ok() {
            return;
        }
        // Keep latest visibility changes under pressure by evicting the oldest
        // queued update instead of silently dropping the new one.
        let _ = self.channels[channel_idx].pop();
        if self.channels[channel_idx].push(update.clone()).is_err() {
            debug!(
                channel = channel_idx,
                "Boundary update queue remained saturated after eviction; dropped latest update."
            );
        }
    }

    pub fn update_snapshots(&self) {
        for dir_idx in 0..4 {
            if self.channels[dir_idx].is_empty() {
                continue;
            }
            let direction = Direction::from_index(dir_idx).unwrap_or(Direction::North);
            self.update_direction_snapshot(direction);
        }
    }

    fn update_direction_snapshot(&self, direction: Direction) {
        let Some(channel_idx) = direction.cardinal_channel_index() else {
            return;
        };
        let channel = &self.channels[channel_idx];
        if channel.is_empty() {
            return;
        }
        let snapshot_cell = &self.snapshots[channel_idx];

        let mut current_players_map: HashMap<PlayerID, (f32, f32)> = {
            let current_snapshot = snapshot_cell.load_full();
            current_snapshot
                .players
                .iter()
                .map(|(id, x, y)| (id.clone(), (*x, *y)))
                .collect()
        };

        while let Some(update) = channel.pop() {
            match update.action {
                BoundaryAction::Enter | BoundaryAction::Update => {
                    current_players_map.insert(update.player_id, update.position);
                }
                BoundaryAction::Leave => {
                    current_players_map.remove(&update.player_id);
                }
            }
        }

        let new_snapshot_data: Vec<(PlayerID, f32, f32)> = current_players_map
            .into_iter()
            .map(|(id, (x, y))| (id, x, y))
            .collect();

        let old_snapshot_version = snapshot_cell.load().version;

        let new_snapshot = BoundarySnapshot {
            players: new_snapshot_data,
            version: old_snapshot_version + 1,
            timestamp: Instant::now(),
        };
        snapshot_cell.store(Arc::new(new_snapshot));
    }

    #[inline]
    pub fn has_pending_updates(&self) -> bool {
        self.channels.iter().any(|channel| !channel.is_empty())
    }

    pub fn get_snapshot(&self, direction: Direction) -> Option<Arc<BoundarySnapshot>> {
        let channel_idx = direction.cardinal_channel_index()?;
        Some(self.snapshots[channel_idx].load_full())
    }
}

impl Direction {
    fn cardinal_channel_index(self) -> Option<usize> {
        match self {
            Direction::North => Some(0),
            Direction::East => Some(1),
            Direction::South => Some(2),
            Direction::West => Some(3),
            _ => None,
        }
    }

    fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Direction::North),
            1 => Some(Direction::East),
            2 => Some(Direction::South),
            3 => Some(Direction::West),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct ImprovedWorldPartition {
    pub id: usize,
    pub bounds: PartitionBounds,
    pub local_players: Arc<DashSet<PlayerID>>,
    pub all_walls_in_partition: Arc<DashMap<EntityId, Wall>>,
    pub dynamic_objects: Arc<DashMap<EntityId, Pickup>>,
    pub boundary_zone: Arc<LockFreeBoundaryZone>,
    pub neighbor_ids: [Option<usize>; 8],
}

impl ImprovedWorldPartition {
    pub fn new(
        id: usize,
        bounds: PartitionBounds,
        neighbor_ids: [Option<usize>; 8],
        boundary_config_capacity: usize,
    ) -> Self {
        ImprovedWorldPartition {
            id,
            bounds,
            local_players: Arc::new(DashSet::new()),
            all_walls_in_partition: Arc::new(DashMap::new()),
            dynamic_objects: Arc::new(DashMap::new()),
            boundary_zone: Arc::new(LockFreeBoundaryZone::new(
                boundary_config_capacity,
                BOUNDARY_ZONE_WIDTH,
            )),
            neighbor_ids,
        }
    }

    pub fn add_wall_on_load(&self, wall: Wall) {
        self.all_walls_in_partition.insert(wall.id, wall);
    }

    pub fn upsert_wall(&self, wall: Wall) {
        self.all_walls_in_partition.insert(wall.id, wall);
    }

    pub fn remove_wall(&self, wall_id: EntityId) -> Option<Wall> {
        self.all_walls_in_partition
            .remove(&wall_id)
            .map(|(_, wall)| wall)
    }

    pub fn get_wall(&self, wall_id: EntityId) -> Option<Wall> {
        self.all_walls_in_partition
            .get(&wall_id)
            .map(|entry| entry.value().clone())
    }

    pub fn get_all_walls_snapshot(&self) -> Vec<Wall> {
        self.all_walls_in_partition
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn damage_destructible_wall(&self, wall_id: EntityId, damage: i32) -> Option<(bool, Vec2)> {
        if let Some(mut wall_entry) = self.all_walls_in_partition.get_mut(&wall_id) {
            let wall = wall_entry.value_mut();
            if wall.is_destructible && wall.current_health > 0 {
                let old_health = wall.current_health;
                wall.current_health = (wall.current_health - damage).max(0);
                debug!(
                    "[Partition {}] Wall {} damaged. Health: {} -> {}",
                    self.id, wall_id, old_health, wall.current_health
                );
                if wall.current_health == 0 && old_health > 0 {
                    return Some((
                        true,
                        Vec2::new(wall.x + wall.width / 2.0, wall.y + wall.height / 2.0),
                    ));
                }
                return Some((
                    false,
                    Vec2::new(wall.x + wall.width / 2.0, wall.y + wall.height / 2.0),
                ));
            }
        }
        None
    }

    pub fn respawn_destructible_wall(&self, wall_id: EntityId) -> bool {
        if let Some(mut wall_entry) = self.all_walls_in_partition.get_mut(&wall_id) {
            let wall = wall_entry.value_mut();
            if wall.is_destructible {
                wall.current_health = wall.max_health;
                debug!(
                    "[Partition {}] Wall {} respawned. Health: {}/{}",
                    self.id, wall_id, wall.current_health, wall.max_health
                );
                return true;
            }
        }
        false
    }

    pub fn contains_point_primary(&self, x: f32, y: f32) -> bool {
        x >= self.bounds.min_x
            && x < self.bounds.max_x
            && y >= self.bounds.min_y
            && y < self.bounds.max_y
    }

    pub fn update_player_status(
        &self,
        player_id: &PlayerID,
        x: f32,
        y: f32,
        is_newly_entered: bool,
    ) {
        let action = if is_newly_entered {
            BoundaryAction::Enter
        } else {
            BoundaryAction::Update
        };
        let is_near_north = y - self.bounds.min_y < self.boundary_zone.width;
        let is_near_east = self.bounds.max_x - x < self.boundary_zone.width;
        let is_near_south = self.bounds.max_y - y < self.boundary_zone.width;
        let is_near_west = x - self.bounds.min_x < self.boundary_zone.width;

        if is_near_north || is_near_east || is_near_south || is_near_west {
            self.boundary_zone.update_player_boundary_status(
                player_id.clone(),
                x,
                y,
                &self.bounds,
                action,
            );
        }
        if !self.contains_point_primary(x, y) && !is_newly_entered {
            self.local_players.remove(player_id);
            self.boundary_zone.update_player_boundary_status(
                player_id.clone(),
                x,
                y,
                &self.bounds,
                BoundaryAction::Leave,
            );
        } else if is_newly_entered {
            self.local_players.insert(player_id.clone());
        }
    }

    pub fn add_dynamic_object(&self, pickup: Pickup) {
        self.dynamic_objects.insert(pickup.id, pickup);
    }

    pub fn remove_dynamic_object(&self, pickup_id: &EntityId) -> Option<Pickup> {
        self.dynamic_objects.remove(pickup_id).map(|(_k, v)| v)
    }
}

pub struct WorldPartitionManager {
    partitions: Vec<Arc<ImprovedWorldPartition>>,
    grid_dim: usize,
    partition_width: f32,
    partition_height: f32,
    world_min_x: f32,
    world_min_y: f32,
}

impl WorldPartitionManager {
    pub fn new(
        grid_dim: usize,
        world_width: f32,
        world_height: f32,
        world_min_x: f32,
        world_min_y: f32,
        boundary_config_capacity_per_channel: usize,
    ) -> Self {
        assert!(grid_dim > 0, "world_partition_grid_dim must be > 0");
        assert!(
            world_width.is_finite() && world_width > 0.0,
            "world_width must be finite and > 0"
        );
        assert!(
            world_height.is_finite() && world_height > 0.0,
            "world_height must be finite and > 0"
        );
        let partition_width = world_width / grid_dim as f32;
        let partition_height = world_height / grid_dim as f32;
        let mut partitions = Vec::with_capacity(grid_dim * grid_dim);

        for y_idx in 0..grid_dim {
            for x_idx in 0..grid_dim {
                let id = y_idx * grid_dim + x_idx;
                let bounds = PartitionBounds {
                    min_x: world_min_x + x_idx as f32 * partition_width,
                    max_x: world_min_x + (x_idx + 1) as f32 * partition_width,
                    min_y: world_min_y + y_idx as f32 * partition_height,
                    max_y: world_min_y + (y_idx + 1) as f32 * partition_height,
                };
                let mut neighbor_ids: [Option<usize>; 8] = [None; 8];
                if y_idx > 0 {
                    neighbor_ids[0] = Some((y_idx - 1) * grid_dim + x_idx);
                }
                if y_idx > 0 && x_idx < grid_dim - 1 {
                    neighbor_ids[1] = Some((y_idx - 1) * grid_dim + (x_idx + 1));
                }
                if x_idx < grid_dim - 1 {
                    neighbor_ids[2] = Some(y_idx * grid_dim + (x_idx + 1));
                }
                if y_idx < grid_dim - 1 && x_idx < grid_dim - 1 {
                    neighbor_ids[3] = Some((y_idx + 1) * grid_dim + (x_idx + 1));
                }
                if y_idx < grid_dim - 1 {
                    neighbor_ids[4] = Some((y_idx + 1) * grid_dim + x_idx);
                }
                if y_idx < grid_dim - 1 && x_idx > 0 {
                    neighbor_ids[5] = Some((y_idx + 1) * grid_dim + (x_idx - 1));
                }
                if x_idx > 0 {
                    neighbor_ids[6] = Some(y_idx * grid_dim + (x_idx - 1));
                }
                if y_idx > 0 && x_idx > 0 {
                    neighbor_ids[7] = Some((y_idx - 1) * grid_dim + (x_idx - 1));
                }
                partitions.push(Arc::new(ImprovedWorldPartition::new(
                    id,
                    bounds,
                    neighbor_ids,
                    boundary_config_capacity_per_channel,
                )));
            }
        }
        WorldPartitionManager {
            partitions,
            grid_dim,
            partition_width,
            partition_height,
            world_min_x,
            world_min_y,
        }
    }

    #[inline]
    pub fn get_partition_index_for_point(&self, x: f32, y: f32) -> usize {
        let grid_x = ((x - self.world_min_x) / self.partition_width).floor() as usize;
        let grid_y = ((y - self.world_min_y) / self.partition_height).floor() as usize;
        let clamped_x = grid_x.min(self.grid_dim.saturating_sub(1));
        let clamped_y = grid_y.min(self.grid_dim.saturating_sub(1));
        clamped_y * self.grid_dim + clamped_x
    }

    pub fn get_partition(&self, index: usize) -> Option<Arc<ImprovedWorldPartition>> {
        self.partitions.get(index).cloned()
    }

    pub fn get_partitions_for_processing(&self) -> Vec<Arc<ImprovedWorldPartition>> {
        self.partitions.clone()
    }

    pub fn collect_partition_indices_for_aoi(
        &self,
        x: f32,
        y: f32,
        radius: f32,
        out: &mut Vec<usize>,
    ) {
        self.collect_partition_indices_for_bounds(
            x - radius,
            x + radius,
            y - radius,
            y + radius,
            out,
        );
    }

    pub fn collect_partition_indices_for_bounds(
        &self,
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
        out: &mut Vec<usize>,
    ) {
        let max_index = self.grid_dim.saturating_sub(1) as isize;
        let grid_min_x = (((min_x - self.world_min_x) / self.partition_width).floor() as isize)
            .clamp(0, max_index);
        let grid_max_x = (((max_x - self.world_min_x) / self.partition_width).floor() as isize)
            .clamp(0, max_index);
        let grid_min_y = (((min_y - self.world_min_y) / self.partition_height).floor() as isize)
            .clamp(0, max_index);
        let grid_max_y = (((max_y - self.world_min_y) / self.partition_height).floor() as isize)
            .clamp(0, max_index);

        out.clear();
        out.reserve(((grid_max_x - grid_min_x + 1) * (grid_max_y - grid_min_y + 1)) as usize);

        for grid_y in grid_min_y..=grid_max_y {
            for grid_x in grid_min_x..=grid_max_x {
                out.push((grid_y as usize) * self.grid_dim + grid_x as usize);
            }
        }
    }

    pub fn update_all_boundary_snapshots(&self) {
        for partition_arc in &self.partitions {
            if partition_arc.boundary_zone.has_pending_updates() {
                partition_arc.boundary_zone.update_snapshots();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WorldPartitionManager;

    #[test]
    #[should_panic(expected = "world_partition_grid_dim must be > 0")]
    fn partition_manager_rejects_zero_grid_dim() {
        let _ = WorldPartitionManager::new(0, 100.0, 100.0, 0.0, 0.0, 32);
    }
}
