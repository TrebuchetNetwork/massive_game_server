// Create new file: src/systems/bots.rs

use crate::core::types::{PlayerID, PlayerInputData, ServerWeaponType, Vec2};
use crate::core::constants::*;
use crate::entities::player::ImprovedPlayerManager;
use crate::concurrent::spatial_index::ImprovedSpatialIndex;
use std::sync::Arc;
use std::time::Instant;
use parking_lot::RwLock;
use rand::Rng;
use uuid::Uuid;

mod open_ai_bot;

#[derive(Clone, Debug)]
pub struct BotBehavior {
    pub id: String,
    pub target_position: Option<Vec2>,
    pub target_player: Option<PlayerID>,
    pub last_decision_time: Instant,
    pub behavior_type: BotType,
    pub skill_level: f32, // 0.0 to 1.0
    pub team_id: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BotType {
    Aggressive,
    Defensive,
    Balanced,
    OpenAI,
}

pub struct BotManager {
    bots: Arc<RwLock<Vec<BotBehavior>>>,
    max_bots: usize,
    bot_names: Vec<&'static str>,
}

impl BotManager {
    pub fn new(max_bots: usize) -> Self {
        let bot_names = vec![
            "Bot_Alpha", "Bot_Bravo", "Bot_Charlie", "Bot_Delta", "Bot_Echo",
            "Bot_Foxtrot", "Bot_Golf", "Bot_Hotel", "Bot_India", "Bot_Juliet",
            "Bot_Kilo", "Bot_Lima", "Bot_Mike", "Bot_Nova", "Bot_Oscar",
            "Bot_Papa", "Bot_Quebec", "Bot_Romeo", "Bot_Sierra", "Bot_Tango",
        ];
        
        Self {
            bots: Arc::new(RwLock::new(Vec::new())),
            max_bots,
            bot_names,
        }
    }
    
    pub fn initialize_bots(&self, player_manager: &ImprovedPlayerManager) -> Vec<String> {
        let mut rng = rand::thread_rng();
        let mut bot_ids = Vec::new();
        let mut bots = self.bots.write();
        
        for i in 0..self.max_bots {
            let bot_id = format!("bot_{}", Uuid::new_v4());
            let bot_name = self.bot_names.get(i % self.bot_names.len())
                .unwrap_or(&"Bot_Unknown");
            
            let team_id = if i < self.max_bots / 2 { 1 } else { 2 }; // Split evenly between teams
            
            let spawn_x = if team_id == 1 {
                rng.gen_range((WORLD_MIN_X + 200.0)..(WORLD_MIN_X + 600.0))
            } else {
                rng.gen_range((WORLD_MAX_X - 600.0)..(WORLD_MAX_X - 200.0))
            };
            let spawn_y = rng.gen_range((WORLD_MIN_Y + 200.0)..(WORLD_MAX_Y - 200.0));
            
            // Add bot to player manager
            if let Some(player_id) = player_manager.add_player(
                bot_id.clone(),
                format!("{} [BOT]", bot_name),
                spawn_x,
                spawn_y
            ) {
                // Set bot's team
                if let Some(mut player_state) = player_manager.get_player_state_mut(&player_id) {
                    player_state.team_id = team_id;
                }
                
                let behavior = BotBehavior {
                    id: bot_id.clone(),
                    target_position: None,
                    target_player: None,
                    last_decision_time: Instant::now(),
                    behavior_type: match rng.gen_range(0..4) {
                        0 => BotType::Aggressive,
                        1 => BotType::Defensive,
                        2 => BotType::Balanced,
                        _ => BotType::OpenAI,
                    },
                    skill_level: rng.gen_range(0.4..0.9),
                    team_id,
                };
                
                bots.push(behavior);
                bot_ids.push(bot_id);
            }
        }
        
        bot_ids
    }
    
    pub fn remove_bot_for_player(&self, player_manager: &ImprovedPlayerManager) -> Option<String> {
        let mut bots = self.bots.write();
        
        if let Some(bot) = bots.pop() {
            player_manager.remove_player(&bot.id);
            return Some(bot.id);
        }
        
        None
    }
    
