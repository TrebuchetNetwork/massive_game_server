// massive_game_server/server/src/entities/player.rs
use crate::concurrent::spatial_index::ImprovedSpatialIndex;
use crate::core::types::{PlayerID, PlayerState, Vec2};
use arc_swap::ArcSwap;
use dashmap::DashMap;
use parking_lot::Mutex;
use seahash;
use std::sync::atomic::{AtomicU8, Ordering as AtomicOrdering};
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
    next_balanced_team: AtomicU8,
    team_assignment_lock: Mutex<()>,
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
    if original.melee_windup_remaining != updated.melee_windup_remaining {
        base.melee_windup_remaining = updated.melee_windup_remaining;
    }
    if original.melee_pending_attack != updated.melee_pending_attack {
        base.melee_pending_attack = updated.melee_pending_attack;
    }
    if original.melee_windup_rotation != updated.melee_windup_rotation {
        base.melee_windup_rotation = updated.melee_windup_rotation;
    }
    if original.wall_slam_stun_remaining != updated.wall_slam_stun_remaining {
        base.wall_slam_stun_remaining = updated.wall_slam_stun_remaining;
    }
    if original.wall_slam_tumble_remaining != updated.wall_slam_tumble_remaining {
        base.wall_slam_tumble_remaining = updated.wall_slam_tumble_remaining;
    }
    if original.dash_melee_chain_bonus_remaining != updated.dash_melee_chain_bonus_remaining {
        base.dash_melee_chain_bonus_remaining = updated.dash_melee_chain_bonus_remaining;
    }
    if original.dodge_shot_chain_bonus_remaining != updated.dodge_shot_chain_bonus_remaining {
        base.dodge_shot_chain_bonus_remaining = updated.dodge_shot_chain_bonus_remaining;
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
    if original.career_kills_per_weapon != updated.career_kills_per_weapon {
        base.career_kills_per_weapon = updated.career_kills_per_weapon;
    }
    if original.hot_zone_kills != updated.hot_zone_kills {
        base.hot_zone_kills = updated.hot_zone_kills;
    }
    if original.hot_zone_time_ticks != updated.hot_zone_time_ticks {
        base.hot_zone_time_ticks = updated.hot_zone_time_ticks;
    }
    if original.is_bot != updated.is_bot {
        base.is_bot = updated.is_bot;
    }
    if original.bot_behavior != updated.bot_behavior {
        base.bot_behavior = updated.bot_behavior;
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
    if original.peak_streak != updated.peak_streak {
        base.peak_streak = updated.peak_streak;
    }
    if original.streak_damage_boost_remaining != updated.streak_damage_boost_remaining {
        base.streak_damage_boost_remaining = updated.streak_damage_boost_remaining;
    }
    if original.streak_speed_boost_remaining != updated.streak_speed_boost_remaining {
        base.streak_speed_boost_remaining = updated.streak_speed_boost_remaining;
    }
    if original.killstreak_reward_preference != updated.killstreak_reward_preference {
        base.killstreak_reward_preference = updated.killstreak_reward_preference;
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
    // Lazily materialized mutable copy. Many write guards are dropped without
    // mutation, so avoid cloning PlayerState up front in that case.
    working: Option<PlayerState>,
}

impl PlayerStateWriteGuard {
    fn new(cell: Arc<ArcSwap<PlayerState>>) -> Self {
        let original = cell.load_full();
        Self {
            cell,
            original,
            working: None,
        }
    }
}

impl std::ops::Deref for PlayerStateWriteGuard {
    type Target = PlayerState;

    fn deref(&self) -> &Self::Target {
        self.working.as_ref().unwrap_or(self.original.as_ref())
    }
}

impl std::ops::DerefMut for PlayerStateWriteGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.working.get_or_insert_with(|| (*self.original).clone())
    }
}

