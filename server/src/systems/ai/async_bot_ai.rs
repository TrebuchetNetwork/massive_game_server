// Async Bot AI that runs independently from the game loop

use crate::core::types::{PlayerID, PlayerInputData, Vec2, PlayerState, Projectile, Pickup, CorePickupType};
use crate::core::constants::*;
use crate::server::instance::{BotController, BotBehaviorState, MassiveGameServer};

use std::sync::Arc;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use rand::Rng;
use tracing::{trace, debug, info};
use tokio::sync::mpsc;
use tokio::time::interval;
use dashmap::DashMap;

// Bot AI decisions are made independently and sent via channels
pub struct BotDecision {
    pub bot_id: PlayerID,
    pub input: PlayerInputData,
}

pub struct AsyncBotAI {
    decision_sender: mpsc::UnboundedSender<BotDecision>,
    decision_receiver: mpsc::UnboundedReceiver<BotDecision>,
}

impl AsyncBotAI {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            decision_sender: tx,
            decision_receiver: rx,
        }
    }

    /// Start the async bot AI task that runs independently
    pub fn start_bot_ai_task(
        server: Arc<MassiveGameServer>,
        decision_sender: mpsc::UnboundedSender<BotDecision>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut bot_update_interval = interval(Duration::from_millis(100)); // Update bots every 100ms
            let mut bot_think_times: HashMap<PlayerID, Instant> = HashMap::new();
            
            loop {
                bot_update_interval.tick().await;
                
                // Get all bot IDs
                let bot_ids: Vec<PlayerID> = server.bot_players
                    .iter()
                    .map(|entry| entry.key().clone())
                    .collect();
                
                for bot_id in bot_ids {
                    let now = Instant::now();
                    let should_think = bot_think_times
                        .get(&bot_id)
                        .map(|&last_think| now.duration_since(last_think) > Duration::from_millis(500))
                        .unwrap_or(true);
                    
                    if should_think {
                        bot_think_times.insert(bot_id.clone(), now);
                        
                        // Get bot and player state
                        let (bot_state, bot_controller) = {
                            let bot_state_opt = server.player_manager
                                .get_player_state(&bot_id)
                                .map(|guard| (*guard).clone());
                            
                            let bot_controller_opt = server.bot_players
                                .get(&bot_id)
                                .map(|entry| entry.value().clone());
                            
                            match (bot_state_opt, bot_controller_opt) {
                                (Some(state), Some(controller)) => (state, controller),
                                _ => continue,
                            }
                        };
                        
                        if !bot_state.alive {
                            continue;
                        }
                        
                        // Make intelligent decision
                        let decision = Self::make_intelligent_decision(
                            &server,
                            &bot_state,
                            &bot_controller,
                        ).await;
                        
                        if let Some(input) = decision {
                            let _ = decision_sender.send(BotDecision {
                                bot_id: bot_id.clone(),
                                input,
                            });
                        }
                    }
                }
                
                // Check if server is shutting down
                if server.is_shutting_down.load(std::sync::atomic::Ordering::Relaxed) {
                    info!("Bot AI task shutting down");
                    break;
                }
            }
        })
    }
    
    /// Make intelligent decisions based on game state
    async fn make_intelligent_decision(
        server: &Arc<MassiveGameServer>,
        bot_state: &PlayerState,
        bot_controller: &BotController,
    ) -> Option<PlayerInputData> {
        let mut rng = rand::thread_rng();
        
        // Find nearby threats and targets
        let nearby_enemies = Self::find_nearby_enemies(server, bot_state, 500.0);
        let nearby_pickups = Self::find_nearby_pickups(server, bot_state, 300.0);
        let incoming_projectiles = Self::detect_incoming_projectiles(server, bot_state, 150.0);
        
        // Decide behavior based on situation
        let mut new_behavior = bot_controller.behavior_state.clone();
        let mut target_pos = bot_controller.target_position;
        let mut should_shoot = false;
        let mut should_reload = false;
        
        // Priority 1: Dodge incoming projectiles
        if !incoming_projectiles.is_empty() {
            new_behavior = BotBehaviorState::Flanking;
            // Calculate dodge direction perpendicular to projectile
            if let Some(proj) = incoming_projectiles.first() {
                let dodge_angle = (proj.y - bot_state.y).atan2(proj.x - bot_state.x) + std::f32::consts::FRAC_PI_2;
                let dodge_dist = 100.0;
                target_pos = Some(Vec2::new(
                    bot_state.x + dodge_angle.cos() * dodge_dist,
                    bot_state.y + dodge_angle.sin() * dodge_dist,
                ));
            }
        }
        // Priority 2: Engage nearby enemies
        else if let Some((enemy_id, enemy_pos, enemy_health)) = nearby_enemies.first() {
            new_behavior = BotBehaviorState::Engaging;
            
            // Calculate engagement position
            let dist_to_enemy = ((enemy_pos.x - bot_state.x).powi(2) + (enemy_pos.y - bot_state.y).powi(2)).sqrt();
            
            if dist_to_enemy < 150.0 {
                // Too close, back up while shooting
                let retreat_angle = (bot_state.y - enemy_pos.y).atan2(bot_state.x - enemy_pos.x);
                target_pos = Some(Vec2::new(
                    bot_state.x + retreat_angle.cos() * 50.0,
                    bot_state.y + retreat_angle.sin() * 50.0,
                ));
                should_shoot = true;
            } else if dist_to_enemy > 300.0 {
                // Move closer
                target_pos = Some(*enemy_pos);
            } else {
                // Good distance, strafe and shoot
                let strafe_angle = (enemy_pos.y - bot_state.y).atan2(enemy_pos.x - bot_state.x) + 
                    if rng.gen_bool(0.5) { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 };
                target_pos = Some(Vec2::new(
                    bot_state.x + strafe_angle.cos() * 50.0,
                    bot_state.y + strafe_angle.sin() * 50.0,
                ));
                should_shoot = true;
            }
            
            // Check ammo
            if bot_state.ammo == 0 {
                should_reload = true;
                should_shoot = false;
            }
        }
        // Priority 3: Seek health/ammo if needed
        else if (bot_state.health < 50 || bot_state.ammo < 5) && !nearby_pickups.is_empty() {
            new_behavior = BotBehaviorState::SeekingPickup;
            // Find best pickup
            let best_pickup = nearby_pickups.iter()
                .min_by_key(|(_, pos, pickup_type)| {
                    let dist = ((pos.x - bot_state.x).powi(2) + (pos.y - bot_state.y).powi(2)) as i32;
                    // Prioritize health if low health
                    if bot_state.health < 30 && matches!(pickup_type, CorePickupType::Health) {
                        dist / 2
                    } else {
                        dist
                    }
                });
            
            if let Some((_, pickup_pos, _)) = best_pickup {
                target_pos = Some(*pickup_pos);
            }
        }
        // Priority 4: Patrol or seek objectives
        else {
            new_behavior = BotBehaviorState::Patrolling;
            // Move to strategic positions or random patrol
            if target_pos.is_none() || rng.gen_bool(0.1) {
                let patrol_points = vec![
                    Vec2::new(0.0, 0.0), // Center
                    Vec2::new(-500.0, -500.0),
                    Vec2::new(500.0, -500.0),
                    Vec2::new(-500.0, 500.0),
                    Vec2::new(500.0, 500.0),
                ];
                target_pos = Some(patrol_points[rng.gen_range(0..patrol_points.len())]);
            }
        }
        
        // Update bot controller
        if let Some(mut bot_controller_entry) = server.bot_players.get_mut(&bot_controller.player_id) {
            bot_controller_entry.behavior_state = new_behavior;
            bot_controller_entry.target_position = target_pos;
        }
        
        // Generate input based on decisions
        let input = Self::generate_movement_input(
            bot_state,
            target_pos,
            should_shoot,
            should_reload,
            &nearby_enemies,
        );
        
        Some(input)
    }
    
    /// Find nearby enemy players
    fn find_nearby_enemies(
        server: &Arc<MassiveGameServer>,
        bot_state: &PlayerState,
        range: f32,
    ) -> Vec<(PlayerID, Vec2, i32)> {
        let mut enemies = Vec::new();
        
        let nearby_players = server.spatial_index.query_nearby_players(
            bot_state.x,
            bot_state.y,
            range,
        );
        
        for player_id in nearby_players {
            if let Some(player_state) = server.player_manager.get_player_state(&player_id) {
                if player_state.alive && 
                   player_state.team_id != bot_state.team_id && 
                   player_state.team_id != 0 {
                    enemies.push((
                        player_id.clone(),
                        Vec2::new(player_state.x, player_state.y),
                        player_state.health,
                    ));
                }
            }
        }
        
        // Sort by distance
        enemies.sort_by_key(|(_, pos, _)| {
            ((pos.x - bot_state.x).powi(2) + (pos.y - bot_state.y).powi(2)) as i32
        });
        
        enemies
    }
    
    /// Find nearby pickups
    fn find_nearby_pickups(
        server: &Arc<MassiveGameServer>,
        bot_state: &PlayerState,
        range: f32,
    ) -> Vec<(u64, Vec2, CorePickupType)> {
        let mut nearby_pickups = Vec::new();
        let pickups_guard = server.pickups.read();
        
        for pickup in pickups_guard.iter() {
            if pickup.is_active {
                let dist_sq = (pickup.x - bot_state.x).powi(2) + (pickup.y - bot_state.y).powi(2);
                if dist_sq <= range * range {
                    nearby_pickups.push((
                        pickup.id,
                        Vec2::new(pickup.x, pickup.y),
                        pickup.pickup_type.clone(),
                    ));
                }
            }
        }
        
        nearby_pickups
    }
    
    /// Detect incoming projectiles
    fn detect_incoming_projectiles(
        server: &Arc<MassiveGameServer>,
        bot_state: &PlayerState,
        range: f32,
    ) -> Vec<Projectile> {
        let mut threats = Vec::new();
        let projectiles_guard = server.projectiles.read();
        
        for proj in projectiles_guard.iter() {
            if proj.owner_id == bot_state.id {
                continue; // Own projectile
            }
            
            let dist_sq = (proj.x - bot_state.x).powi(2) + (proj.y - bot_state.y).powi(2);
            if dist_sq <= range * range {
                // Check if projectile is heading towards us
                let future_x = proj.x + proj.velocity_x * 0.5;
                let future_y = proj.y + proj.velocity_y * 0.5;
                let future_dist_sq = (future_x - bot_state.x).powi(2) + (future_y - bot_state.y).powi(2);
                
                if future_dist_sq < dist_sq {
                    threats.push(proj.clone());
                }
            }
        }
        
        threats
    }
    
    /// Generate movement input based on decisions
    fn generate_movement_input(
        bot_state: &PlayerState,
        target_pos: Option<Vec2>,
        should_shoot: bool,
        should_reload: bool,
        nearby_enemies: &[(PlayerID, Vec2, i32)],
    ) -> PlayerInputData {
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
            shooting: should_shoot,
            reload: should_reload,
            rotation: bot_state.rotation,
            melee_attack: false,
            change_weapon_slot: 0,
            use_ability_slot: 0,
        };
        
        // Calculate rotation towards nearest enemy or target
        if let Some((_, enemy_pos, _)) = nearby_enemies.first() {
            input.rotation = (enemy_pos.y - bot_state.y).atan2(enemy_pos.x - bot_state.x);
        } else if let Some(target) = target_pos {
            input.rotation = (target.y - bot_state.y).atan2(target.x - bot_state.x);
        }
        
        // Movement towards target
        if let Some(target) = target_pos {
            let dx = target.x - bot_state.x;
            let dy = target.y - bot_state.y;
            let dist_sq = dx * dx + dy * dy;
            
            if dist_sq > 25.0 { // Move if more than 5 units away
                let angle_diff = ((input.rotation - bot_state.rotation).rem_euclid(2.0 * std::f32::consts::PI) - std::f32::consts::PI).abs();
                
                if angle_diff < std::f32::consts::FRAC_PI_4 {
                    input.move_forward = true;
                } else if angle_diff > 3.0 * std::f32::consts::FRAC_PI_4 {
                    input.move_backward = true;
                }
                
                // Strafe for better movement
                let strafe_angle = input.rotation - bot_state.rotation;
                if strafe_angle.sin() > 0.3 {
                    input.move_right = true;
                } else if strafe_angle.sin() < -0.3 {
                    input.move_left = true;
                }
            }
        }
        
        input
    }
    
    /// Poll for bot decisions and apply them
    pub fn poll_and_apply_decisions(&mut self, server: &MassiveGameServer) {
        while let Ok(decision) = self.decision_receiver.try_recv() {
            if let Some(mut player_state_entry) = server.player_manager.get_player_state_mut(&decision.bot_id) {
                player_state_entry.queue_input(decision.input);
            }
        }
    }
}
