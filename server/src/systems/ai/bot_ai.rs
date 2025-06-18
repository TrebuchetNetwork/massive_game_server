// src/systems/ai/bot_ai.rs

use crate::core::types::{
    PlayerID, PlayerInputData, Vec2, PlayerState, CorePickupType, ServerWeaponType, Wall, EntityId,
    FIELD_POSITION_ROTATION,
};
use crate::core::constants::*;
use crate::server::instance::{BotController, BotBehaviorState, MassiveGameServer};
use crate::flatbuffers_generated::game_protocol as fb;
use crate::world::partition::WorldPartitionManager;

use std::sync::Arc;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};
use std::collections::{VecDeque, HashMap, HashSet};
use rand::Rng;
use tracing::{debug, info, trace, warn};

// Constants for Bot AI
const BOT_UPDATE_RATE: u64 = 2; // Update every 2 frames for more responsive AI
const BOT_DECISION_INTERVAL: Duration = Duration::from_millis(25); // Very fast decision making
const BOT_PATH_RECALCULATION_INTERVAL: Duration = Duration::from_millis(200); // More frequent path updates
const BOT_MELEE_RANGE: f32 = 50.0;
const BOT_MAX_NAVIGATION_TARGET_DISTANCE: f32 = 5000.0;

// Dynamic Movement Constants - Ultra Aggressive
const BOT_SPREAD_DISTANCE: f32 = 100.0; // Very close combat formations
const BOT_FLANK_DISTANCE: f32 = 200.0; // Tighter flanking maneuvers
const BOT_RETREAT_HEALTH: i32 = 5; // Fight until almost dead
const BOT_AGGRESSION_RANGE: f32 = 5000.0; // Extreme aggression range

// Tactical positions around the map
const TACTICAL_POSITIONS: [(f32, f32); 12] = [
    (-500.0, -500.0), (500.0, -500.0), // Corners
    (-500.0, 500.0), (500.0, 500.0),
    (0.0, -700.0), (0.0, 700.0), // North/South positions
    (-700.0, 0.0), (700.0, 0.0), // East/West positions
    (-350.0, -350.0), (350.0, -350.0), // Inner positions
    (-350.0, 350.0), (350.0, 350.0),
];

pub struct BotAISystem;

impl BotAISystem {
    pub fn update_bots(server_instance: &MassiveGameServer, _delta_time: f32) {
        let frame_count = server_instance.frame_counter.load(std::sync::atomic::Ordering::Relaxed);
        if frame_count % BOT_UPDATE_RATE != 0 {
            return;
        }

        let current_time_instant = Instant::now();
        let all_player_states_snapshot: HashMap<PlayerID, PlayerState> = {
            let mut entities = HashMap::new();
            server_instance.player_manager.for_each_player(|id, state| {
                entities.insert(id.clone(), state.clone());
            });
            entities
        };

        let active_pickups_snapshot: Vec<(EntityId, Vec2, CorePickupType)> = server_instance
            .pickups
            .read()
            .iter()
            .filter(|p| p.is_active)
            .map(|p| (p.id, Vec2::new(p.x, p.y), p.pickup_type.clone()))
            .collect();
        
        let bot_ids: Vec<PlayerID> = server_instance
            .bot_players
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        for bot_id_arc in bot_ids {
            let mut rng_thread = rand::thread_rng();
            let bot_current_state_owned_opt = all_player_states_snapshot.get(&bot_id_arc).cloned();

            if let Some(bot_current_state) = bot_current_state_owned_opt {
                if !bot_current_state.alive {
                    if let Some(mut bot_controller_entry) = server_instance.bot_players.get_mut(&bot_id_arc) {
                        let bot_controller = bot_controller_entry.value_mut();
                        if bot_controller.behavior_state != BotBehaviorState::Idle {
                            debug!("[Bot {} ({})]: Died. Resetting to Idle.", bot_current_state.username, bot_id_arc.as_str());
                        }
                        bot_controller.behavior_state = BotBehaviorState::Idle;
                        bot_controller.current_path.clear();
                        bot_controller.target_position = None;
                        bot_controller.target_enemy_id = None;
                    }
                    continue; 
                }

                if let Some(mut bot_controller_entry) = server_instance.bot_players.get_mut(&bot_id_arc) {
                    let bot_controller = bot_controller_entry.value_mut();

                    // Decision making
                    if current_time_instant.duration_since(bot_controller.last_decision_time) > BOT_DECISION_INTERVAL {
                        bot_controller.last_decision_time = current_time_instant;
                        Self::make_tactical_decision(
                            &bot_current_state, 
                            bot_controller,    
                            &all_player_states_snapshot,
                            &active_pickups_snapshot,
                            server_instance,
                            &mut rng_thread,
                        );
                    }

                    // Path recalculation
                    if bot_controller.target_position.is_some() &&
                       (bot_controller.current_path.is_empty() ||
                        current_time_instant.duration_since(bot_controller.path_recalculation_timer) > BOT_PATH_RECALCULATION_INTERVAL) {
                        if let Some(goal_pos) = bot_controller.target_position {
                            bot_controller.current_path = Self::calculate_warzone_path(
                                Vec2::new(bot_current_state.x, bot_current_state.y),
                                goal_pos,
                                &server_instance.world_partition_manager, 
                            );
                            bot_controller.path_recalculation_timer = current_time_instant;
                        }
                    }
                    
                    // Input generation
                    let mut input = PlayerInputData {
                        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_millis() as u64,
                        sequence: bot_current_state.last_processed_input_sequence.wrapping_add(1),
                        move_forward: false, move_backward: false, move_left: false, move_right: false,
                        shooting: false, reload: false, rotation: bot_current_state.rotation,
                        melee_attack: false, change_weapon_slot: 0, use_ability_slot: 0,
                    };

                    Self::generate_warzone_input(
                        &bot_current_state, 
                        bot_controller,    
                        &mut input,
                        &all_player_states_snapshot,
                        server_instance,
                        &mut rng_thread,
                    );
                    
                    if let Some(mut actual_bot_player_state_entry) = server_instance.player_manager.get_player_state_mut(&bot_id_arc) {
                        let actual_bot_player_state = &mut *actual_bot_player_state_entry;
                        actual_bot_player_state.queue_input(input);
                    }
                }
            } else {
                if server_instance.bot_players.remove(&bot_id_arc).is_some() {
                    trace!("[Bot {}]: Cleaned up controller as bot state was not in snapshot.", bot_id_arc.as_str());
                }
            }
        }
    }