impl Drop for PlayerStateWriteGuard {
    fn drop(&mut self) {
        let Some(working) = self.working.take() else {
            // Guard never observed mutably.
            return;
        };

        if working == *self.original {
            return;
        }

        let original = Arc::clone(&self.original);
        let desired = Arc::new(working);
        self.cell.rcu(|current| {
            if Arc::ptr_eq(current, &original) {
                Arc::clone(&desired)
            } else {
                let mut merged = (**current).clone();
                merge_player_state_delta(&mut merged, original.as_ref(), desired.as_ref());
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
            next_balanced_team: AtomicU8::new(0),
            team_assignment_lock: Mutex::new(()),
        }
    }

    fn get_shard_index(&self, player_id_str: &str) -> usize {
        (seahash::hash(player_id_str.as_bytes()) % self.num_shards as u64) as usize
    }

    fn assign_team_to_new_player_unlocked(&self) -> u8 {
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

        if team1_count < team2_count {
            return 1;
        }
        if team2_count < team1_count {
            return 2;
        }

        // Tie-break with an atomic alternator so concurrent joins from a tied
        // snapshot do not all collapse onto the same team.
        if self
            .next_balanced_team
            .fetch_add(1, AtomicOrdering::Relaxed)
            .is_multiple_of(2)
        {
            1
        } else {
            2
        }
    }

    pub fn assign_team_to_new_player(&self) -> u8 {
        let _assignment_guard = self.team_assignment_lock.lock();
        self.assign_team_to_new_player_unlocked()
    }

    /// Adds a newly-joining player while holding the team-assignment lock so
    /// that team selection and insertion are atomic with respect to other joins.
    pub fn add_player_for_join<F>(
        &self,
        id_str: String,
        username: String,
        requested_team: Option<u8>,
        requested_spectator: bool,
        mut spawn_resolver: F,
    ) -> Option<(PlayerID, u8, Vec2)>
    where
        F: FnMut(&PlayerID, u8) -> Vec2,
    {
        let _assignment_guard = self.team_assignment_lock.lock();
        let assigned_team = if requested_spectator {
            0
        } else {
            requested_team
                .filter(|team| *team == 1 || *team == 2)
                .unwrap_or_else(|| self.assign_team_to_new_player_unlocked())
        };

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
        if self.shards[shard_idx].get(&player_arc_id).is_some() {
            warn!(
                "Player with ID {} already exists. Not adding again.",
                id_str
            );
            return None;
        }

        let spawn = spawn_resolver(&player_arc_id, assigned_team);
        let mut player_state = PlayerState::new(id_str.clone(), username, spawn.x, spawn.y);
        player_state.team_id = assigned_team;
        player_state.is_spectator = requested_spectator;
        if requested_spectator {
            player_state.health = player_state.max_health;
            player_state.respawn_timer = None;
            player_state.reload_progress = None;
        }

        self.shards[shard_idx].insert(
            player_arc_id.clone(),
            Arc::new(ArcSwap::from_pointee(player_state)),
        );
        self.spatial_index
            .update_player_position(player_arc_id.clone(), spawn.x, spawn.y);
        Some((player_arc_id, assigned_team, spawn))
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
                    // Authoritative reposition: keep the anti-cheat reference
                    // in sync so the next physics tick does not flag it.
                    next.last_valid_position = (new_x, new_y);
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

    pub fn player_ids_snapshot(&self) -> Vec<PlayerID> {
        let mut ids = Vec::new();
        for shard in &self.shards {
            ids.reserve(shard.len());
            for entry in shard.iter() {
                ids.push(entry.key().clone());
            }
        }
        ids
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_spatial_index() -> Arc<ImprovedSpatialIndex> {
        Arc::new(ImprovedSpatialIndex::new(
            1000.0, 1000.0, -500.0, -500.0, 100.0,
        ))
    }

    fn test_manager(num_shards: usize) -> ImprovedPlayerManager {
        ImprovedPlayerManager::new(num_shards, test_spatial_index())
    }

    #[test]
    fn player_id_pool_reuses_existing_id_and_remove_clears_it() {
        let pool = PlayerIdPool::new();

        let first = pool.get_or_create("player-1");
        let second = pool.get_or_create("player-1");
        assert!(Arc::ptr_eq(&first, &second));

        let removed = pool.remove("player-1").expect("id should be removable");
        assert!(Arc::ptr_eq(&first, &removed));
        assert!(pool.remove("player-1").is_none());
    }

    #[test]
    fn add_player_roundtrip_and_duplicate_rejected() {
        let manager = test_manager(8);
        let player_id = manager
            .add_player("p1".into(), "alice".into(), 12.0, 34.0)
            .expect("first insert should succeed");
        assert!(manager
            .add_player("p1".into(), "alice".into(), 12.0, 34.0)
            .is_none());

        let state = manager
            .get_player_state(&player_id)
            .expect("state should exist");
        assert_eq!(state.id.as_ref(), "p1");
        assert_eq!(state.username, "alice");
        assert_eq!(state.x, 12.0);
        assert_eq!(state.y, 34.0);
        assert_eq!(manager.player_count(), 1);
    }

    #[test]
    fn update_and_remove_player_updates_spatial_index() {
        let manager = test_manager(4);
        let player_id = manager
            .add_player("p2".into(), "bob".into(), 0.0, 0.0)
            .expect("insert should succeed");

        manager.update_player_position(&player_id, 220.0, -140.0);
        let state = manager
            .get_player_state(&player_id)
            .expect("state should still exist");
        assert_eq!(state.x, 220.0);
        assert_eq!(state.y, -140.0);

        let nearby = manager
            .spatial_index
            .query_nearby_players(220.0, -140.0, 1.0);
        assert_eq!(nearby.len(), 1);
        assert_eq!(nearby[0], player_id);

        manager.remove_player("p2");
        assert!(manager.get_player_state(&player_id).is_none());
        assert_eq!(manager.player_count(), 0);
        assert!(manager
            .spatial_index
            .query_nearby_players(220.0, -140.0, 5.0)
            .is_empty());
    }

    #[test]
    fn write_guard_drop_without_mutation_keeps_state_unchanged() {
        let manager = test_manager(2);
        let player_id = manager
            .add_player("p3".into(), "carol".into(), 7.0, 9.0)
            .expect("insert should succeed");

        {
            let _guard = manager
                .get_player_state_mut(&player_id)
                .expect("write guard should exist");
            // Intentionally no mutation.
        }

        let state = manager
            .get_player_state(&player_id)
            .expect("state should exist");
        assert_eq!(state.x, 7.0);
        assert_eq!(state.y, 9.0);
    }

    #[test]
    fn write_guard_merge_preserves_concurrent_updates() {
        let manager = test_manager(2);
        let player_id = manager
            .add_player("p4".into(), "dave".into(), 1.0, 2.0)
            .expect("insert should succeed");

        let mut guard_a = manager
            .get_player_state_mut(&player_id)
            .expect("first guard should exist");
        let mut guard_b = manager
            .get_player_state_mut(&player_id)
            .expect("second guard should exist");

        guard_a.x = 77.0;
        drop(guard_a);

        guard_b.health = 42;
        drop(guard_b);

        let state = manager
            .get_player_state(&player_id)
            .expect("state should exist");
        assert_eq!(state.x, 77.0);
        assert_eq!(state.health, 42);
    }

    #[test]
    fn assign_team_balances_and_alternates_on_ties() {
        let manager = test_manager(4);

        // Tie alternation: 1, 2, 1...
        assert_eq!(manager.assign_team_to_new_player(), 1);
        assert_eq!(manager.assign_team_to_new_player(), 2);
        assert_eq!(manager.assign_team_to_new_player(), 1);

        let p1 = manager
            .add_player("team-a".into(), "a".into(), 0.0, 0.0)
            .expect("insert team-a");
        let p2 = manager
            .add_player("team-b".into(), "b".into(), 1.0, 0.0)
            .expect("insert team-b");

        manager
            .get_player_state_mut(&p1)
            .expect("state for team-a")
            .team_id = 1;
        manager
            .get_player_state_mut(&p2)
            .expect("state for team-b")
            .team_id = 1;

        // Team 1 is overrepresented, so next assignment should choose team 2.
        assert_eq!(manager.assign_team_to_new_player(), 2);
    }

    #[test]
    fn add_player_for_join_honors_requested_team_and_spectator_mode() {
        let manager = test_manager(2);

        let (player_id, assigned_team, spawn) = manager
            .add_player_for_join(
                "join-1".into(),
                "eva".into(),
                Some(2),
                false,
                |_id, team| {
                    assert_eq!(team, 2);
                    Vec2::new(50.0, -10.0)
                },
            )
            .expect("join insert should succeed");

        assert_eq!(assigned_team, 2);
        assert_eq!(spawn.x, 50.0);
        assert_eq!(spawn.y, -10.0);

        let joined_state = manager
            .get_player_state(&player_id)
            .expect("joined state should exist");
        assert_eq!(joined_state.team_id, 2);
        assert!(!joined_state.is_spectator);

        let (spectator_id, spectator_team, _) = manager
            .add_player_for_join(
                "join-spec".into(),
                "spectator".into(),
                Some(1),
                true,
                |_id, team| {
                    assert_eq!(team, 0);
                    Vec2::new(0.0, 0.0)
                },
            )
            .expect("spectator join should succeed");
        assert_eq!(spectator_team, 0);

        let spectator_state = manager
            .get_player_state(&spectator_id)
            .expect("spectator state should exist");
        assert!(spectator_state.is_spectator);
        assert_eq!(spectator_state.team_id, 0);
        assert_eq!(spectator_state.health, spectator_state.max_health);
        assert!(spectator_state.respawn_timer.is_none());
        assert!(spectator_state.reload_progress.is_none());
    }

    #[test]
    fn for_each_player_mut_applies_changes_to_all_players() {
        let manager = test_manager(4);
        let p1 = manager
            .add_player("iter-1".into(), "p1".into(), 1.0, 1.0)
            .expect("insert iter-1");
        let p2 = manager
            .add_player("iter-2".into(), "p2".into(), 2.0, 2.0)
            .expect("insert iter-2");

        manager.for_each_player_mut(|_id, state| {
            state.score += 10;
            state.kills += 1;
        });

        let mut seen = 0usize;
        manager.for_each_player(|id, state| {
            if id == &p1 || id == &p2 {
                seen += 1;
            }
            assert_eq!(state.score, 10);
            assert_eq!(state.kills, 1);
        });
        assert_eq!(seen, 2);
    }
}
