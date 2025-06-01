// massive_game_server/server/src/world/partition.rs
use crate::core::types::{PlayerID, PartitionBounds, BoundaryUpdate, BoundaryAction, BoundarySnapshot, Direction, Vec2, Wall, Pickup, GameEvent, EntityId};
use crate::core::constants::{BOUNDARY_ZONE_WIDTH}; // Removed unused constants
use crossbeam_epoch::{self as epoch, Guard, Shared, Atomic}; // Removed 'unprotected' as it's not directly needed with pinned guards
use crossbeam_queue::{ArrayQueue, SegQueue};
use dashmap::{DashMap, DashSet};
// Removed unused: use parking_lot::RwLock;
// Removed unused: use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering}; // Removed unused AtomicU64
use std::time::Instant;
use tracing::{debug}; // Removed unused warn, error


#[derive(Debug)]
pub struct LockFreeBoundaryZone {
    width: f32,
    channels: [Arc<ArrayQueue<BoundaryUpdate>>; 4],
    snapshots: [Arc<AtomicPtr<BoundarySnapshot>>; 4],
}

impl LockFreeBoundaryZone {
    pub fn new(capacity_per_channel: usize, boundary_width: f32) -> Self {
        let default_snapshot_ptr = Box::into_raw(Box::new(BoundarySnapshot::default()));
        Self {
            width: boundary_width,
            channels: [
                Arc::new(ArrayQueue::new(capacity_per_channel)),
                Arc::new(ArrayQueue::new(capacity_per_channel)),
                Arc::new(ArrayQueue::new(capacity_per_channel)),
                Arc::new(ArrayQueue::new(capacity_per_channel)),
            ],
            snapshots: [
                Arc::new(AtomicPtr::new(default_snapshot_ptr)),
                Arc::new(AtomicPtr::new(Box::into_raw(Box::new(BoundarySnapshot::default())))),
                Arc::new(AtomicPtr::new(Box::into_raw(Box::new(BoundarySnapshot::default())))),
                Arc::new(AtomicPtr::new(Box::into_raw(Box::new(BoundarySnapshot::default())))),
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
        let update = BoundaryUpdate { player_id, action, position: (x, y) };
        if y - partition_bounds.min_y < self.width { let _ = self.channels[Direction::North as usize].push(update.clone()); }
        if partition_bounds.max_x - x < self.width { let _ = self.channels[Direction::East as usize].push(update.clone()); }
        if partition_bounds.max_y - y < self.width { let _ = self.channels[Direction::South as usize].push(update.clone()); }
        if x - partition_bounds.min_x < self.width { let _ = self.channels[Direction::West as usize].push(update.clone()); }
    }

    pub fn update_snapshots(&self) {
        for dir_idx in 0..4 {
            let direction = Direction::from_index(dir_idx).unwrap_or(Direction::North);
            self.update_direction_snapshot(direction);
        }
    }

    fn update_direction_snapshot(&self, direction: Direction) {
        let channel = &self.channels[direction as usize];
        let snapshot_atomic_ptr = &self.snapshots[direction as usize];

        // Pin the guard for safe dereferencing and defer_destroy
        let guard = &epoch::pin();

        let mut current_players_map: HashMap<PlayerID, (f32, f32)> = {
            let current_snapshot_raw_ptr = snapshot_atomic_ptr.load(Ordering::Acquire);
            if !current_snapshot_raw_ptr.is_null() {
                // Safely dereference the pointer. The guard ensures validity.
                // Shared::from creates Shared<'static, T>, as_ref() on it is fine.
                unsafe { Shared::from(current_snapshot_raw_ptr as *const BoundarySnapshot).as_ref() }
                    .map_or_else(HashMap::new, |snap| snap.players.iter().map(|(id,x,y)| (id.clone(), (*x,*y))).collect())
            } else {
                HashMap::new()
            }
        };

        while let Some(update) = channel.pop() {
            match update.action {
                BoundaryAction::Enter | BoundaryAction::Update => { current_players_map.insert(update.player_id, update.position); }
                BoundaryAction::Leave => { current_players_map.remove(&update.player_id); }
            }
        }

        let new_snapshot_data: Vec<(PlayerID, f32, f32)> = current_players_map.into_iter().map(|(id, (x,y))| (id, x,y)).collect();

        let old_snapshot_version = {
            let ptr_raw = snapshot_atomic_ptr.load(Ordering::Relaxed); // Relaxed is fine for version check
            if !ptr_raw.is_null() {
                // Safely dereference for version check. The guard ensures validity.
                unsafe { Shared::from(ptr_raw as *const BoundarySnapshot).as_ref() }
                    .map_or(0, |snap| snap.version)
            } else {
                0
            }
        };

        let new_snapshot = Box::new(BoundarySnapshot {
            players: new_snapshot_data, version: old_snapshot_version + 1, timestamp: Instant::now(),
        });
        let new_snapshot_raw_ptr = Box::into_raw(new_snapshot);

        // Swap the pointer
        let old_snapshot_raw_ptr = snapshot_atomic_ptr.swap(new_snapshot_raw_ptr, Ordering::Release);

        // Defer destruction of the old snapshot using the pinned guard
        if !old_snapshot_raw_ptr.is_null() {
            unsafe {
                guard.defer_destroy(Shared::from(old_snapshot_raw_ptr as *const BoundarySnapshot));
            }
        }
    }

    pub fn get_snapshot<'g>(&self, direction: Direction, guard: &'g Guard) -> Option<&'g BoundarySnapshot> {
        let snapshot_ptr_raw = self.snapshots[direction as usize].load(Ordering::Acquire);
        if snapshot_ptr_raw.is_null() {
            None
        } else {
            // Convert the raw pointer to Shared and then use as_ref.
            // The lifetime 'g of the returned reference is tied to the provided 'guard'.
            // This is safe because the guard ensures the data is valid for its lifetime.
            unsafe { Shared::from(snapshot_ptr_raw as *const BoundarySnapshot).as_ref() }
        }
    }
}