    fn make_tactical_decision(
        bot_state: &PlayerState,
        bot_controller: &mut BotController,
        all_player_entities: &HashMap<PlayerID, PlayerState>,
        active_pickups: &[(EntityId, Vec2, CorePickupType)],
        server: &MassiveGameServer,
        rng: &mut impl Rng,
    ) {
        let bot_id_str = bot_state.id.as_str();
        let bot_pos = Vec2::new(bot_state.x, bot_state.y);
        
        // Check game mode for CTF logic
        let match_info = server.match_info.read();
        let is_ctf = match_info.game_mode == fb::GameModeType::CaptureTheFlag;
        let ctf_flag_states = match_info.flag_states.clone();
        drop(match_info);
        
        // CTF Priority Logic
        if is_ctf && bot_state.team_id != 0 {
            // If carrying enemy flag, rush to base!
            if bot_state.is_carrying_flag_team_id != 0 && bot_state.is_carrying_flag_team_id != bot_state.team_id {
                let home_base = MassiveGameServer::get_flag_base_position(bot_state.team_id);
                bot_controller.behavior_state = BotBehaviorState::MovingToObjective;
                bot_controller.target_position = Some(home_base);
                bot_controller.target_enemy_id = None;
                bot_controller.current_path.clear();
                debug!("[Bot {} ({})]: Carrying enemy flag! Rushing to base at ({:.1}, {:.1})",
                    bot_state.username, bot_id_str, home_base.x, home_base.y);
                return;
            }
            
            // Count bots on our team going for the flag
            let mut team_bots_going_for_flag = 0;
            let mut total_team_bots = 0;
            let enemy_team = if bot_state.team_id == 1 { 2 } else { 1 };
            let enemy_flag_pos = if let Some(enemy_flag_state) = ctf_flag_states.get(&enemy_team) {
                Some(enemy_flag_state.position)
            } else {
                None
            };
            
            // Count how many bots are going for the flag
            for entry in server.bot_players.iter() {
                let bot_id = entry.key();
                let bot_ctrl = entry.value();
                
                if let Some(bot_player_state) = all_player_entities.get(bot_id) {
                    if bot_player_state.team_id == bot_state.team_id && bot_player_state.alive {
                        total_team_bots += 1;
                        
                        // Check if this bot is going for the enemy flag
                        if let (Some(target_pos), Some(flag_pos)) = (bot_ctrl.target_position, enemy_flag_pos) {
                            let dist_to_flag = ((target_pos.x - flag_pos.x).powi(2) + (target_pos.y - flag_pos.y).powi(2)).sqrt();
                            if dist_to_flag < 100.0 && bot_ctrl.behavior_state == BotBehaviorState::MovingToObjective {
                                team_bots_going_for_flag += 1;
                            }
                        }
                    }
                }
            }
            
            // Calculate percentage of bots going for flag
            let flag_capture_percentage = if total_team_bots > 0 {
                team_bots_going_for_flag as f32 / total_team_bots as f32
            } else {
                0.0
            };
            
            // More aggressive flag capture - at least 40% of bots go for the flag
            let should_go_for_flag = flag_capture_percentage < 0.4 || rng.gen_bool(0.5);
            
            // Check if we should go for enemy flag
            if let Some(enemy_flag_state) = ctf_flag_states.get(&enemy_team) {
                if (enemy_flag_state.status == fb::FlagStatus::AtBase || enemy_flag_state.status == fb::FlagStatus::Dropped) 
                   && should_go_for_flag {
                    bot_controller.behavior_state = BotBehaviorState::MovingToObjective;
                    bot_controller.target_position = Some(enemy_flag_state.position);
                    bot_controller.target_enemy_id = None;
                    bot_controller.current_path.clear();
                    debug!("[Bot {} ({})]: Going for enemy flag at ({:.1}, {:.1}) (team flag capture rate: {:.1}%)",
                        bot_state.username, bot_id_str, enemy_flag_state.position.x, enemy_flag_state.position.y, 
                        flag_capture_percentage * 100.0);
                    return;
                }
            }
            
            // Check if we should defend our flag
            if let Some(our_flag_state) = ctf_flag_states.get(&bot_state.team_id) {
                if our_flag_state.status == fb::FlagStatus::Carried && rng.gen_bool(0.6) { // 60% chance to hunt flag carrier
                    // Find the enemy carrying our flag
                    for (player_id, player_state) in all_player_entities {
                        if player_state.is_carrying_flag_team_id == bot_state.team_id {
                            bot_controller.behavior_state = BotBehaviorState::Engaging;
                            bot_controller.target_enemy_id = Some(player_id.clone());
                            bot_controller.target_position = Some(Vec2::new(player_state.x, player_state.y));
                            bot_controller.current_path.clear();
                            debug!("[Bot {} ({})]: Hunting enemy flag carrier {} at ({:.1}, {:.1})",
                                bot_state.username, bot_id_str, player_state.username, player_state.x, player_state.y);
                            return;
                        }
                    }
                }
            }
        }
        
        // Check if we need health - but only if extremely low in aggressive mode
        let needs_health = bot_state.health < BOT_RETREAT_HEALTH;
        if needs_health {
            // Look for health pickups
            if let Some((pickup_pos, _)) = Self::find_nearest_health_pickup(bot_state, active_pickups) {
                bot_controller.behavior_state = BotBehaviorState::SeekingPickup;
                bot_controller.target_position = Some(pickup_pos);
                bot_controller.target_enemy_id = None;
                bot_controller.current_path.clear();
                
                debug!("[Bot {} ({})]: Low health ({}), seeking health pickup at ({:.1}, {:.1})",
                    bot_state.username, bot_id_str, bot_state.health, pickup_pos.x, pickup_pos.y);
                return;
            }
        }
        
        // Find enemies and allies - increased scan radius
        let scan_radius = 2500.0;
        let nearby_player_ids = server.spatial_index.query_nearby_players(
            bot_state.x, bot_state.y, scan_radius,
        );
        
        let mut enemies: Vec<(&PlayerState, f32)> = Vec::new();
        let mut allies: Vec<&PlayerState> = Vec::new();
        
        for entity_id_arc in nearby_player_ids {
            if let Some(other_state) = all_player_entities.get(&entity_id_arc) {
                if entity_id_arc == bot_state.id || !other_state.alive {
                    continue;
                }
                
                let is_enemy = if bot_state.team_id == 0 {
                    true // FFA mode - everyone is enemy
                } else {
                    other_state.team_id != bot_state.team_id && other_state.team_id != 0
                };
                
                if is_enemy {
                    let dist = ((other_state.x - bot_state.x).powi(2) + (other_state.y - bot_state.y).powi(2)).sqrt();
                    enemies.push((other_state, dist));
                } else {
                    allies.push(other_state);
                }
            }
        }
        
        // Sort enemies by distance
        enemies.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        
        // Decision logic based on situation
        if let Some((closest_enemy, dist)) = enemies.first() {
            let weapon_range = Self::get_weapon_effective_range(bot_state.weapon);
            
            // Check if we're too close to allies
            let too_close_to_allies = allies.iter().any(|ally| {
                let ally_dist = ((ally.x - bot_state.x).powi(2) + (ally.y - bot_state.y).powi(2)).sqrt();
                ally_dist < BOT_SPREAD_DISTANCE
            });
            
            if *dist < weapon_range && !needs_health {
                // Enemy in range - engage
                bot_controller.behavior_state = BotBehaviorState::Engaging;
                bot_controller.target_enemy_id = Some(closest_enemy.id.clone());
                bot_controller.target_position = Some(Vec2::new(closest_enemy.x, closest_enemy.y));
                bot_controller.current_path.clear();
                
                debug!("[Bot {} ({})]: Engaging {} at distance {:.1}",
                    bot_state.username, bot_id_str, closest_enemy.username, dist);
            } else if *dist < BOT_AGGRESSION_RANGE && !needs_health {
                // Enemy nearby - attempt flanking maneuver
                let flank_pos = Self::calculate_flank_position(bot_pos, Vec2::new(closest_enemy.x, closest_enemy.y), rng);
                
                bot_controller.behavior_state = BotBehaviorState::Flanking;
                bot_controller.target_enemy_id = Some(closest_enemy.id.clone());
                bot_controller.target_position = Some(flank_pos);
                bot_controller.current_path.clear();
                
                debug!("[Bot {} ({})]: Flanking {} via ({:.1}, {:.1})",
                    bot_state.username, bot_id_str, closest_enemy.username, flank_pos.x, flank_pos.y);
            } else if too_close_to_allies {
                // Spread out from allies
                let spread_pos = Self::find_spread_position(bot_pos, &allies, &all_player_entities, rng);
                
                bot_controller.behavior_state = BotBehaviorState::MovingToPosition;
                bot_controller.target_position = Some(spread_pos);
                bot_controller.target_enemy_id = None;
                bot_controller.current_path.clear();
                
                debug!("[Bot {} ({})]: Spreading out to ({:.1}, {:.1})",
                    bot_state.username, bot_id_str, spread_pos.x, spread_pos.y);
            } else {
                // Patrol to tactical position
                let patrol_pos = Self::choose_tactical_position(bot_pos, &enemies, rng);
                
                bot_controller.behavior_state = BotBehaviorState::Patrolling;
                bot_controller.target_position = Some(patrol_pos);
                bot_controller.target_enemy_id = None;
                bot_controller.current_path.clear();
                
                debug!("[Bot {} ({})]: Patrolling to ({:.1}, {:.1})",
                    bot_state.username, bot_id_str, patrol_pos.x, patrol_pos.y);
            }
        } else {
            // No enemies - patrol or spread out
            if allies.iter().any(|ally| {
                let ally_dist = ((ally.x - bot_state.x).powi(2) + (ally.y - bot_state.y).powi(2)).sqrt();
                ally_dist < BOT_SPREAD_DISTANCE
            }) {
                let spread_pos = Self::find_spread_position(bot_pos, &allies, &all_player_entities, rng);
                bot_controller.behavior_state = BotBehaviorState::MovingToPosition;
                bot_controller.target_position = Some(spread_pos);
                bot_controller.target_enemy_id = None;
                bot_controller.current_path.clear();
            } else {
                let patrol_pos = TACTICAL_POSITIONS[rng.gen_range(0..TACTICAL_POSITIONS.len())];
                bot_controller.behavior_state = BotBehaviorState::Patrolling;
                bot_controller.target_position = Some(Vec2::new(patrol_pos.0, patrol_pos.1));
                bot_controller.target_enemy_id = None;
                bot_controller.current_path.clear();
            }
        }
    }

