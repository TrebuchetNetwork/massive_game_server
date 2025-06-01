// massive_game_server/server/src/systems/respawn.rs

use crate::core::types::{PlayerID, Wall, EntityId, Vec2};
use crate::core::constants::*;
use crate::server::instance::MassiveGameServer; // Added for server access
use dashmap::DashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use std::time::{Instant, Duration};
use rand::Rng;
use tracing::{debug, warn};

#[derive(Clone, Debug)]
pub struct SpawnPoint {
    pub position: Vec2,
    pub last_used: Instant,
    pub team_id: Option<u8>,
    pub spawn_type: SpawnType,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpawnType {
    TeamBase,
    Safe,
    Contested,
    Arena,
}

pub struct RespawnManager {
    spawn_points: Arc<RwLock<Vec<SpawnPoint>>>,
    recent_deaths: Arc<DashMap<PlayerID, (Vec2, Instant)>>,
    spawn_protection_duration: Duration,
}

impl RespawnManager {
    pub fn new() -> Self { // Removed server parameter for now, will be passed to get_respawn_position
        let initial_spawn_points = Self::generate_initial_spawn_points();
        Self {
            spawn_points: Arc::new(RwLock::new(initial_spawn_points)),
            recent_deaths: Arc::new(DashMap::new()),
            spawn_protection_duration: Duration::from_secs(3),
        }
    }

    fn generate_initial_spawn_points() -> Vec<SpawnPoint> {
        let mut spawns = Vec::new();
        let now = Instant::now() - Duration::from_secs(60);

        let team_spawns = crate::world::map_generator::MapGenerator::get_team_spawn_areas();
        for (pos, team_id) in team_spawns {
            spawns.push(SpawnPoint {
                position: pos,
                last_used: now,
                team_id: Some(team_id),
                spawn_type: SpawnType::TeamBase,
            });
        }

        let safe_positions = [
            Vec2::new(WORLD_MIN_X + 150.0, WORLD_MIN_Y + 150.0),
            Vec2::new(WORLD_MAX_X - 150.0, WORLD_MIN_Y + 150.0),
            Vec2::new(WORLD_MIN_X + 150.0, WORLD_MAX_Y - 150.0),
            Vec2::new(WORLD_MAX_X - 150.0, WORLD_MAX_Y - 150.0),
        ];
        for pos in &safe_positions {
            spawns.push(SpawnPoint {
                position: *pos,
                last_used: now,
                team_id: None,
                spawn_type: SpawnType::Safe,
            });
        }

        let contested_positions = [
            Vec2::new(0.0, WORLD_MIN_Y + 200.0),
            Vec2::new(0.0, WORLD_MAX_Y - 200.0),
            Vec2::new(WORLD_MIN_X + 200.0, 0.0),
            Vec2::new(WORLD_MAX_X - 200.0, 0.0),
        ];
        for pos in &contested_positions {
            spawns.push(SpawnPoint {
                position: *pos,
                last_used: now,
                team_id: None,
                spawn_type: SpawnType::Contested,
            });
        }

        let arena_radius = 250.0;
        for i in 0..4 {
            let angle = (i as f32) * 2.0 * std::f32::consts::PI / 4.0;
            spawns.push(SpawnPoint {
                position: Vec2::new(arena_radius * angle.cos(), arena_radius * angle.sin()),
                last_used: now,
                team_id: None,
                spawn_type: SpawnType::Arena,
            });
        }
        spawns
    }

    pub fn record_death(&self, player_id: PlayerID, position: Vec2) {
        self.recent_deaths.insert(player_id, (position, Instant::now()));
        self.recent_deaths.retain(|_, (_, time)| time.elapsed() < Duration::from_secs(60));
    }

    fn is_spawn_point_obstructed(
        &self,
        spawn_pos: Vec2,
        // static_walls: &[Wall], // No longer separate, combined in all_current_walls
        all_current_walls: &[Wall],
    ) -> bool {
        let player_check_radius = PLAYER_RADIUS + 5.0;

        for wall in all_current_walls { // Iterate over combined list
            if wall.is_destructible && wall.current_health <= 0 {
                continue;
            }
            let closest_x = spawn_pos.x.clamp(wall.x, wall.x + wall.width);
            let closest_y = spawn_pos.y.clamp(wall.y, wall.y + wall.height);
            let distance_x = spawn_pos.x - closest_x;
            let distance_y = spawn_pos.y - closest_y;
            let distance_squared = (distance_x * distance_x) + (distance_y * distance_y);
            if distance_squared < (player_check_radius * player_check_radius) {
                return true;
            }
        }
        false
    }

