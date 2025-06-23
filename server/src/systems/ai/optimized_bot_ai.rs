// Optimized Bot AI with CTF Support

use crate::core::types::{PlayerID, PlayerInputData, Vec2, PlayerState, ServerWeaponType};
use crate::core::constants::*;
use crate::server::instance::{BotController, BotBehaviorState, MassiveGameServer};
use crate::flatbuffers_generated::game_protocol as fb;

use std::sync::Arc;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use rand::Rng;
use tracing::{trace, warn, debug};

// Optimized constants
const BOT_UPDATE_BATCH_SIZE: usize = 50; // Process all bots every frame
const BOT_SIMPLE_MOVEMENT_ONLY: bool = false; // Enable full AI with combat
const BOT_MOVEMENT_CHANGE_INTERVAL: Duration = Duration::from_millis(2000); // Less frequent decision changes
const BOT_TARGET_ACQUISITION_RANGE: f32 = 600.0; // Increased combat range
const BOT_FLAG_DETECTION_RANGE: f32 = 2000.0; // See flags from far away
const BOT_SHOOT_ACCURACY: f32 = 0.80; // 80% accuracy
const BOT_REACTION_TIME: Duration = Duration::from_millis(100); // Very fast reactions
const BOT_FLAG_CHASE_PRIORITY: f32 = 3.0; // High priority for flag objectives
const BOT_MOVEMENT_TOLERANCE: f32 = 50.0; // Distance to consider "at target"
const BOT_STUCK_THRESHOLD: f32 = 10.0; // Min distance to move to not be considered stuck
const BOT_STUCK_TIME_THRESHOLD: f32 = 2.0; // Seconds before considering bot stuck
const BOT_STUCK_CHECK_INTERVAL: f32 = 0.5; // Check every half second

#[derive(Debug, Clone)]
enum BotObjective {
    AttackEnemyFlag,      // Go get the enemy flag
    DefendOwnFlag,        // Stay near own flag base
    ChaseEnemyCarrier,    // Chase enemy who has our flag
    ProtectFriendlyCarrier, // Protect teammate with enemy flag
    PatrolMidfield,       // General patrol
    EngageNearbyEnemy,    // Fight nearby enemy
}

pub struct OptimizedBotAI;

impl OptimizedBotAI {
    /// Process ALL bots every frame for consistent movement
    pub fn update_bots_batch(server_instance: &MassiveGameServer, delta_time: f32) {
        let frame_count = server_instance.frame_counter.load(std::sync::atomic::Ordering::Relaxed);
        let current_time = Instant::now();
        
        // Get list of bot IDs
        let bot_ids: Vec<PlayerID> = server_instance
            .bot_players
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        
        if bot_ids.is_empty() {
            return;
        }
        
        trace!("Frame {}: Processing {} bots", frame_count, bot_ids.len());
        
        // Get current match info
        let match_info_guard = server_instance.match_info.read();
        let game_mode = match_info_guard.game_mode;
        let match_state = match_info_guard.match_state;
        let flag_states = match_info_guard.flag_states.clone();
        drop(match_info_guard);
        
        // Process ALL bots every frame
        for bot_id in &bot_ids {
            // Get bot state
            let bot_state_opt = server_instance.player_manager
                .get_player_state(bot_id)
                .map(|guard| (*guard).clone());
            
            if let Some(bot_state) = bot_state_opt {
                if !bot_state.alive {
                    continue;
                }
                
                // Update bot controller
                if let Some(mut bot_controller_entry) = server_instance.bot_players.get_mut(bot_id) {
                    let bot_controller = bot_controller_entry.value_mut();
                    
                    // Only make new decisions at intervals, but always generate movement
                    if current_time.duration_since(bot_controller.last_decision_time) > BOT_MOVEMENT_CHANGE_INTERVAL {
                        bot_controller.last_decision_time = current_time;
                        
                        if game_mode == fb::GameModeType::CaptureTheFlag && match_state == fb::MatchStateType::Active {
                            Self::make_ctf_decision(bot_controller, &bot_state, &flag_states, server_instance);
                        } else {
                            Self::make_simple_movement_decision(bot_controller, &bot_state);
                        }
                        
                        debug!("Bot {} made new decision: {:?} targeting {:?}", 
                            bot_state.username, bot_controller.behavior_state, bot_controller.target_position);
                    }
                    
                    // Check if bot is stuck before generating input
                    Self::check_stuck_status(bot_controller, &bot_state, delta_time);
                    
                    // Always generate input based on current objective
                    let input = Self::generate_combat_input(&bot_state, bot_controller, server_instance, game_mode);
                    
                    // Queue the input
                    if let Some(mut player_state_entry) = server_instance.player_manager.get_player_state_mut(bot_id) {
                        if input.move_forward || input.move_backward || input.move_left || input.move_right || input.shooting {
                            trace!("Bot {} input - forward:{} back:{} left:{} right:{} rot:{:.2} shoot:{}", 
                                bot_state.username, input.move_forward, input.move_backward, 
                                input.move_left, input.move_right, input.rotation, input.shooting);
                        }
                        player_state_entry.queue_input(input);
                    }
                }
            }
        }
    }
    