    fn find_nearest_health_pickup(
        bot_state: &PlayerState,
        pickups: &[(EntityId, Vec2, CorePickupType)],
    ) -> Option<(Vec2, f32)> {
        let mut best_pickup: Option<(Vec2, f32)> = None;
        let mut closest_dist_sq = f32::MAX;

        for (_id, pos, pickup_type) in pickups {
            if matches!(pickup_type, CorePickupType::Health | CorePickupType::Shield) {
                let dist_sq = (pos.x - bot_state.x).powi(2) + (pos.y - bot_state.y).powi(2);
                if dist_sq < closest_dist_sq {
                    closest_dist_sq = dist_sq;
                    best_pickup = Some((*pos, dist_sq.sqrt()));
                }
            }
        }
        best_pickup
    }

    fn calculate_flank_position(bot_pos: Vec2, enemy_pos: Vec2, rng: &mut impl Rng) -> Vec2 {
        let angle_to_enemy = (enemy_pos.y - bot_pos.y).atan2(enemy_pos.x - bot_pos.x);
        let flank_angle = if rng.gen_bool(0.5) {
            angle_to_enemy + std::f32::consts::FRAC_PI_2 // Right flank
        } else {
            angle_to_enemy - std::f32::consts::FRAC_PI_2 // Left flank
        };
        
        let flank_x = enemy_pos.x + BOT_FLANK_DISTANCE * flank_angle.cos();
        let flank_y = enemy_pos.y + BOT_FLANK_DISTANCE * flank_angle.sin();
        
        Vec2::new(
            flank_x.clamp(WORLD_MIN_X + 100.0, WORLD_MAX_X - 100.0),
            flank_y.clamp(WORLD_MIN_Y + 100.0, WORLD_MAX_Y - 100.0),
        )
    }