    // Modified to accept &MassiveGameServer to access wall data
    pub fn get_respawn_position(
        &self,
        server: &MassiveGameServer, // Added server parameter
        player_id: &PlayerID,
        team_id: Option<u8>,
        enemy_positions: &[(Vec2, PlayerID)],
    ) -> Vec2 {
        // Fetch current wall states using the server reference
        let all_current_walls = server.collect_all_walls_current_state();
        // The original user snippet separated static and dynamic, but collect_all_walls_current_state
        // seems to return all relevant walls. If you have a separate static_walls field on server,
        // you'd pass that too. For now, assuming collect_all_walls_current_state is sufficient.
        // let static_walls = server.static_walls.read(); // If server has such a field
        // let dynamic_walls = server.collect_all_walls_current_state(); // This might be redundant if above is used

        self.get_respawn_position_with_walls(
            player_id,
            team_id,
            enemy_positions,
            &all_current_walls, // Pass the fetched walls
            // &dynamic_walls // If you separate them, pass both
        )
    }


    // Modified to take a single all_current_walls slice
    pub fn get_respawn_position_with_walls(
        &self,
        player_id: &PlayerID,
        team_id: Option<u8>,
        enemy_positions: &[(Vec2, PlayerID)],
        all_current_walls: &[Wall], // Combined wall list
    ) -> Vec2 {
        let mut rng = rand::thread_rng();
        let now = Instant::now();
        let death_location_opt = self.recent_deaths.get(player_id).map(|entry| entry.value().0);
        let mut spawn_points_guard = self.spawn_points.write();

        let mut scored_spawns: Vec<(usize, f32)> = spawn_points_guard
            .iter()
            .enumerate()
            .filter_map(|(idx, sp)| {
                let team_compatible = match (team_id, sp.team_id) {
                    (Some(p_team), Some(sp_team)) => p_team == sp_team,
                    (Some(_p_team), None) => true,
                    (None, None) => true,
                    (None, Some(_sp_team)) => false,
                };
                if !team_compatible { return None; }
                // Pass the combined wall list to is_spawn_point_obstructed
                if self.is_spawn_point_obstructed(sp.position, all_current_walls) {
                    return None;
                }

                let mut score = 100.0;
                let time_since_last_use = now.duration_since(sp.last_used).as_secs_f32();
                if time_since_last_use < self.spawn_protection_duration.as_secs_f32() * 2.0 {
                    score -= (self.spawn_protection_duration.as_secs_f32() * 2.0 - time_since_last_use) * 15.0;
                }
                if let Some(death_loc) = death_location_opt {
                    let dist_from_death = ((sp.position.x - death_loc.x).powi(2) + (sp.position.y - death_loc.y).powi(2)).sqrt();
                    score += dist_from_death * 0.1;
                }
                let mut min_dist_to_enemy = f32::MAX;
                for (enemy_pos, _) in enemy_positions {
                    let dist = ((sp.position.x - enemy_pos.x).powi(2) + (sp.position.y - enemy_pos.y).powi(2)).sqrt();
                    if dist < min_dist_to_enemy {
                        min_dist_to_enemy = dist;
                    }
                }
                if min_dist_to_enemy < SAFE_SPAWN_RADIUS_FROM_ENEMY {
                    score -= (SAFE_SPAWN_RADIUS_FROM_ENEMY - min_dist_to_enemy) * 0.5;
                } else {
                    score += min_dist_to_enemy * 0.05;
                }
                if let (Some(p_team), Some(sp_team)) = (team_id, sp.team_id) {
                    if p_team == sp_team && sp.spawn_type == SpawnType::TeamBase {
                        score += 50.0;
                    }
                } else if sp.spawn_type == SpawnType::Safe {
                    score += 20.0;
                }
                Some((idx, score.max(0.0)))
            })
            .collect();

        if scored_spawns.is_empty() {
             for (idx, sp) in spawn_points_guard.iter().enumerate() {
                 if !self.is_spawn_point_obstructed(sp.position, all_current_walls) { // Check with combined walls
                    spawn_points_guard[idx].last_used = now;
                    return spawn_points_guard[idx].position;
                 }
             }
            warn!("[RESPAWN_WARN] All spawn points are obstructed or unsuitable! Returning default (0,0). Player: {:?}", player_id);
            return Vec2::new(0.0, 0.0);
        }

        scored_spawns.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let top_n = scored_spawns.iter().take(3).collect::<Vec<_>>();
        if top_n.is_empty() {
            warn!("[RESPAWN_WARN] No top N spawns found, though scored_spawns was not empty. Player: {:?}", player_id);
            if let Some(&(best_idx, _)) = scored_spawns.first() {
                 if best_idx < spawn_points_guard.len() {
                    spawn_points_guard[best_idx].last_used = now;
                    return spawn_points_guard[best_idx].position;
                 }
            }
            warn!("[RESPAWN_WARN] Critical fallback in respawn logic (top_n empty). Returning (0,0). Player: {:?}", player_id);
            return Vec2::new(0.0, 0.0);
        }

        let chosen_spawn_ref = top_n[rng.gen_range(0..top_n.len())];
        let spawn_idx = chosen_spawn_ref.0;

        if spawn_idx < spawn_points_guard.len() {
            spawn_points_guard[spawn_idx].last_used = now;
            return spawn_points_guard[spawn_idx].position;
        } else {
             warn!("[RESPAWN_WARN] Chosen spawn_idx {} is out of bounds (len: {}). Player: {:?}", spawn_idx, spawn_points_guard.len(), player_id);
            if let Some(&(first_valid_idx, _)) = scored_spawns.first() {
                 if first_valid_idx < spawn_points_guard.len() {
                    spawn_points_guard[first_valid_idx].last_used = now;
                    return spawn_points_guard[first_valid_idx].position;
                 }
            }
            warn!("[RESPAWN_WARN] Critical fallback in respawn logic. Returning (0,0). Player: {:?}", player_id);
            return Vec2::new(0.0, 0.0);
        }
    }

}

// --- WallRespawnManager ---

#[derive(Clone)]
pub(crate) struct DestroyedWallInfo {
    wall_data: Wall,
    #[allow(dead_code)]
    destroyed_at: Instant,
    #[allow(dead_code)]
    respawn_delay: Duration,
}

pub struct WallRespawnManager {
    destroyed_walls: Arc<DashMap<EntityId, DestroyedWallInfo>>,
    respawn_queue: Arc<RwLock<Vec<(EntityId, Instant)>>>,
    wall_templates: Arc<DashMap<EntityId, Wall>>,
}

impl WallRespawnManager {
    pub fn new() -> Self {
        Self {
            destroyed_walls: Arc::new(DashMap::new()),
            respawn_queue: Arc::new(RwLock::new(Vec::new())),
            wall_templates: Arc::new(DashMap::new()),
        }
    }