    /// Make CTF-specific decisions
    fn make_ctf_decision(
        bot_controller: &mut BotController,
        bot_state: &PlayerState,
        flag_states: &HashMap<u8, crate::server::instance::ServerFlagState>,
        server_instance: &MassiveGameServer,
    ) {
        let mut rng = rand::thread_rng();
        let bot_team = bot_state.team_id;
        let enemy_team = if bot_team == 1 { 2 } else { 1 };
        
        // Determine objective based on game state
        let objective = Self::determine_ctf_objective(bot_state, flag_states, server_instance);
        
        debug!("Bot {} (Team {}) objective: {:?}", bot_state.username, bot_team, objective);
        
        match objective {
            BotObjective::AttackEnemyFlag => {
                // If carrying flag, go to own base. Otherwise attack enemy flag
                if bot_state.is_carrying_flag_team_id != 0 {
                    // Bot has enemy flag, return to own base
                    let own_base = MassiveGameServer::get_flag_base_position(bot_team);
                    bot_controller.target_position = Some(own_base);
                    bot_controller.behavior_state = BotBehaviorState::MovingToObjective;
                    debug!("Bot {} carrying flag, returning to base at {:?}", bot_state.username, own_base);
                } else if let Some(enemy_flag) = flag_states.get(&enemy_team) {
                    // Go get enemy flag
                    bot_controller.target_position = Some(enemy_flag.position);
                    bot_controller.behavior_state = BotBehaviorState::MovingToObjective;
                    debug!("Bot {} going for enemy flag at {:?}", bot_state.username, enemy_flag.position);
                }
            }
            BotObjective::DefendOwnFlag => {
                // Stay near own flag base with some variation
                let base_pos = MassiveGameServer::get_flag_base_position(bot_team);
                let defend_radius = 150.0;
                let angle = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                let distance = rng.gen_range(50.0..defend_radius);
                bot_controller.target_position = Some(Vec2::new(
                    base_pos.x + distance * angle.cos(),
                    base_pos.y + distance * angle.sin()
                ));
                bot_controller.behavior_state = BotBehaviorState::Defending;
            }
            BotObjective::ChaseEnemyCarrier => {
                // Find and chase the enemy carrying our flag
                if let Some(own_flag) = flag_states.get(&bot_team) {
                    if let Some(carrier_id) = &own_flag.carrier_id {
                        if let Some(carrier_state) = server_instance.player_manager.get_player_state(carrier_id) {
                            bot_controller.target_position = Some(Vec2::new(carrier_state.x, carrier_state.y));
                            bot_controller.target_enemy_id = Some(carrier_id.clone());
                            bot_controller.behavior_state = BotBehaviorState::Engaging;
                        }
                    }
                }
            }
            BotObjective::ProtectFriendlyCarrier => {
                // Find and protect teammate carrying enemy flag
                if let Some(enemy_flag) = flag_states.get(&enemy_team) {
                    if let Some(carrier_id) = &enemy_flag.carrier_id {
                        if let Some(carrier_state) = server_instance.player_manager.get_player_state(carrier_id) {
                            // Move near the carrier but not too close
                            let offset_angle = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                            let offset_dist = 100.0;
                            bot_controller.target_position = Some(Vec2::new(
                                carrier_state.x + offset_dist * offset_angle.cos(),
                                carrier_state.y + offset_dist * offset_angle.sin()
                            ));
                            bot_controller.behavior_state = BotBehaviorState::Defending;
                        }
                    }
                }
            }
            BotObjective::PatrolMidfield => {
                // Patrol center area
                let patrol_x = rng.gen_range(-400.0..400.0);
                let patrol_y = rng.gen_range(-400.0..400.0);
                bot_controller.target_position = Some(Vec2::new(patrol_x, patrol_y));
                bot_controller.behavior_state = BotBehaviorState::Patrolling;
            }
            BotObjective::EngageNearbyEnemy => {
                // Find nearest enemy
                if let Some((enemy_pos, enemy_id)) = Self::find_nearest_enemy(bot_state, server_instance) {
                    bot_controller.target_position = Some(enemy_pos);
                    bot_controller.target_enemy_id = Some(enemy_id);
                    bot_controller.behavior_state = BotBehaviorState::Engaging;
                }
            }
        }
    }
    
