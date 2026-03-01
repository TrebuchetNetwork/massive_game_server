// massive_game_server/server/src/entities/player.rs
use crate::concurrent::spatial_index::ImprovedSpatialIndex;
use crate::core::types::{PlayerID, PlayerState};
use arc_swap::ArcSwap;
use dashmap::DashMap;
use seahash;
use std::sync::Arc;
use tracing::warn;

// Player ID Pool
pub struct PlayerIdPool {
    allocated_ids: Arc<DashMap<String, PlayerID>>,
}

impl PlayerIdPool {
    pub fn new() -> Self {
        PlayerIdPool {
            allocated_ids: Arc::new(DashMap::new()),
        }
    }

    pub fn get_or_create(&self, id_str: &str) -> PlayerID {
        if let Some(existing_arc) = self.allocated_ids.get(id_str) {
            return existing_arc.value().clone();
        }
        let new_arc_id: PlayerID = Arc::from(id_str.to_owned());
        self.allocated_ids
            .insert(id_str.to_string(), new_arc_id.clone());
        new_arc_id
    }

    pub fn remove(&self, id_str: &str) -> Option<PlayerID> {
        self.allocated_ids
            .remove(id_str)
            .map(|(_key, arc_id)| arc_id)
    }
}

impl Default for PlayerIdPool {
    fn default() -> Self {
        Self::new()
    }
}

// Improved Player Manager
pub struct ImprovedPlayerManager {
    pub id_pool: Arc<PlayerIdPool>,
    shards: Vec<Arc<DashMap<PlayerID, Arc<ArcSwap<PlayerState>>>>>,
    num_shards: usize,
    spatial_index: Arc<ImprovedSpatialIndex>,
}

fn merge_player_state_delta(base: &mut PlayerState, original: &PlayerState, updated: &PlayerState) {
    if original.id != updated.id {
        base.id = updated.id.clone();
    }
    if original.username != updated.username {
        base.username = updated.username.clone();
    }
    if original.is_spectator != updated.is_spectator {
        base.is_spectator = updated.is_spectator;
    }
    if original.x != updated.x {
        base.x = updated.x;
    }
    if original.y != updated.y {
        base.y = updated.y;
    }
    if original.velocity_x != updated.velocity_x {
        base.velocity_x = updated.velocity_x;
    }
    if original.velocity_y != updated.velocity_y {
        base.velocity_y = updated.velocity_y;
    }
    if original.rotation != updated.rotation {
        base.rotation = updated.rotation;
    }
    if original.health != updated.health {
        base.health = updated.health;
    }
    if original.max_health != updated.max_health {
        base.max_health = updated.max_health;
    }
    if original.alive != updated.alive {
        base.alive = updated.alive;
    }
    if original.last_processed_input_sequence != updated.last_processed_input_sequence {
        base.last_processed_input_sequence = updated.last_processed_input_sequence;
    }
    if original.input_queue != updated.input_queue {
        base.input_queue = updated.input_queue.clone();
    }
    if original.score != updated.score {
        base.score = updated.score;
    }
    if original.kills != updated.kills {
        base.kills = updated.kills;
    }
    if original.deaths != updated.deaths {
        base.deaths = updated.deaths;
    }
    if original.team_id != updated.team_id {
        base.team_id = updated.team_id;
    }
    if original.last_update_timestamp != updated.last_update_timestamp {
        base.last_update_timestamp = updated.last_update_timestamp;
    }
    if original.weapon != updated.weapon {
        base.weapon = updated.weapon;
    }
    if original.ammo != updated.ammo {
        base.ammo = updated.ammo;
    }
    if original.primary_weapon != updated.primary_weapon {
        base.primary_weapon = updated.primary_weapon;
    }
    if original.primary_ammo != updated.primary_ammo {
        base.primary_ammo = updated.primary_ammo;
    }
    if original.secondary_weapon != updated.secondary_weapon {
        base.secondary_weapon = updated.secondary_weapon;
    }
    if original.secondary_ammo != updated.secondary_ammo {
        base.secondary_ammo = updated.secondary_ammo;
    }
    if original.weapon_swap_progress != updated.weapon_swap_progress {
        base.weapon_swap_progress = updated.weapon_swap_progress;
    }
    if original.pending_weapon_swap != updated.pending_weapon_swap {
        base.pending_weapon_swap = updated.pending_weapon_swap;
    }
    if original.respawn_timer != updated.respawn_timer {
        base.respawn_timer = updated.respawn_timer;
    }
    if original.reload_progress != updated.reload_progress {
        base.reload_progress = updated.reload_progress;
    }
    if original.last_shot_time != updated.last_shot_time {
        base.last_shot_time = updated.last_shot_time;
    }
    if original.ability_1_cooldown_remaining != updated.ability_1_cooldown_remaining {
        base.ability_1_cooldown_remaining = updated.ability_1_cooldown_remaining;
    }
    if original.ability_2_cooldown_remaining != updated.ability_2_cooldown_remaining {
        base.ability_2_cooldown_remaining = updated.ability_2_cooldown_remaining;
    }
    if original.dash_remaining != updated.dash_remaining {
        base.dash_remaining = updated.dash_remaining;
    }
    if original.dodge_roll_remaining != updated.dodge_roll_remaining {
        base.dodge_roll_remaining = updated.dodge_roll_remaining;
    }
    if original.invulnerable_remaining != updated.invulnerable_remaining {
        base.invulnerable_remaining = updated.invulnerable_remaining;
    }
    if original.ping_cooldown_remaining != updated.ping_cooldown_remaining {
        base.ping_cooldown_remaining = updated.ping_cooldown_remaining;
    }
    if original.zone_boost_cooldown_remaining != updated.zone_boost_cooldown_remaining {
        base.zone_boost_cooldown_remaining = updated.zone_boost_cooldown_remaining;
    }
    if original.speed_boost_remaining != updated.speed_boost_remaining {
        base.speed_boost_remaining = updated.speed_boost_remaining;
    }
    if original.damage_boost_remaining != updated.damage_boost_remaining {
        base.damage_boost_remaining = updated.damage_boost_remaining;
    }
    if original.shield_current != updated.shield_current {
        base.shield_current = updated.shield_current;
    }
    if original.shield_max != updated.shield_max {
        base.shield_max = updated.shield_max;
    }
    if original.is_carrying_flag_team_id != updated.is_carrying_flag_team_id {
        base.is_carrying_flag_team_id = updated.is_carrying_flag_team_id;
    }
    if original.damage_dealt != updated.damage_dealt {
        base.damage_dealt = updated.damage_dealt;
    }
    if original.damage_taken != updated.damage_taken {
        base.damage_taken = updated.damage_taken;
    }
    if original.flag_captures != updated.flag_captures {
        base.flag_captures = updated.flag_captures;
    }
    if original.flag_returns != updated.flag_returns {
        base.flag_returns = updated.flag_returns;
    }
    if original.kills_per_weapon != updated.kills_per_weapon {
        base.kills_per_weapon = updated.kills_per_weapon;
    }
    if original.last_valid_position != updated.last_valid_position {
        base.last_valid_position = updated.last_valid_position;
    }
    if original.violation_count != updated.violation_count {
        base.violation_count = updated.violation_count;
    }
    if original.changed_fields != updated.changed_fields {
        base.changed_fields = updated.changed_fields;
    }
    if original.current_streak != updated.current_streak {
        base.current_streak = updated.current_streak;
    }
    if original.streak_damage_boost_remaining != updated.streak_damage_boost_remaining {
        base.streak_damage_boost_remaining = updated.streak_damage_boost_remaining;
    }
    if original.streak_speed_boost_remaining != updated.streak_speed_boost_remaining {
        base.streak_speed_boost_remaining = updated.streak_speed_boost_remaining;
    }
    if original.recent_damage_sources != updated.recent_damage_sources {
        base.recent_damage_sources = updated.recent_damage_sources.clone();
    }
    if original.last_queued_input_sequence != updated.last_queued_input_sequence {
        base.last_queued_input_sequence = updated.last_queued_input_sequence;
    }
    if original.prev_velocity != updated.prev_velocity {
        base.prev_velocity = updated.prev_velocity;
    }
    if original.acceleration_violation_count != updated.acceleration_violation_count {
        base.acceleration_violation_count = updated.acceleration_violation_count;
    }
}