    fn find_spread_position(
        bot_pos: Vec2, 
        allies: &[&PlayerState],
        all_players: &HashMap<PlayerID, PlayerState>,
        rng: &mut impl Rng
    ) -> Vec2 {
        // Calculate average ally position
        let mut avg_x = 0.0;
        let mut avg_y = 0.0;
        for ally in allies {
            avg_x += ally.x;
            avg_y += ally.y;
        }
        avg_x /= allies.len() as f32;
        avg_y /= allies.len() as f32;
        
        // Move away from average ally position
        let angle_from_center = (bot_pos.y - avg_y).atan2(bot_pos.x - avg_x);
        let spread_angle = angle_from_center + rng.gen_range(-0.5..0.5);
        
        let spread_x = bot_pos.x + BOT_SPREAD_DISTANCE * spread_angle.cos();
        let spread_y = bot_pos.y + BOT_SPREAD_DISTANCE * spread_angle.sin();
        
        Vec2::new(
            spread_x.clamp(WORLD_MIN_X + 100.0, WORLD_MAX_X - 100.0),
            spread_y.clamp(WORLD_MIN_Y + 100.0, WORLD_MAX_Y - 100.0),
        )
    }

    fn choose_tactical_position(bot_pos: Vec2, enemies: &[(&PlayerState, f32)], rng: &mut impl Rng) -> Vec2 {
        // If enemies exist, choose position that provides good angle
        if !enemies.is_empty() {
            let enemy_center = enemies.iter()
                .fold((0.0, 0.0), |acc, (e, _)| (acc.0 + e.x, acc.1 + e.y));
            let enemy_center = (enemy_center.0 / enemies.len() as f32, enemy_center.1 / enemies.len() as f32);
            
            // Find tactical position with best angle to enemy center
            let mut best_pos = TACTICAL_POSITIONS[0];
            let mut best_score = f32::MIN;
            
            for &pos in &TACTICAL_POSITIONS {
                let dist_to_pos = ((pos.0 - bot_pos.x).powi(2) + (pos.1 - bot_pos.y).powi(2)).sqrt();
                let dist_to_enemy = ((pos.0 - enemy_center.0).powi(2) + (pos.1 - enemy_center.1).powi(2)).sqrt();
                
                // Prefer positions that are close to us but give good angle on enemies
                let score = 1000.0 / (dist_to_pos + 100.0) + 500.0 / (dist_to_enemy + 100.0);
                
                if score > best_score {
                    best_score = score;
                    best_pos = pos;
                }
            }
            
            Vec2::new(best_pos.0, best_pos.1)
        } else {
            // Random tactical position
            let pos = TACTICAL_POSITIONS[rng.gen_range(0..TACTICAL_POSITIONS.len())];
            Vec2::new(pos.0, pos.1)
        }
    }
    