    /// Determine the best objective for a bot in CTF mode
    fn determine_ctf_objective(
        bot_state: &PlayerState,
        flag_states: &HashMap<u8, crate::server::instance::ServerFlagState>,
        server_instance: &MassiveGameServer,
    ) -> BotObjective {
        let bot_team = bot_state.team_id;
        let enemy_team = if bot_team == 1 { 2 } else { 1 };
        
        // Check if bot is carrying flag - HIGHEST PRIORITY
        if bot_state.is_carrying_flag_team_id != 0 {
            // Bot has flag, should return to base
            return BotObjective::AttackEnemyFlag; // Will navigate to own base
        }
        
        // Check flag states
        let own_flag = flag_states.get(&bot_team);
        let enemy_flag = flag_states.get(&enemy_team);
        
        // Priority 1: Chase enemy who has our flag - VERY IMPORTANT
        if let Some(own_flag_state) = own_flag {
            if own_flag_state.status == fb::FlagStatus::Carried {
                if let Some(carrier_id) = &own_flag_state.carrier_id {
                    if let Some(carrier_state) = server_instance.player_manager.get_player_state(carrier_id) {
                        // Always chase if our flag is taken
                        return BotObjective::ChaseEnemyCarrier;
                    }
                }
            }
        }
        
        // Priority 2: Protect friendly flag carrier
        if let Some(enemy_flag_state) = enemy_flag {
            if enemy_flag_state.status == fb::FlagStatus::Carried {
                if let Some(carrier_id) = &enemy_flag_state.carrier_id {
                    if let Some(carrier_state) = server_instance.player_manager.get_player_state(carrier_id) {
                        if carrier_state.team_id == bot_team {
                            // Always protect our flag carrier
                            return BotObjective::ProtectFriendlyCarrier;
                        }
                    }
                }
            }
        }
        
        // Count teammates near each objective
        let mut defenders_at_base = 0;
        let mut attackers_going_for_flag = 0;
        
        server_instance.player_manager.for_each_player(|_, player| {
            if player.team_id == bot_team && player.alive {
                let own_base = MassiveGameServer::get_flag_base_position(bot_team);
                let dist_to_base = ((player.x - own_base.x).powi(2) + 
                                   (player.y - own_base.y).powi(2)).sqrt();
                if dist_to_base < 200.0 {
                    defenders_at_base += 1;
                }
                
                if let Some(enemy_flag_state) = enemy_flag {
                    let dist_to_enemy_flag = ((player.x - enemy_flag_state.position.x).powi(2) + 
                                             (player.y - enemy_flag_state.position.y).powi(2)).sqrt();
                    if dist_to_enemy_flag < 300.0 {
                        attackers_going_for_flag += 1;
                    }
                }
            }
        });
        
        // More aggressive role distribution
        let mut rng = rand::thread_rng();
        let role_choice = rng.gen_range(0..100);
        
        // 60% attack, 25% defend, 15% flexible - MORE AGGRESSIVE
        if defenders_at_base < 1 && role_choice < 25 {
            // Only 1-2 defenders needed
            BotObjective::DefendOwnFlag
        } else if attackers_going_for_flag < 5 && role_choice < 85 {
            // Most bots should attack
            BotObjective::AttackEnemyFlag
        } else if defenders_at_base < 2 && own_flag.map_or(false, |f| f.status == fb::FlagStatus::Dropped) {
            // If our flag is dropped, help return it
            BotObjective::DefendOwnFlag
        } else {
            // Default to attacking
            BotObjective::AttackEnemyFlag
        }
    }
    