impl Direction {
    fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Direction::North), 1 => Some(Direction::East),
            2 => Some(Direction::South), 3 => Some(Direction::West),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct ImprovedWorldPartition {
    pub id: usize,
    pub bounds: PartitionBounds,
    pub local_players: Arc<DashSet<PlayerID>>,
    pub local_projectiles: Arc<DashSet<EntityId>>,
    pub all_walls_in_partition: Arc<DashMap<EntityId, Wall>>,
    pub dynamic_objects: Arc<DashMap<EntityId, Pickup>>,
    pub boundary_zone: Arc<LockFreeBoundaryZone>,
    pub neighbor_ids: [Option<usize>; 8],
    pub local_events: Arc<SegQueue<GameEvent>>,
}

impl ImprovedWorldPartition {
    pub fn new(id: usize, bounds: PartitionBounds, neighbor_ids: [Option<usize>; 8], boundary_config_capacity: usize) -> Self {
        ImprovedWorldPartition {
            id,
            bounds,
            local_players: Arc::new(DashSet::new()),
            local_projectiles: Arc::new(DashSet::new()),
            all_walls_in_partition: Arc::new(DashMap::new()),
            dynamic_objects: Arc::new(DashMap::new()),
            boundary_zone: Arc::new(LockFreeBoundaryZone::new(boundary_config_capacity, BOUNDARY_ZONE_WIDTH)),
            neighbor_ids,
            local_events: Arc::new(SegQueue::new()),
        }
    }

    pub fn add_wall_on_load(&self, wall: Wall) {
        self.all_walls_in_partition.insert(wall.id, wall);
    }

    pub fn get_wall(&self, wall_id: EntityId) -> Option<Wall> {
        self.all_walls_in_partition.get(&wall_id).map(|entry| entry.value().clone())
    }

    pub fn get_all_walls_snapshot(&self) -> Vec<Wall> {
        self.all_walls_in_partition.iter().map(|entry| entry.value().clone()).collect()
    }

    pub fn damage_destructible_wall(&self, wall_id: EntityId, damage: i32) -> Option<(bool, Vec2)> {
        if let Some(mut wall_entry) = self.all_walls_in_partition.get_mut(&wall_id) {
            let wall = wall_entry.value_mut();
            if wall.is_destructible && wall.current_health > 0 {
                let old_health = wall.current_health;
                wall.current_health = (wall.current_health - damage).max(0);
                debug!("[Partition {}] Wall {} damaged. Health: {} -> {}", self.id, wall_id, old_health, wall.current_health);
                if wall.current_health == 0 && old_health > 0 {
                    return Some((true, Vec2::new(wall.x + wall.width / 2.0, wall.y + wall.height / 2.0)));
                }
                return Some((false, Vec2::new(wall.x + wall.width / 2.0, wall.y + wall.height / 2.0)));
            }
        }
        None
    }