    fn calculate_warzone_path(
        start: Vec2,
        goal: Vec2,
        world_partition_manager: &Arc<WorldPartitionManager>,
    ) -> VecDeque<Vec2> {
        let mut path = VecDeque::new();
        // INCREASED segment length for fewer waypoints and faster movement
        const MAX_PATH_SEGMENT_LENGTH: f32 = 800.0; 

        if ((start.x - goal.x).powi(2) + (start.y - goal.y).powi(2)).sqrt() < 50.0 {
            return path;
        }
        
        if !Self::is_path_obstructed(start, goal, world_partition_manager) {
            // Direct path - just go straight there
            path.push_back(goal);
            return path;
        }

        // Simple detour if direct path blocked
        let mut rng = rand::thread_rng();
        for i in 0..3 { // Fewer attempts for speed
            let detour_angle = if i == 0 {
                std::f32::consts::FRAC_PI_4 // 45 degrees
            } else if i == 1 {
                -std::f32::consts::FRAC_PI_4 // -45 degrees  
            } else {
                rng.gen_range(-std::f32::consts::FRAC_PI_2..std::f32::consts::FRAC_PI_2)
            };
            
            let dist_to_goal = ((goal.x - start.x).powi(2) + (goal.y - start.y).powi(2)).sqrt();
            let angle_to_goal = (goal.y - start.y).atan2(goal.x - start.x);

            let detour_x = start.x + (dist_to_goal * 0.6) * (angle_to_goal + detour_angle).cos();
            let detour_y = start.y + (dist_to_goal * 0.6) * (angle_to_goal + detour_angle).sin();
            
            let detour_point = Vec2::new(
                detour_x.clamp(WORLD_MIN_X + 50.0, WORLD_MAX_X - 50.0),
                detour_y.clamp(WORLD_MIN_Y + 50.0, WORLD_MAX_Y - 50.0),
            );

            if !Self::is_path_obstructed(start, detour_point, world_partition_manager) &&
               !Self::is_path_obstructed(detour_point, goal, world_partition_manager) {
                path.push_back(detour_point);
                path.push_back(goal);
                return path;
            }
        }
        
        // Just go direct if no detour found
        path.push_back(goal);
        path
    }