pub struct PlayerStateReadGuard {
    snapshot: Arc<PlayerState>,
}

impl std::ops::Deref for PlayerStateReadGuard {
    type Target = PlayerState;

    fn deref(&self) -> &Self::Target {
        self.snapshot.as_ref()
    }
}

pub struct PlayerStateWriteGuard {
    cell: Arc<ArcSwap<PlayerState>>,
    original: Arc<PlayerState>,
    working: PlayerState,
}

impl PlayerStateWriteGuard {
    fn new(cell: Arc<ArcSwap<PlayerState>>) -> Self {
        let original = cell.load_full();
        let working = (*original).clone();
        Self {
            cell,
            original,
            working,
        }
    }
}

impl std::ops::Deref for PlayerStateWriteGuard {
    type Target = PlayerState;

    fn deref(&self) -> &Self::Target {
        &self.working
    }
}

impl std::ops::DerefMut for PlayerStateWriteGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.working
    }
}

impl Drop for PlayerStateWriteGuard {
    fn drop(&mut self) {
        if self.working == *self.original {
            return;
        }

        let original = self.original.clone();
        let updated = self.working.clone();
        self.cell.rcu(|current| {
            if Arc::ptr_eq(current, &original) {
                Arc::new(updated.clone())
            } else {
                let mut merged = (**current).clone();
                merge_player_state_delta(&mut merged, original.as_ref(), &updated);
                Arc::new(merged)
            }
        });
    }
}

impl ImprovedPlayerManager {
    pub fn new(num_shards: usize, spatial_index: Arc<ImprovedSpatialIndex>) -> Self {
        let mut shards = Vec::with_capacity(num_shards);
        for _ in 0..num_shards {
            shards.push(Arc::new(DashMap::new()));
        }
        ImprovedPlayerManager {
            id_pool: Arc::new(PlayerIdPool::new()),
            shards,
            num_shards,
            spatial_index,
        }
    }