    /// Find nearest enemy to the bot
    fn find_nearest_enemy(bot_state: &PlayerState, server_instance: &MassiveGameServer) -> Option<(Vec2, PlayerID)> {
        let mut nearest_enemy = None;
        let mut nearest_dist_sq = f32::MAX;
        
        server_instance.player_manager.for_each_player(|id, player| {
            if player.alive && player.team_id != bot_state.team_id && player.team_id != 0 {
                let dist_sq = (player.x - bot_state.x).powi(2) + (player.y - bot_state.y).powi(2);
                if dist_sq < nearest_dist_sq && dist_sq < BOT_TARGET_ACQUISITION_RANGE.powi(2) {
                    nearest_dist_sq = dist_sq;
                    nearest_enemy = Some((Vec2::new(player.x, player.y), id.clone()));
                }
            }
        });
        
        nearest_enemy
    }
    
    /// Enhanced movement decision with combat awareness
    fn make_simple_movement_decision(bot_controller: &mut BotController, bot_state: &PlayerState) {
        let mut rng = rand::thread_rng();
        
        // Randomly choose behavior
        let behavior_choice = rng.gen_range(0..100);
        
        if behavior_choice < 40 {
            // 40% - Aggressive: Move towards center for action
            let target_x = rng.gen_range(-200.0..200.0);
            let target_y = rng.gen_range(-200.0..200.0);
            bot_controller.target_position = Some(Vec2::new(target_x, target_y));
            bot_controller.behavior_state = BotBehaviorState::Engaging;
        } else if behavior_choice < 70 {
            // 30% - Flanking: Move to sides
            let side = if rng.gen_bool(0.5) { 1.0 } else { -1.0 };
            let target_x = side * rng.gen_range(300.0..600.0);
            let target_y = rng.gen_range(-400.0..400.0);
            bot_controller.target_position = Some(Vec2::new(target_x, target_y));
            bot_controller.behavior_state = BotBehaviorState::Flanking;
        } else {
            // 30% - Patrol: Random movement
            let target_x = rng.gen_range(WORLD_MIN_X + 100.0..WORLD_MAX_X - 100.0);
            let target_y = rng.gen_range(WORLD_MIN_Y + 100.0..WORLD_MAX_Y - 100.0);
            bot_controller.target_position = Some(Vec2::new(target_x, target_y));
            bot_controller.behavior_state = BotBehaviorState::Patrolling;
        }
        
        // Randomly switch weapons occasionally
        if rng.gen_bool(0.1) {
            bot_controller.path_recalculation_timer = Instant::now(); // Use as weapon switch timer
        }
        
        trace!("Bot {} behavior: {:?}, target: {:?}", 
            bot_state.username, bot_controller.behavior_state, bot_controller.target_position);
    }
    
    /// Check if there's a clear line of sight between two positions
    fn has_line_of_sight(
        from: Vec2,
        to: Vec2,
        server_instance: &MassiveGameServer,
    ) -> bool {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let distance = (dx * dx + dy * dy).sqrt();
        
        // Number of steps to check along the line
        let steps = (distance / 20.0).ceil() as usize;
        
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let check_x = from.x + dx * t;
            let check_y = from.y + dy * t;
            
            // Query walls near this point
            let nearby_walls = server_instance.wall_spatial_index.query_radius(check_x, check_y, 5.0);
            
            // Check if any wall blocks this point
            for wall in nearby_walls {
                // Skip destructible walls that are destroyed
                if wall.is_destructible && wall.current_health <= 0 {
                    continue;
                }
                
                // Check if point is inside wall
                if check_x >= wall.x && check_x <= wall.x + wall.width &&
                   check_y >= wall.y && check_y <= wall.y + wall.height {
                    return false; // Wall blocks line of sight
                }
            }
        }
        
