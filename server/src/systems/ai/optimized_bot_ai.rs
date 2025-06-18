// Optimized Bot AI that won't block the game loop

use crate::core::types::{PlayerID, PlayerInputData, Vec2, PlayerState};
use crate::core::constants::*;
use crate::server::instance::{BotController, BotBehaviorState, MassiveGameServer};

use std::sync::Arc;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use rand::Rng;
use tracing::{trace, warn};

// Optimized constants
const BOT_UPDATE_BATCH_SIZE: usize = 10; // Process more bots per frame
const BOT_SIMPLE_MOVEMENT_ONLY: bool = false; // Enable full AI with combat
const BOT_MOVEMENT_CHANGE_INTERVAL: Duration = Duration::from_millis(1500); // Change behavior more frequently
const BOT_TARGET_ACQUISITION_RANGE: f32 = 400.0; // Range to detect enemies
const BOT_SHOOT_ACCURACY: f32 = 0.8; // 80% chance to aim correctly
const BOT_REACTION_TIME: Duration = Duration::from_millis(300); // Time before shooting

pub struct OptimizedBotAI;

impl OptimizedBotAI {
    /// Lightweight bot update that processes a batch of bots per frame
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
        
        // Calculate which batch to process this frame
        let batch_start = ((frame_count as usize) * BOT_UPDATE_BATCH_SIZE) % bot_ids.len();
        let batch_end = (batch_start + BOT_UPDATE_BATCH_SIZE).min(bot_ids.len());
        
        trace!("Frame {}: Processing bots {} to {} of {}", 
            frame_count, batch_start, batch_end, bot_ids.len());
        
        // Process only this batch of bots
        for i in batch_start..batch_end {
            let bot_id = &bot_ids[i];
            
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
                    
                    // Simple movement decision
                    if current_time.duration_since(bot_controller.last_decision_time) > BOT_MOVEMENT_CHANGE_INTERVAL {
                        bot_controller.last_decision_time = current_time;
                        Self::make_simple_movement_decision(bot_controller, &bot_state);
                    }
                    
                    // Generate input
                    let input = Self::generate_simple_input(&bot_state, bot_controller);
                    
                    // Queue the input
                    if let Some(mut player_state_entry) = server_instance.player_manager.get_player_state_mut(bot_id) {
                        player_state_entry.queue_input(input);
                    }
                }
            }
        }
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
    
    /// Generate enhanced combat input with shooting and movement
    fn generate_simple_input(
        bot_state: &PlayerState,
        bot_controller: &BotController,
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
        
        // Weapon switching logic - use path_recalculation_timer as weapon switch timer
        if current_time.duration_since(bot_controller.path_recalculation_timer) < Duration::from_secs(1) {
            // Recently decided to switch weapons
            input.change_weapon_slot = rng.gen_range(1..=4); // Switch between weapons 1-4
        }
        
        // Reload if low on ammo
        if bot_state.ammo == 0 {
            input.reload = true;
        }
        
        // Combat behavior based on state
        match bot_controller.behavior_state {
            BotBehaviorState::Engaging => {
                // Aggressive combat - always shoot when possible
                input.shooting = bot_state.ammo > 0 && rng.gen_bool(0.7);
                
                // Combat movement - strafe while shooting
                if input.shooting {
                    input.move_left = rng.gen_bool(0.3);
                    input.move_right = !input.move_left && rng.gen_bool(0.3);
                    input.move_forward = rng.gen_bool(0.4);
                    input.move_backward = !input.move_forward && rng.gen_bool(0.2);
                }
                
                // Occasional melee
                if rng.gen_bool(0.05) {
                    input.melee_attack = true;
                }
            }
            BotBehaviorState::Flanking => {
                // Strategic shooting with movement
                input.shooting = bot_state.ammo > 0 && rng.gen_bool(0.5);
                
                // More movement focused
                input.move_forward = rng.gen_bool(0.6);
                input.move_left = rng.gen_bool(0.4);
                input.move_right = !input.move_left && rng.gen_bool(0.4);
            }
            BotBehaviorState::Patrolling => {
                // Occasional defensive shooting
                input.shooting = bot_state.ammo > 0 && rng.gen_bool(0.3);
            }
            _ => {}
        }
        
        // Movement towards target
        if let Some(target_pos) = bot_controller.target_position {
            let dx = target_pos.x - bot_state.x;
            let dy = target_pos.y - bot_state.y;
            let dist_sq = dx * dx + dy * dy;
            
            // Set rotation towards target
            let target_angle = dy.atan2(dx);
            
            // Add some inaccuracy for more realistic aiming
            let aim_offset = if input.shooting {
                rng.gen_range(-0.1..0.1) * (1.0 - BOT_SHOOT_ACCURACY)
            } else {
                0.0
            };
            
            input.rotation = target_angle + aim_offset;
            
            // Move if not at target
            if dist_sq > 50.0 * 50.0 {
                // Base movement
                if !input.move_forward && !input.move_backward {
                    input.move_forward = true;
                }
                
                // Dynamic strafing
                if dist_sq < 200.0 * 200.0 {
                    // Close range - more erratic movement
                    if rng.gen_bool(0.2) {
                        input.move_left = rng.gen_bool(0.5);
                        input.move_right = !input.move_left;
                        input.move_backward = rng.gen_bool(0.3);
                        input.move_forward = !input.move_backward;
                    }
                }
            } else {
                // At target - look around randomly
                if rng.gen_bool(0.1) {
                    input.rotation += rng.gen_range(-1.0..1.0);
                }
                
                // Occasional movement to avoid being static
                if rng.gen_bool(0.05) {
                    input.move_forward = rng.gen_bool(0.5);
                    input.move_left = rng.gen_bool(0.5);
                    input.move_right = !input.move_left && rng.gen_bool(0.5);
                }
            }
        }
        
        input
    }
}