    pub fn respawn_destructible_wall(&self, wall_id: EntityId) -> bool {
        if let Some(mut wall_entry) = self.all_walls_in_partition.get_mut(&wall_id) {
            let wall = wall_entry.value_mut();
            if wall.is_destructible {
                wall.current_health = wall.max_health;
                debug!("[Partition {}] Wall {} respawned. Health: {}/{}", self.id, wall_id, wall.current_health, wall.max_health);
                return true;
            }
        }
        false
    }


    pub fn contains_point_primary(&self, x: f32, y: f32) -> bool {
        x >= self.bounds.min_x && x < self.bounds.max_x &&
        y >= self.bounds.min_y && y < self.bounds.max_y
    }

    pub fn update_player_status(&self, player_id: &PlayerID, x: f32, y: f32, is_newly_entered: bool) {
        let action = if is_newly_entered { BoundaryAction::Enter } else { BoundaryAction::Update };
        let is_near_north = y - self.bounds.min_y < self.boundary_zone.width;
        let is_near_east = self.bounds.max_x - x < self.boundary_zone.width;
        let is_near_south = self.bounds.max_y - y < self.boundary_zone.width;
        let is_near_west = x - self.bounds.min_x < self.boundary_zone.width;

        if is_near_north || is_near_east || is_near_south || is_near_west {
            self.boundary_zone.update_player_boundary_status(player_id.clone(), x, y, &self.bounds, action);
        }
        if !self.contains_point_primary(x,y) && !is_newly_entered {
             self.local_players.remove(player_id);
             self.boundary_zone.update_player_boundary_status(player_id.clone(), x, y, &self.bounds, BoundaryAction::Leave);
        } else if is_newly_entered {
            self.local_players.insert(player_id.clone());
        }
    }

    pub fn add_dynamic_object(&self, pickup: Pickup) {
        self.dynamic_objects.insert(pickup.id, pickup);
    }

    pub fn remove_dynamic_object(&self, pickup_id: &EntityId) -> Option<Pickup> {
        self.dynamic_objects.remove(pickup_id).map(|(_k,v)| v)
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
        grid_dim: usize, world_width: f32, world_height: f32,
        world_min_x: f32, world_min_y: f32, boundary_config_capacity_per_channel: usize
    ) -> Self {
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
                if y_idx > 0 { neighbor_ids[0] = Some((y_idx - 1) * grid_dim + x_idx); }
                if y_idx > 0 && x_idx < grid_dim - 1 { neighbor_ids[1] = Some((y_idx - 1) * grid_dim + (x_idx + 1)); }
                if x_idx < grid_dim - 1 { neighbor_ids[2] = Some(y_idx * grid_dim + (x_idx + 1)); }
                if y_idx < grid_dim - 1 && x_idx < grid_dim - 1 { neighbor_ids[3] = Some((y_idx + 1) * grid_dim + (x_idx + 1)); }
                if y_idx < grid_dim - 1 { neighbor_ids[4] = Some((y_idx + 1) * grid_dim + x_idx); }
                if y_idx < grid_dim - 1 && x_idx > 0 { neighbor_ids[5] = Some((y_idx + 1) * grid_dim + (x_idx - 1)); }
                if x_idx > 0 { neighbor_ids[6] = Some(y_idx * grid_dim + (x_idx - 1)); }
                if y_idx > 0 && x_idx > 0 { neighbor_ids[7] = Some((y_idx - 1) * grid_dim + (x_idx - 1)); }
                partitions.push(Arc::new(ImprovedWorldPartition::new(id, bounds, neighbor_ids, boundary_config_capacity_per_channel)));
            }
        }
        WorldPartitionManager { partitions, grid_dim, partition_width, partition_height, world_min_x, world_min_y }
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

    pub fn update_all_boundary_snapshots(&self) {
        for partition_arc in &self.partitions {
            partition_arc.boundary_zone.update_snapshots();
        }
    }
}