    pub fn update_bots(
        &self,
        player_manager: &ImprovedPlayerManager,
        spatial_index: &ImprovedSpatialIndex,
        current_time: Instant,
    ) {
        let mut bots = self.bots.write();
        let mut rng = rand::thread_rng();
        
        for bot in bots.iter_mut() {
            // Get bot's player ID
            let bot_player_id = player_manager.id_pool.get_or_create(&bot.id);
            
            // Get bot's current state
            let (bot_pos, bot_alive, bot_health, bot_weapon) = {
                if let Some(state) = player_manager.get_player_state(&bot_player_id) {
                    (Vec2::new(state.x, state.y), state.alive, state.health, state.weapon)
                } else {
                    continue;
                }
            };
            
            if !bot_alive {
                continue;
            }
            
            // Make decisions every 100-300ms based on skill
            let decision_interval = 0.1 + (1.0 - bot.skill_level) * 0.2;
            if current_time.duration_since(bot.last_decision_time).as_secs_f32() > decision_interval {
                bot.last_decision_time = current_time;
                
                // Find nearby players
                let nearby_players = spatial_index.query_nearby_players(
                    bot_pos.x,
                    bot_pos.y,
                    500.0 // Detection radius
                );
                
                // Find closest enemy
                let mut closest_enemy: Option<(PlayerID, f32, Vec2)> = None;
                let mut closest_distance = f32::MAX;
                
                for player_id in &nearby_players {
                    if player_id == &bot_player_id {
                        continue;
                    }
                    
                    if let Some(player_state) = player_manager.get_player_state(player_id) {
                        if !player_state.alive || player_state.team_id == bot.team_id {
                            continue;
                        }
                        
                        let dist = ((player_state.x - bot_pos.x).powi(2) + 
                                   (player_state.y - bot_pos.y).powi(2)).sqrt();
                        
                        if dist < closest_distance {
                            closest_distance = dist;
                            closest_enemy = Some((
                                player_id.clone(),
                                dist,
                                Vec2::new(player_state.x, player_state.y)
                            ));
                        }
                    }
                }
                
                // Update behavior based on situation
                match bot.behavior_type {
                    BotType::Aggressive => {
                        // Always chase closest enemy
                        if let Some((enemy_id, _, enemy_pos)) = closest_enemy {
                            bot.target_player = Some(enemy_id);
                            bot.target_position = Some(enemy_pos);
                        } else {
                            // Roam towards enemy base
                            bot.target_position = Some(Vec2::new(
                                if bot.team_id == 1 { 
                                    rng.gen_range(0.0..WORLD_MAX_X - 200.0)
                                } else {
                                    rng.gen_range((WORLD_MIN_X + 200.0)..0.0)
                                },
                                rng.gen_range((WORLD_MIN_Y + 200.0)..(WORLD_MAX_Y - 200.0))
                            ));
                        }
                    }
                    BotType::Defensive => {
                        // Stay near base unless enemy is very close
                        if let Some((enemy_id, dist, enemy_pos)) = closest_enemy {
                            if dist < 300.0 {
                                bot.target_player = Some(enemy_id);
                                bot.target_position = Some(enemy_pos);
                            } else {
                                // Return to base area
                                bot.target_position = Some(Vec2::new(
                                    if bot.team_id == 1 {
                                        rng.gen_range((WORLD_MIN_X + 150.0)..(WORLD_MIN_X + 400.0))
                                    } else {
                                        rng.gen_range((WORLD_MAX_X - 400.0)..(WORLD_MAX_X - 150.0))
                                    },
                                    rng.gen_range(-200.0..200.0)
                                ));
                            }
                        }
                    }
                    BotType::Balanced => {
                        // Mix of aggressive and defensive
                        if let Some((enemy_id, dist, enemy_pos)) = closest_enemy {
                            if dist < 400.0 || bot_health > 70 {
                                bot.target_player = Some(enemy_id);
                                bot.target_position = Some(enemy_pos);
                            } else {
                                // Find cover or health
                                bot.target_position = Some(Vec2::new(
                                    rng.gen_range(-400.0..400.0),
                                    rng.gen_range(-300.0..300.0)
                                ));
                            }
                        }
                    }
                    BotType::OpenAI => {
                        open_ai_bot::decide_action(
                            bot,
                            bot_pos,
                            bot_health,
                            closest_enemy,
                            &mut rng,
                        );
                    }
                }
            }
            
            // Generate input based on current target
            let mut input = PlayerInputData {
                timestamp: current_time.elapsed().as_millis() as u64,
                sequence: rng.gen::<u32>() % 10000,
                move_forward: false,
                move_backward: false,
                move_left: false,
                move_right: false,
                shooting: false,
                reload: false,
                rotation: 0.0,
                melee_attack: false,
                change_weapon_slot: 0,
                use_ability_slot: 0,
            };
            
            if let Some(target_pos) = bot.target_position {
                // Calculate movement direction
                let dx = target_pos.x - bot_pos.x;
                let dy = target_pos.y - bot_pos.y;
                let dist = (dx * dx + dy * dy).sqrt();
                
                if dist > 50.0 { // Move if not too close
                    let angle = dy.atan2(dx);
                    
                    // Set movement based on angle (with some inaccuracy based on skill)
                    let angle_error = (1.0 - bot.skill_level) * 0.3;
                    let move_angle = angle + rng.gen_range(-angle_error..angle_error);
                    
                    if move_angle.cos() > 0.5 {
                        input.move_right = true;
                    } else if move_angle.cos() < -0.5 {
                        input.move_left = true;
                    }
                    
                    if move_angle.sin() > 0.5 {
                        input.move_backward = true;
                    } else if move_angle.sin() < -0.5 {
                        input.move_forward = true;
                    }
                }
                
                // Aim at target
                input.rotation = dy.atan2(dx);
                
                // Shoot if enemy is targeted and in range
                if bot.target_player.is_some() {
                    let effective_range = match bot_weapon {
                        ServerWeaponType::Shotgun => 150.0,
                        ServerWeaponType::Pistol => 300.0,
                        ServerWeaponType::Rifle => 400.0,
                        ServerWeaponType::Sniper => 600.0,
                        ServerWeaponType::Melee => 50.0,
                    };
                    
                    if dist < effective_range {
                        // Add accuracy based on skill
                        let hit_chance = bot.skill_level * 0.8 + 0.2;
                        if rng.gen::<f32>() < hit_chance {
                            input.shooting = true;
                        }
                    }
                }
            } else {
                // Wander randomly
                if rng.gen::<f32>() < 0.1 {
                    bot.target_position = Some(Vec2::new(
                        rng.gen_range((WORLD_MIN_X + 200.0)..(WORLD_MAX_X - 200.0)),
                        rng.gen_range((WORLD_MIN_Y + 200.0)..(WORLD_MAX_Y - 200.0))
                    ));
                }
            }
            
            // Random actions
            if rng.gen::<f32>() < 0.01 {
                input.reload = true;
            }
            
            // Queue input for bot
            if let Some(mut player_state) = player_manager.get_player_state_mut(&bot_player_id) {
                player_state.queue_input(input);
            }
        }
    }
    
    pub fn get_bot_count(&self) -> usize {
        self.bots.read().len()
    }
}