// massive_game_server/server/src/entities/player.rs
use crate::core::types::{PlayerID, PlayerState};
use crate::concurrent::spatial_index::ImprovedSpatialIndex;
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
        let new_arc_id = Arc::new(id_str.to_string());
        self.allocated_ids.insert(id_str.to_string(), new_arc_id.clone());
        new_arc_id
    }

    pub fn remove(&self, id_str: &str) -> Option<PlayerID> {
        self.allocated_ids.remove(id_str).map(|(_key, arc_id)| arc_id)
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
    shards: Vec<Arc<DashMap<PlayerID, PlayerState>>>,
    num_shards: usize,
    spatial_index: Arc<ImprovedSpatialIndex>,
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

    pub fn add_player(&self, id_str: String, username: String, initial_x: f32, initial_y: f32) -> Option<PlayerID> {
        let player_arc_id = self.id_pool.get_or_create(&id_str);

        let shard_idx = self.get_shard_index(&id_str);
        if shard_idx >= self.shards.len() {
            warn!("Calculated shard index {} is out of bounds for {} shards.", shard_idx, self.shards.len());
            return None;
        }

        let player_state = PlayerState::new(id_str.clone(), username, initial_x, initial_y);

        if self.shards[shard_idx].get(&player_arc_id).is_some() {
            warn!("Player with ID {} already exists. Not adding again.", id_str);
            return None;
        }

        self.shards[shard_idx].insert(player_arc_id.clone(), player_state);
        self.spatial_index.update_player_position(player_arc_id.clone(), initial_x, initial_y);
        Some(player_arc_id)
    }

    pub fn remove_player(&self, player_id_str: &str) {
        let player_arc_id_opt = self.id_pool.allocated_ids.get(player_id_str).map(|entry| entry.value().clone());

        if let Some(player_arc_id) = player_arc_id_opt {
            let shard_idx = self.get_shard_index(player_id_str);
            if shard_idx < self.shards.len() {
                if self.shards[shard_idx].remove(&player_arc_id).is_some() {
                    self.spatial_index.remove_player(&player_arc_id);
                    self.id_pool.remove(player_id_str);
                } else {
                     warn!("Attempted to remove player {} from shard {} but they were not found.", player_id_str, shard_idx);
                }
            } else {
                 warn!("Attempted to remove player {}: shard index {} out of bounds.", player_id_str, shard_idx);
            }
        } else {
            warn!("Attempted to remove player {}: ID not found in pool.", player_id_str);
        }
    }

    pub fn update_player_position(&self, player_id: &PlayerID, new_x: f32, new_y: f32) {
        let shard_idx = self.get_shard_index(player_id.as_str());
        if shard_idx < self.shards.len() {
            if let Some(mut player_state_entry) = self.shards[shard_idx].get_mut(player_id) {
                player_state_entry.x = new_x;
                player_state_entry.y = new_y;
            }
            self.spatial_index.update_player_position(player_id.clone(), new_x, new_y);
        }
    }

    pub fn get_player_state(&self, player_id: &PlayerID) -> Option<impl std::ops::Deref<Target = PlayerState> + '_> {
        let shard_idx = self.get_shard_index(player_id.as_str());
        if shard_idx < self.shards.len() {
            self.shards[shard_idx].get(player_id)
        } else {
            None
        }
    }

    pub fn get_player_state_mut(&self, player_id: &PlayerID) -> Option<impl std::ops::DerefMut<Target = PlayerState> + '_> {
         let shard_idx = self.get_shard_index(player_id.as_str());
        if shard_idx < self.shards.len() {
            self.shards[shard_idx].get_mut(player_id)
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
                func(entry.key(), entry.value());
            }
        }
    }

    pub fn for_each_player_mut<F>(&self, mut func: F)
    where
        F: FnMut(&PlayerID, &mut PlayerState),
    {
        for shard_arc in &self.shards {
            for mut entry in shard_arc.iter_mut() {
                let key_clone = entry.key().clone();
                let value_mut = entry.value_mut();
                func(&key_clone, value_mut);
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