    fn generate_warzone_input(
        bot_state: &PlayerState, 
        bot_controller: &mut BotController, 
        input: &mut PlayerInputData,
        all_player_entities: &HashMap<PlayerID, PlayerState>,
        server_instance: &MassiveGameServer,
        rng: &mut impl Rng,
    ) {
        let bot_id_str = bot_state.id.as_str();
        
        // Movement handling - ALWAYS try to move
        let mut current_movement_target: Option<Vec2> = None;
        if !bot_controller.current_path.is_empty() {
            current_movement_target = bot_controller.current_path.front().cloned();
        } else if bot_controller.target_position.is_some() {
            current_movement_target = bot_controller.target_position;
        } else {
            // No target? Pick a random direction to move
            let random_angle = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
            let random_dist = 200.0;
            let random_target = Vec2::new(
                (bot_state.x + random_dist * random_angle.cos()).clamp(WORLD_MIN_X + 100.0, WORLD_MAX_X - 100.0),
                (bot_state.y + random_dist * random_angle.sin()).clamp(WORLD_MIN_Y + 100.0, WORLD_MAX_Y - 100.0),
            );
            current_movement_target = Some(random_target);
        }
        
        if let Some(target_pos) = current_movement_target {
            let dx = target_pos.x - bot_state.x;
            let dy = target_pos.y - bot_state.y;
            let dist_sq = dx * dx + dy * dy;
            
            // REDUCED threshold for faster movement - was 50.0
            let waypoint_threshold_sq = (100.0_f32).powi(2);

            if dist_sq > waypoint_threshold_sq {
                input.rotation = dy.atan2(dx);
                input.move_forward = true;
                
                // ALWAYS keep moving forward for speed
                // Add diagonal movement for extra speed
                let angle_diff = ((input.rotation - bot_state.rotation).abs() % (2.0 * std::f32::consts::PI)).min(
                    2.0 * std::f32::consts::PI - (input.rotation - bot_state.rotation).abs() % (2.0 * std::f32::consts::PI)
                );
                
                // Constant strafing movement
                if rng.gen_bool(0.8) { // Very high chance to strafe
                    if dx > 0.0 {
                        input.move_right = true;
                        input.move_left = false;
                    } else {
                        input.move_left = true;
                        input.move_right = false;
                    }
                }
                
                // Dynamic movement based on behavior
                match bot_controller.behavior_state {
                    BotBehaviorState::Flanking => {
                        // Aggressive zigzag when flanking
                        if rng.gen_bool(0.4) {
                            if rng.gen_bool(0.5) { 
                                input.move_left = true; 
                            } else { 
                                input.move_right = true; 
                            }
                        }
                    },
                    BotBehaviorState::SeekingPickup => {
                        // Sprint to pickup - diagonal movement for speed
                        if rng.gen_bool(0.8) {
                            input.move_left = rng.gen_bool(0.5);
                            input.move_right = !input.move_left;
                        }
                    },
                    BotBehaviorState::Patrolling => {
                        // Fast patrol movement
                        if rng.gen_bool(0.5) {
                            if rng.gen_bool(0.5) { 
                                input.move_left = true; 
                            } else { 
                                input.move_right = true; 
                            }
                        }
                    },
                    _ => {
                        // Default fast movement
                        if rng.gen_bool(0.6) {
                            input.move_left = rng.gen_bool(0.5);
                            input.move_right = !input.move_left;
                        }
                    }
                }
            } else {
                // Constant strafing movement around target
                input.move_forward = true;
                if dx > 0.0 {
                    input.move_right = true;
                    input.move_left = false;
                } else {
                    input.move_left = true;
                    input.move_right = false;
                }
                
                if !bot_controller.current_path.is_empty() {
                    bot_controller.current_path.pop_front();
                }
            }
        }

        // Combat behavior
        if bot_controller.behavior_state == BotBehaviorState::Engaging || 
           bot_controller.behavior_state == BotBehaviorState::Flanking {
            if let Some(enemy_id_arc) = &bot_controller.target_enemy_id {
                if let Some(enemy_state) = all_player_entities.get(enemy_id_arc) {
                    if enemy_state.alive {
                        let enemy_dx = enemy_state.x - bot_state.x;
                        let enemy_dy = enemy_state.y - bot_state.y;
                        let dist_to_enemy = (enemy_dx * enemy_dx + enemy_dy * enemy_dy).sqrt();
                        
                        // Aim prediction
                        let projectile_speed = Self::get_projectile_speed(bot_state.weapon).max(1.0);
                        let time_to_target = dist_to_enemy / projectile_speed;
                        
                        let predicted_x = enemy_state.x + enemy_state.velocity_x * time_to_target * 0.8;
                        let predicted_y = enemy_state.y + enemy_state.velocity_y * time_to_target * 0.8;
                        
                        input.rotation = (predicted_y - bot_state.y).atan2(predicted_x - bot_state.x);

                        let weapon_range = Self::get_weapon_effective_range(bot_state.weapon);
                        let is_in_range = dist_to_enemy < weapon_range;
                        
                        let has_los = Self::has_line_of_sight(
                            Vec2::new(bot_state.x, bot_state.y), 
                            Vec2::new(enemy_state.x, enemy_state.y), 
                            &server_instance.world_partition_manager
                        );

                        if has_los && is_in_range {
                            if bot_state.weapon == ServerWeaponType::Melee {
                                if dist_to_enemy < BOT_MELEE_RANGE {
                                    input.melee_attack = true;
                                }
                            } else {
                                // Aggressive shooting
                                if rng.gen_bool(0.95) {  // More aggressive shooting
                                    input.shooting = true;
                                }
                            }
                            
                            // More aggressive and direct movement during combat
                            if rng.gen_bool(0.9) {  // Higher chance to strafe
                                input.move_left = rng.gen_bool(0.5);
                                input.move_right = !input.move_left;
                            }
                            
                            // Keep moving forward if not too close
                            if dist_to_enemy > 150.0 {
                                input.move_forward = true;
                            } else if dist_to_enemy < 100.0 && rng.gen_bool(0.5) {
                                // Back up if too close
                                input.move_backward = true;
                                input.move_forward = false;
                            }
                        } else if has_los {
                            // Close distance very aggressively
                            input.move_forward = true;
                            // Always zigzag when approaching
                            if rng.gen_bool(0.6) {
                                if rng.gen_bool(0.5) {
                                    input.move_left = true;
                                } else {
                                    input.move_right = true;
                                }
                            }
                        } else {
                            // No LOS - move to last known position fast
                            input.move_forward = true;
                            // Add strafe for speed
                            if rng.gen_bool(0.7) {
                                input.move_left = rng.gen_bool(0.5);
                                input.move_right = !input.move_left;
                            }
                        }
                    } else {
                        bot_controller.target_enemy_id = None;
                    }
                }
            }
        }

        // Reload management
        if bot_state.weapon != ServerWeaponType::Melee {
            if bot_state.ammo == 0 && bot_state.reload_progress.is_none() {
                input.reload = true;
            } else if bot_state.ammo < PlayerState::get_max_ammo_for_weapon(bot_state.weapon) / 3 && 
                      bot_controller.behavior_state != BotBehaviorState::Engaging && 
                      bot_state.reload_progress.is_none() && 
                      rng.gen_bool(0.4) { 
                input.reload = true;
            }
        }
        
        // Dynamic weapon switching
        if bot_controller.behavior_state != BotBehaviorState::Engaging && rng.gen_bool(0.03) {
            // Choose weapon based on situation
            let new_weapon = if bot_controller.behavior_state == BotBehaviorState::Flanking {
                // Prefer close range for flanking
                if rng.gen_bool(0.6) { ServerWeaponType::Shotgun } else { ServerWeaponType::Rifle }
            } else {
                // General weapon choice
                match rng.gen_range(0..4) {
                    0 => ServerWeaponType::Pistol,
                    1 => ServerWeaponType::Shotgun,
                    2 => ServerWeaponType::Rifle,
                    _ => ServerWeaponType::Sniper,
                }
            };
            
            if new_weapon != bot_state.weapon {
                input.change_weapon_slot = match new_weapon {
                    ServerWeaponType::Pistol => 1,
                    ServerWeaponType::Shotgun => 2,
                    ServerWeaponType::Rifle => 3,
                    ServerWeaponType::Sniper => 4,
                    ServerWeaponType::Melee => 5,
                };
            }
        }
    }