    pub fn is_wall_template_registered_for_test(&self, wall_id: EntityId) -> bool {
        self.wall_templates.contains_key(&wall_id)
    }

    pub fn get_destroyed_walls_count_for_test(&self) -> usize {
        self.destroyed_walls.len()
    }

    pub fn is_wall_id_in_destroyed_map_for_test(&self, wall_id: EntityId) -> bool {
        self.destroyed_walls.contains_key(&wall_id)
    }

    pub fn register_wall(&self, wall: &Wall) {
        if wall.is_destructible {
            self.wall_templates.insert(wall.id, wall.clone());
        }
    }

    pub fn register_all_walls(&self, walls: &[Wall]) {
        for wall in walls {
            self.register_wall(wall);
        }
    }

    pub fn wall_destroyed(&self, wall_id: EntityId) {
        if self.destroyed_walls.contains_key(&wall_id) {
            debug!("Wall ID {} already destroyed and scheduled for respawn, skipping.", wall_id);
            return;
        }

        if let Some(template_wall_entry) = self.wall_templates.get(&wall_id) {
            let template_wall = template_wall_entry.value();
            let respawn_delay_duration = match template_wall.max_health {
                h if h <= 100 => Duration::from_secs(30),
                h if h <= 200 => Duration::from_secs(60),
                _ => Duration::from_secs(90),
            };

            let info = DestroyedWallInfo {
                wall_data: template_wall.clone(),
                destroyed_at: Instant::now(),
                respawn_delay: respawn_delay_duration,
            };
            self.destroyed_walls.insert(wall_id, info);

            let scheduled_time = Instant::now() + respawn_delay_duration;
            let mut queue_guard = self.respawn_queue.write();
            queue_guard.push((wall_id, scheduled_time));
            queue_guard.sort_by_key(|k| k.1);
            debug!("Wall ID {} scheduled for respawn in {} seconds.", wall_id, respawn_delay_duration.as_secs());
        } else {
            warn!("Wall ID {} not found in templates, cannot schedule respawn.", wall_id);
        }
    }

    pub fn check_respawns(&self) -> Vec<Wall> {
        let mut ready_to_respawn_walls = Vec::new();
        let now = Instant::now();
        let mut queue_guard = self.respawn_queue.write();

        let i = 0;
        while i < queue_guard.len() {
            if now >= queue_guard[i].1 {
                let (wall_id, _scheduled_time) = queue_guard.remove(i);
                if let Some((_id, info)) = self.destroyed_walls.remove(&wall_id) {
                    ready_to_respawn_walls.push(info.wall_data.clone());
                } else {
                    warn!("Wall ID {} was due for respawn but not found in destroyed_walls map.", wall_id);
                }
            } else {
                break;
            }
        }
        ready_to_respawn_walls
    }

    pub fn get_wall_respawn_timer(&self, wall_id: EntityId) -> Option<Duration> {
        let now = Instant::now();
        if let Some(entry) = self.respawn_queue.read().iter().find(|(id, _)| *id == wall_id) {
            if entry.1 > now {
                return Some(entry.1.duration_since(now));
            } else {
                 if self.destroyed_walls.contains_key(&wall_id) {
                    warn!("Wall {} in respawn_queue but past its time and still in destroyed_walls.", wall_id);
                    return Some(Duration::ZERO);
                 }
                 return None;
            }
        }
        if self.destroyed_walls.contains_key(&wall_id) {
            warn!("Wall {} in destroyed_walls but not in respawn_queue. Inconsistent state.", wall_id);
        }
        None
    }
}