    fn get_shard_index(&self, player_id_str: &str) -> usize {
        (seahash::hash(player_id_str.as_bytes()) % self.num_shards as u64) as usize
    }

    pub fn assign_team_to_new_player(&self) -> u8 {
        let mut team1_count = 0;
        let mut team2_count = 0;

        self.for_each_player(|_id, p_state| {
            // Consider only counting human players for balancing if bots are managed separately
            // For now, counts all players assigned to a team.
            if p_state.team_id == 1 {
                team1_count += 1;
            } else if p_state.team_id == 2 {
                team2_count += 1;
            }
        });

        if team1_count <= team2_count {
            1 // Assign to Red team
        } else {
            2 // Assign to Blue team
        }
    }

    pub fn add_player(
        &self,
        id_str: String,
        username: String,
        initial_x: f32,
        initial_y: f32,
    ) -> Option<PlayerID> {
        let player_arc_id = self.id_pool.get_or_create(&id_str);

        let shard_idx = self.get_shard_index(&id_str);
        if shard_idx >= self.shards.len() {
            warn!(
                "Calculated shard index {} is out of bounds for {} shards.",
                shard_idx,
                self.shards.len()
            );
            return None;
        }

        let player_state = PlayerState::new(id_str.clone(), username, initial_x, initial_y);

        if self.shards[shard_idx].get(&player_arc_id).is_some() {
            warn!(
                "Player with ID {} already exists. Not adding again.",
                id_str
            );
            return None;
        }

        self.shards[shard_idx].insert(
            player_arc_id.clone(),
            Arc::new(ArcSwap::from_pointee(player_state)),
        );
        self.spatial_index
            .update_player_position(player_arc_id.clone(), initial_x, initial_y);
        Some(player_arc_id)
    }

    pub fn remove_player(&self, player_id_str: &str) {
        let player_arc_id_opt = self
            .id_pool
            .allocated_ids
            .get(player_id_str)
            .map(|entry| entry.value().clone());

        if let Some(player_arc_id) = player_arc_id_opt {
            let shard_idx = self.get_shard_index(player_id_str);
            if shard_idx < self.shards.len() {
                if self.shards[shard_idx].remove(&player_arc_id).is_some() {
                    self.spatial_index.remove_player(&player_arc_id);
                    self.id_pool.remove(player_id_str);
                } else {
                    warn!(
                        "Attempted to remove player {} from shard {} but they were not found.",
                        player_id_str, shard_idx
                    );
                }
            } else {
                warn!(
                    "Attempted to remove player {}: shard index {} out of bounds.",
                    player_id_str, shard_idx
                );
            }
        } else {
            warn!(
                "Attempted to remove player {}: ID not found in pool.",
                player_id_str
            );
        }
    }

    pub fn update_player_position(&self, player_id: &PlayerID, new_x: f32, new_y: f32) {
        let shard_idx = self.get_shard_index(player_id.as_ref());
        if shard_idx < self.shards.len() {
            if let Some(player_state_cell) = self.shards[shard_idx]
                .get(player_id)
                .map(|entry| entry.value().clone())
            {
                player_state_cell.rcu(|current| {
                    let mut next = (**current).clone();
                    next.x = new_x;
                    next.y = new_y;
                    Arc::new(next)
                });
            }
            self.spatial_index
                .update_player_position(player_id.clone(), new_x, new_y);
        }
    }

    pub fn get_player_state(&self, player_id: &PlayerID) -> Option<PlayerStateReadGuard> {
        let shard_idx = self.get_shard_index(player_id.as_ref());
        if shard_idx < self.shards.len() {
            self.shards[shard_idx]
                .get(player_id)
                .map(|entry| PlayerStateReadGuard {
                    snapshot: entry.value().load_full(),
                })
        } else {
            None
        }
    }

    pub fn get_player_state_mut(&self, player_id: &PlayerID) -> Option<PlayerStateWriteGuard> {
        let shard_idx = self.get_shard_index(player_id.as_ref());
        if shard_idx < self.shards.len() {
            self.shards[shard_idx]
                .get(player_id)
                .map(|entry| PlayerStateWriteGuard::new(entry.value().clone()))
        } else {
            None
        }
    }

    pub fn for_each_player<F>(&self, mut func: F)
    where
        F: FnMut(&PlayerID, &PlayerState),
    {
        for shard in &self.shards {
            for entry in shard.iter() {
                let snapshot = entry.value().load_full();
                func(entry.key(), snapshot.as_ref());
            }
        }
    }

    pub fn for_each_player_mut<F>(&self, mut func: F)
    where
        F: FnMut(&PlayerID, &mut PlayerState),
    {
        for shard_arc in &self.shards {
            for entry in shard_arc.iter() {
                let key_clone = entry.key().clone();
                let mut guard = PlayerStateWriteGuard::new(entry.value().clone());
                func(&key_clone, &mut guard);
            }
        }
    }

    // Method to count total players
    pub fn player_count(&self) -> usize {
        let mut count = 0;
        for shard in &self.shards {
            count += shard.len();
        }
        count
    }
}