    fn get_weapon_effective_range(weapon: ServerWeaponType) -> f32 {
        match weapon {
            ServerWeaponType::Pistol => 500.0,
            ServerWeaponType::Shotgun => 250.0,
            ServerWeaponType::Rifle => 700.0,
            ServerWeaponType::Sniper => 1200.0,
            ServerWeaponType::Melee => BOT_MELEE_RANGE,
        }
    }

    fn get_projectile_speed(weapon: ServerWeaponType) -> f32 {
        match weapon {
            ServerWeaponType::Pistol => PISTOL_PROJECTILE_SPEED,
            ServerWeaponType::Shotgun => SHOTGUN_PROJECTILE_SPEED,
            ServerWeaponType::Rifle => RIFLE_PROJECTILE_SPEED,
            ServerWeaponType::Sniper => SNIPER_PROJECTILE_SPEED,
            ServerWeaponType::Melee => 0.0,
        }
    }
    
    fn has_line_of_sight(
        start: Vec2,
        end: Vec2,
        world_partition_manager: &Arc<WorldPartitionManager>,
    ) -> bool {
        !Self::is_path_obstructed(start, end, world_partition_manager)
    }

    fn is_path_obstructed(
        start: Vec2,
        end: Vec2,
        world_partition_manager: &Arc<WorldPartitionManager>,
    ) -> bool {
        let mut relevant_partition_indices = HashSet::new();
        let steps = 10;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let point_on_line = Vec2::new(start.x * (1.0 - t) + end.x * t, start.y * (1.0 - t) + end.y * t);
            relevant_partition_indices.insert(world_partition_manager.get_partition_index_for_point(point_on_line.x, point_on_line.y));
        }