        true // Clear line of sight
    }
    
    /// Generate enhanced combat input with shooting and movement
    fn generate_combat_input(
        bot_state: &PlayerState,
        bot_controller: &BotController,
        server_instance: &MassiveGameServer,
        game_mode: fb::GameModeType,
    ) -> PlayerInputData {
        let mut rng = rand::thread_rng();
        let current_time = Instant::now();
        
        let mut input = PlayerInputData {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
            sequence: bot_state.last_processed_input_sequence.wrapping_add(1),
            move_forward: false,
            move_backward: false,
            move_left: false,
            move_right: false,
            shooting: false,
            reload: false,
            rotation: bot_state.rotation,
            melee_attack: false,
            change_weapon_slot: 0,
            use_ability_slot: 0,
        };
        
        // Weapon switching logic
        if current_time.duration_since(bot_controller.path_recalculation_timer) < Duration::from_secs(1) {
            input.change_weapon_slot = rng.gen_range(1..=4);
        }
        
        // Reload if low on ammo
        if bot_state.ammo == 0 {
            input.reload = true;
        }
        
        // Find nearby enemies for combat
        let mut nearest_enemy_dist = f32::MAX;
        let mut nearest_enemy_angle = 0.0;
        let mut has_enemy_target = false;
        let mut enemy_position = Vec2::zero();
        
        server_instance.player_manager.for_each_player(|_, player| {
            if player.alive && player.team_id != bot_state.team_id && player.team_id != 0 {
                let dx = player.x - bot_state.x;
                let dy = player.y - bot_state.y;
                let dist_sq = dx * dx + dy * dy;
                
                if dist_sq < BOT_TARGET_ACQUISITION_RANGE.powi(2) && dist_sq < nearest_enemy_dist {
                    // Check line of sight before targeting
                    let bot_pos = Vec2::new(bot_state.x, bot_state.y);
                    let enemy_pos = Vec2::new(player.x, player.y);
                    
                    if Self::has_line_of_sight(bot_pos, enemy_pos, server_instance) {
                        nearest_enemy_dist = dist_sq;
                        nearest_enemy_angle = dy.atan2(dx);
                        has_enemy_target = true;
                        enemy_position = enemy_pos;
                        
                        // Priority target: enemy flag carrier
                        if game_mode == fb::GameModeType::CaptureTheFlag && 
                           player.is_carrying_flag_team_id == bot_state.team_id {
                            // This enemy has our flag - prioritize them!
                            nearest_enemy_dist *= 0.5; // Make them seem closer for priority
                        }
                    }
                }
            }
        });
        
        // Movement towards objective - THIS IS THE KEY PART
        let mut movement_handled = false;
        
        if let Some(target_pos) = bot_controller.target_position {
            let dx = target_pos.x - bot_state.x;
            let dy = target_pos.y - bot_state.y;
            let dist_sq = dx * dx + dy * dy;
            
            // Always set rotation towards target
            let target_angle = dy.atan2(dx);
            input.rotation = target_angle;
            
            // Move if not at target
            if dist_sq > BOT_MOVEMENT_TOLERANCE * BOT_MOVEMENT_TOLERANCE {
                // Always move forward when we have a target
                input.move_forward = true;
                movement_handled = true;
                
                // Add some zigzag movement occasionally
                if rng.gen_bool(0.1) {
                    if rng.gen_bool(0.5) {
                        input.move_left = true;
                    } else {
                        input.move_right = true;
                    }
                }
                
                // If carrying flag, sprint more
                if bot_state.is_carrying_flag_team_id != 0 {
                    input.move_forward = true;
                    // Less zigzag when carrying flag
                    if rng.gen_bool(0.05) {
                        input.move_left = rng.gen_bool(0.5);
                        input.move_right = !input.move_left;
                    }
                }
                
                trace!("Bot {} moving to target at ({:.0}, {:.0}), distance: {:.0}", 
                    bot_state.username, target_pos.x, target_pos.y, dist_sq.sqrt());
            } else {
                // At objective - defensive behavior
                match bot_controller.behavior_state {
                    BotBehaviorState::Defending => {
                        // Look around while defending
                        if rng.gen_bool(0.02) {
                            input.rotation += rng.gen_range(-1.5..1.5);
                        }
                        // Small movements to avoid being static
                        if rng.gen_bool(0.1) {
                            input.move_forward = rng.gen_bool(0.3);
                            input.move_backward = rng.gen_bool(0.3);
                            input.move_left = rng.gen_bool(0.3);
                            input.move_right = rng.gen_bool(0.3);
                        }
                    }
                    _ => {
                        // Patrol movement
                        if rng.gen_bool(0.05) {
                            input.move_forward = rng.gen_bool(0.5);
                            input.move_left = rng.gen_bool(0.5);
                            input.move_right = !input.move_left && rng.gen_bool(0.5);
                        }
                    }
                }
            }
        } else {
            // No target - wander randomly
            if rng.gen_bool(0.1) {
                input.move_forward = rng.gen_bool(0.7);
                input.move_backward = !input.move_forward && rng.gen_bool(0.3);
                input.move_left = rng.gen_bool(0.3);
                input.move_right = !input.move_left && rng.gen_bool(0.3);
                input.rotation += rng.gen_range(-0.5..0.5);
            }
        }
        
        // Combat behavior - only override rotation if we have a nearby enemy
        if has_enemy_target && nearest_enemy_dist < BOT_TARGET_ACQUISITION_RANGE.powi(2) {
            // Aim at enemy with some inaccuracy
            let aim_offset = rng.gen_range(-0.2..0.2) * (1.0 - BOT_SHOOT_ACCURACY);
            input.rotation = nearest_enemy_angle + aim_offset;
            
            // Shoot if close enough and have line of sight
            let shoot_range: f32 = match bot_state.weapon {
                ServerWeaponType::Shotgun => 150.0,
                ServerWeaponType::Sniper => 800.0,
                _ => 400.0,
            };
            
            if nearest_enemy_dist < shoot_range.powi(2) {
                // Apply reaction time
                if bot_controller.last_decision_time.elapsed() > BOT_REACTION_TIME {
                    input.shooting = rng.gen_bool(0.7); // 70% chance to shoot when in range
                    
                    // Sometimes use melee if very close
                    if nearest_enemy_dist < 60.0 * 60.0 && rng.gen_bool(0.3) {
                        input.melee_attack = true;
                        input.shooting = false;
                    }
                }
            }
            
            // Tactical movement during combat
            if has_enemy_target && !movement_handled {
                if nearest_enemy_dist < 200.0 * 200.0 {
                    // Strafe at close range
                    if rng.gen_bool(0.6) {
                        input.move_left = rng.gen_bool(0.5);
                        input.move_right = !input.move_left;
                    }
                    // Sometimes retreat
                    if nearest_enemy_dist < 100.0 * 100.0 && rng.gen_bool(0.3) {
                        input.move_backward = true;
                        input.move_forward = false;
                    }
                } else {
                    // Move towards enemy if not too close
                    if !bot_controller.target_position.is_some() {
                        input.move_forward = true;
                    }
                }
            }
        }
        
        input
    }
    
    /// Check if bot is stuck and needs to change direction
    fn check_stuck_status(bot_controller: &mut BotController, bot_state: &PlayerState, delta_time: f32) {
        let current_pos = Vec2::new(bot_state.x, bot_state.y);
        
        // Update stuck timer
        bot_controller.stuck_timer += delta_time;
        
        // Check position every BOT_STUCK_CHECK_INTERVAL seconds
        if bot_controller.stuck_timer >= BOT_STUCK_CHECK_INTERVAL {
            let dx = current_pos.x - bot_controller.stuck_check_position.x;
            let dy = current_pos.y - bot_controller.stuck_check_position.y;
            let distance_moved = (dx * dx + dy * dy).sqrt();
            
            // Check if bot has moved enough
            if distance_moved < BOT_STUCK_THRESHOLD {
                // Bot is potentially stuck
                if bot_controller.stuck_timer >= BOT_STUCK_TIME_THRESHOLD {
                    // Bot is definitely stuck - take action
                    warn!("Bot {} is stuck at ({:.0}, {:.0}), generating new target", 
                        bot_state.username, current_pos.x, current_pos.y);
                    
                    let mut rng = rand::thread_rng();
                    
                    // Try to move in a random direction away from current position
                    let escape_angle = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                    let escape_distance = rng.gen_range(100.0..300.0);
                    
                    let new_x = (current_pos.x + escape_distance * escape_angle.cos())
                        .clamp(WORLD_MIN_X + 100.0, WORLD_MAX_X - 100.0);
                    let new_y = (current_pos.y + escape_distance * escape_angle.sin())
                        .clamp(WORLD_MIN_Y + 100.0, WORLD_MAX_Y - 100.0);
                    
                    bot_controller.target_position = Some(Vec2::new(new_x, new_y));
                    bot_controller.behavior_state = BotBehaviorState::MovingToPosition;
                    
                    // Reset stuck detection
                    bot_controller.stuck_timer = 0.0;
                    bot_controller.stuck_check_position = current_pos;
                    bot_controller.last_position = current_pos;
                    
                    // Force a new decision soon
                    bot_controller.last_decision_time = Instant::now() - BOT_MOVEMENT_CHANGE_INTERVAL + Duration::from_millis(500);
                    
                    debug!("Bot {} unstuck - new target: ({:.0}, {:.0})", 
                        bot_state.username, new_x, new_y);
                }
            } else {
                // Bot has moved, reset stuck detection
                bot_controller.stuck_timer = 0.0;
                bot_controller.stuck_check_position = current_pos;
            }
        }
        
        // Always update last position
        bot_controller.last_position = current_pos;
    }
}