        for partition_idx in relevant_partition_indices {
            if let Some(partition) = world_partition_manager.get_partition(partition_idx) {
                for wall_entry in partition.all_walls_in_partition.iter() {
                    let wall = wall_entry.value();
                    if (!wall.is_destructible || wall.current_health > 0) &&
                       Self::line_intersects_rect(start, end, wall) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn line_intersects_rect(p1: Vec2, p2: Vec2, rect_wall: &Wall) -> bool {
        let rect_x = rect_wall.x;
        let rect_y = rect_wall.y;
        let rect_w = rect_wall.width;
        let rect_h = rect_wall.height;

        Self::line_intersects_line(p1, p2, Vec2::new(rect_x, rect_y), Vec2::new(rect_x + rect_w, rect_y)) ||
        Self::line_intersects_line(p1, p2, Vec2::new(rect_x + rect_w, rect_y), Vec2::new(rect_x + rect_w, rect_y + rect_h)) ||
        Self::line_intersects_line(p1, p2, Vec2::new(rect_x, rect_y + rect_h), Vec2::new(rect_x + rect_w, rect_y + rect_h)) ||
        Self::line_intersects_line(p1, p2, Vec2::new(rect_x, rect_y), Vec2::new(rect_x, rect_y + rect_h))
    }

    fn line_intersects_line(l1p1: Vec2, l1p2: Vec2, l2p1: Vec2, l2p2: Vec2) -> bool {
        let den = (l1p1.x - l1p2.x) * (l2p1.y - l2p2.y) - (l1p1.y - l1p2.y) * (l2p1.x - l2p2.x);
        if den.abs() < 0.0001 {
            return false;
        }
        let t = ((l1p1.x - l2p1.x) * (l2p1.y - l2p2.y) - (l1p1.y - l2p1.y) * (l2p1.x - l2p2.x)) / den;
        let u = -((l1p1.x - l1p2.x) * (l1p1.y - l2p1.y) - (l1p1.y - l1p2.y) * (l1p1.x - l2p1.x)) / den;
        
        t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0
    }
}
