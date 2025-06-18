// massive_game_server/server/src/server/game_loop.rs
use super::instance::MassiveGameServer;
use crate::core::constants::{TICK_DURATION, SERVER_TICK_RATE};
// Removed unused: use crate::network::signaling::{ChatMessage, handle_dc_send_error};
// Removed unused: use crate::flatbuffers_generated::game_protocol as fb;
use std::sync::Arc;
use std::time::{Instant, Duration}; // Removed unused SystemTime, UNIX_EPOCH
use tokio::time::interval;
use tracing::{info, warn, error, debug, trace};
// Removed unused: use std::collections::VecDeque;
use std::sync::atomic::Ordering as AtomicOrdering;
use futures::executor::block_on;
// Removed unused: use bytes::Bytes;
// Removed unused: use crate::core::types::{PlayerID, PlayerAoI, Vec2, GameEvent, CorePickupType, EventPriority};
// Removed unused: use crate::core::types::EntityId; 
// Removed unused: use std::collections::HashSet; 
use crate::core::types::{PlayerID, PlayerAoI, Vec2};
use tokio::time::sleep; // Add this import
use std::collections::HashSet; // If not already imported for PlayerAoI
use crate::core::constants::{AOI_RADIUS, AOI_UPDATE_INTERVAL_SECS}; // Assuming these are in constants
use crate::network::signaling::{ClientState, ChatMessage};


const MAX_FRAME_TIME_HISTORY: usize = 100;
const SIGNIFICANT_MOVEMENT_THRESHOLD_SQ: f32 = 5.0 * 5.0; // Player must move more than 5 units for AoI recalc

impl MassiveGameServer {
    

    /*pub async fn run_game_loop_v2(self: Arc<Self>) {
        let mut tick_timer = interval(TICK_DURATION);
        let mut last_tick_time = Instant::now();
        let mut frame_count = 0;
    
        info!("Game loop started. Tick rate: {}ms", TICK_DURATION.as_millis());
    
        loop {
            tick_timer.tick().await;
            let frame_start_time = Instant::now();
            
            info!("Starting frame {}", frame_count);
            
            if let Err(e) = Arc::clone(&self).process_game_tick(TICK_DURATION.as_secs_f32()).await {
                error!("Game tick failed: {:?}", e);
            }
            
            let frame_time = frame_start_time.elapsed();
            if frame_time > TICK_DURATION {
                warn!("Frame {} took {:?} (target: {:?})", frame_count, frame_time, TICK_DURATION);
            }
            
            frame_count += 1;
            self.frame_counter.store(frame_count, AtomicOrdering::Relaxed);
        }
    }*/

    

    pub async fn run_game_loop(self: Arc<Self>) {
        let delta_time_fixed = 1.0 / self.config.tick_rate as f32;
        let mut tick_timer = interval(TICK_DURATION);
        let mut last_tick_time = Instant::now();
        let mut bots_spawned = false;
    
        info!("Game loop started. Tick rate: {}ms, Delta time: {}s", TICK_DURATION.as_millis(), delta_time_fixed);
    
        loop {
            let frame_start_time = Instant::now();
            tick_timer.tick().await;
            
            let current_frame = self.frame_counter.load(AtomicOrdering::Relaxed);
            
            // Spawn bots after 10 frames to let server stabilize
            if !bots_spawned && current_frame == 10 {
                let initial_bot_count = self.target_bot_count.load(AtomicOrdering::Relaxed) as usize;
                info!("Spawning {} bots after server stabilization (frame {})", initial_bot_count, current_frame);
                self.spawn_initial_bots(initial_bot_count);
                bots_spawned = true;
            }
            
            // Log every 60 frames (1 second at 60 FPS)
            if current_frame % 60 == 0 {
                info!("Game loop running - Frame: {}", current_frame);
            }
    
            // Process game tick
            if let Err(e) = Arc::clone(&self).process_game_tick(delta_time_fixed).await {
                error!("Game tick failed: {:?}", e);
                continue; // Don't stop the game loop on error
            }
    
            self.frame_counter.fetch_add(1, AtomicOrdering::Relaxed);
            
            // Log frame time if it's too long
            let frame_time = frame_start_time.elapsed();
            if frame_time > TICK_DURATION + Duration::from_millis(5) {
                warn!("Frame {} took too long: {:?}", current_frame, frame_time);
            }
        }
    }


    /*pub async fn synchronize_state(&self) {
        self.player_manager.for_each_player(|player_id, player_state| {
            self.spatial_index.update_player_position(player_id.clone(), player_state.x, player_state.y);

            let partition_idx = self.world_partition_manager.get_partition_index_for_point(player_state.x, player_state.y);
            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                let is_newly_entered_partition = !partition.local_players.contains(player_id);
                partition.update_player_status(player_id, player_state.x, player_state.y, is_newly_entered_partition);
            }

            self.update_player_aoi(player_id, player_state.x, player_state.y);
        });

        self.world_partition_manager.update_all_boundary_snapshots();
    }*/

    pub async fn synchronize_state(&self) {
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        trace!("[Frame {}] Starting synchronize_state", frame);
        let sync_loop_start = Instant::now();
    
        // Step 1: Collect player data that needs processing
        // This releases the write lock after each player
        let mut players_to_update = Vec::new();
        
        self.player_manager.for_each_player_mut(|player_id, player_state| {
            let player_processing_start = Instant::now();
    
            let last_pos_opt = self.player_last_sync_positions.get(player_id);
            let mut needs_full_aoi_update = true;
    
            if let Some(last_pos_entry) = last_pos_opt {
                let last_pos = last_pos_entry.value();
                let dist_moved_sq = (player_state.x - last_pos.0).powi(2) + 
                                    (player_state.y - last_pos.1).powi(2);
                
                if dist_moved_sq < SIGNIFICANT_MOVEMENT_THRESHOLD_SQ && player_state.changed_fields == 0 { 
                    needs_full_aoi_update = false;
                }
            }
            
            // Always update spatial index for current position
            let spatial_update_start = Instant::now();
            self.spatial_index.update_player_position(player_id.clone(), player_state.x, player_state.y);
            trace!("[Frame {}] Player {}: Spatial index updated in {:?}", 
                frame, player_id.as_str(), spatial_update_start.elapsed());
    
            // Update partition status
            let partition_update_start = Instant::now();
            let partition_idx = self.world_partition_manager.get_partition_index_for_point(
                player_state.x, player_state.y
            );
            
            // Collect data for later processing (after write lock is released)
            players_to_update.push((
                player_id.clone(),
                player_state.x,
                player_state.y,
                partition_idx,
                needs_full_aoi_update
            ));
            
            trace!("[Frame {}] Player {}: Prepared for update in {:?}", 
                frame, player_id.as_str(), player_processing_start.elapsed());
        });
        
        // Step 2: Process updates that require read access (no write locks held)
        for (player_id, x, y, partition_idx, needs_full_aoi_update) in players_to_update {
            // Update partition status
            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                let is_newly_entered_partition = !partition.local_players.contains(&player_id);
                partition.update_player_status(&player_id, x, y, is_newly_entered_partition);
            }
            
            // Update AoI if needed
            if needs_full_aoi_update {
                let aoi_update_start = Instant::now();
                self.update_player_aoi(&player_id, x, y);
                self.player_last_sync_positions.insert(player_id.clone(), (x, y));
                trace!("[Frame {}] Player {}: AoI updated in {:?}", 
                    frame, player_id.as_str(), aoi_update_start.elapsed());
            } else {
                trace!("[Frame {}] Player {}: Full AoI update skipped", 
                    frame, player_id.as_str());
            }
        }
        
            // Update boundary snapshots less frequently (every 30 frames)
            if frame % 30 == 0 {
                let boundary_update_start = Instant::now();
                self.world_partition_manager.update_all_boundary_snapshots();
                trace!("[Frame {}] Boundary snapshots updated in {:?}", 
                    frame, boundary_update_start.elapsed());
            }
        
        trace!("[Frame {}] Finished synchronize_state in {:?}", 
            frame, sync_loop_start.elapsed());
    }
    


    


    pub fn update_player_aoi(&self, player_id: &PlayerID, x: f32, y: f32) {
        const AOI_RADIUS_SQUARED: f32 = AOI_RADIUS * AOI_RADIUS;
        
        let player_id_str = player_id.as_str();
        
        // Ensure player exists before updating AoI
        if self.player_manager.get_player_state(player_id).is_none() {
            debug!("Player {} not found, skipping AoI update", player_id_str);
            return;
        }
        
        let mut player_aoi_entry = self.player_aois.entry(player_id_str.to_string())
            .or_insert_with(PlayerAoI::new);
        
        if player_aoi_entry.value().last_update.elapsed().as_secs_f32() < AOI_UPDATE_INTERVAL_SECS {
            return;
        }
        
        let player_aoi = player_aoi_entry.value_mut();
        
        // Clear previous data
        player_aoi.visible_players.clear();
        player_aoi.visible_projectiles.clear();
        player_aoi.visible_pickups.clear();
        player_aoi.visible_walls.clear();
        
        // 1. Update visible players (using spatial index)
        let nearby_player_ids = self.spatial_index.query_nearby_players(x, y, AOI_RADIUS);
        for other_id_arc in nearby_player_ids {
            if &other_id_arc != player_id {
                player_aoi.visible_players.insert(other_id_arc);
            }
        }
        
        // 2. Update visible projectiles
        let projectiles_guard = self.projectiles.read();
        let projectile_count = projectiles_guard.len();
        for proj in projectiles_guard.iter() {
            let dx = proj.x - x;
            let dy = proj.y - y;
            if (dx * dx + dy * dy) <= AOI_RADIUS_SQUARED {
                player_aoi.visible_projectiles.insert(proj.id);
            }
        }
        drop(projectiles_guard);
        
        // 3. Update visible pickups
        let pickups_guard = self.pickups.read();
        let total_pickups = pickups_guard.len();
        let mut active_pickups = 0;
        for pickup in pickups_guard.iter() {
            if pickup.is_active {
                active_pickups += 1;
                let dx = pickup.x - x;
                let dy = pickup.y - y;
                if (dx * dx + dy * dy) <= AOI_RADIUS_SQUARED {
                    player_aoi.visible_pickups.insert(pickup.id);
                }
            }
        }
        drop(pickups_guard);
        
        // 4. Update visible walls (check relevant partitions)
        let min_aoi_x = x - AOI_RADIUS;
        let max_aoi_x = x + AOI_RADIUS;
        let min_aoi_y = y - AOI_RADIUS;
        let max_aoi_y = y + AOI_RADIUS;
        
        let mut relevant_partition_indices = HashSet::new();
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(x, y));
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(min_aoi_x, min_aoi_y));
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(max_aoi_x, min_aoi_y));
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(min_aoi_x, max_aoi_y));
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(max_aoi_x, max_aoi_y));
        
        for partition_idx in relevant_partition_indices {
            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                for wall_entry in partition.all_walls_in_partition.iter() {
                    let wall = wall_entry.value();
                    // Check if wall AABB intersects with AoI AABB
                    if wall.x < max_aoi_x && wall.x + wall.width > min_aoi_x &&
                       wall.y < max_aoi_y && wall.y + wall.height > min_aoi_y {
                        player_aoi.visible_walls.insert(wall.id);
                    }
                }
            }
        }
        
        // Debug logging
        trace!(
            "[AoI Update] Player {}: {} players, {} projectiles (of {} total), {} pickups (of {} active/{} total), {} walls visible", 
            player_id_str,
            player_aoi.visible_players.len(),
            player_aoi.visible_projectiles.len(),
            projectile_count,
            player_aoi.visible_pickups.len(),
            active_pickups,
            total_pickups,
            player_aoi.visible_walls.len()
        );
        
        player_aoi.last_update = Instant::now();
    }

    fn update_player_aoi_v3(&self, player_id: &PlayerID, x: f32, y: f32) {
        // const AOI_RADIUS: f32 = 600.0; // Defined in constants
        const AOI_RADIUS_SQUARED: f32 = AOI_RADIUS * AOI_RADIUS;
        // const AOI_UPDATE_INTERVAL_SECS: f32 = 0.1; // Defined in constants
    
        let player_id_str = player_id.as_str();
        let mut player_aoi_entry = self.player_aois.entry(player_id_str.to_string())
            .or_insert_with(PlayerAoI::new);
    
        if player_aoi_entry.value().last_update.elapsed().as_secs_f32() < AOI_UPDATE_INTERVAL_SECS {
            return;
        }
        let player_aoi = player_aoi_entry.value_mut();
    
        // 1. Visible Players (Already Optimized)
        player_aoi.visible_players.clear();
        let nearby_player_ids = self.spatial_index.query_nearby_players(x, y, AOI_RADIUS); // 
        for other_id_arc in nearby_player_ids {
            if &other_id_arc != player_id {
                player_aoi.visible_players.insert(other_id_arc);
            }
        }
    
        // 2. Visible Projectiles (OPTIMIZED with Spatial Index Query)
        player_aoi.visible_projectiles.clear();
        // Assuming self.spatial_index (or another index) has a method like query_nearby_projectiles:
        // let nearby_projectile_ids = self.projectile_spatial_index.query_nearby_projectiles(x, y, AOI_RADIUS);
        // for proj_id in nearby_projectile_ids {
        //     player_aoi.visible_projectiles.insert(proj_id);
        // }
        //
        // IF NO DEDICATED SPATIAL INDEX for projectiles yet, the fallback is less optimal:
        // This part should be replaced once projectiles are in a spatial index.
        let projectiles_guard = self.projectiles.read(); // 
        for proj in projectiles_guard.iter() {
            let dx = proj.x - x;
            let dy = proj.y - y;
            if (dx * dx + dy * dy) <= AOI_RADIUS_SQUARED {
                player_aoi.visible_projectiles.insert(proj.id); // 
            }
        }
        drop(projectiles_guard);
    
    
        // 3. Visible Pickups (OPTIMIZED with Spatial Index Query)
        player_aoi.visible_pickups.clear();
        // Assuming self.spatial_index (or another index) has a method like query_nearby_pickups:
        // let nearby_pickup_ids = self.pickup_spatial_index.query_nearby_pickups(x, y, AOI_RADIUS);
        // for pickup_id in nearby_pickup_ids {
        //     // Optionally, you might only insert if the pickup is active,
        //     // if the spatial index stores inactive ones too.
        //     // if self.pickups.read().iter().any(|p| p.id == pickup_id && p.is_active) {
        //         player_aoi.visible_pickups.insert(pickup_id);
        //     // }
        // }
        //
        // IF NO DEDICATED SPATIAL INDEX for pickups yet, the fallback:
        let pickups_guard = self.pickups.read(); // 
        for pickup in pickups_guard.iter() {
            if pickup.is_active {
                let dx = pickup.x - x;
                let dy = pickup.y - y;
                if (dx * dx + dy * dy) <= AOI_RADIUS_SQUARED {
                    player_aoi.visible_pickups.insert(pickup.id); // 
                }
            }
        }
        drop(pickups_guard);
    
        // 4. Visible Walls (NEWLY ADDED and OPTIMIZED)
        player_aoi.visible_walls.clear();
        let min_aoi_x = x - AOI_RADIUS;
        let max_aoi_x = x + AOI_RADIUS;
        let min_aoi_y = y - AOI_RADIUS;
        let max_aoi_y = y + AOI_RADIUS;
    
        // Get a set of partition indices that could overlap with the AoI circle.
        // This involves checking corners and center of the AoI bounding box.
        // A more precise way is to find all partitions intersecting the circle,
        // but this is a good approximation for speed.
        let mut relevant_partition_indices = HashSet::new();
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(x, y)); // Center
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(min_aoi_x, min_aoi_y));
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(max_aoi_x, min_aoi_y));
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(min_aoi_x, max_aoi_y));
        relevant_partition_indices.insert(self.world_partition_manager.get_partition_index_for_point(max_aoi_x, max_aoi_y));
    
        for partition_idx in relevant_partition_indices {
            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                // Iterate only walls within this relevant partition
                for wall_entry in partition.all_walls_in_partition.iter() { // for `all_walls_in_partition` field
                    let wall = wall_entry.value();
                    // Broad phase check: wall's AABB vs AoI's AABB
                    if wall.x < max_aoi_x && wall.x + wall.width > min_aoi_x &&
                       wall.y < max_aoi_y && wall.y + wall.height > min_aoi_y {
                        // Optional: More precise check (rect-circle intersection) if needed,
                        // but for sending to client, this might be enough and client culls.
                        // For simplicity, we'll include it if bounding boxes overlap.
                        player_aoi.visible_walls.insert(wall.id);
                    }
                }
            }
        }
    
        player_aoi.last_update = Instant::now(); // 
    }


    /*pub async fn _optimized_game_tick(self: Arc<Self>, delta_time: f32) {
        let server_clone1 = self.clone();
        let server_clone2 = self.clone();
        let server_clone3 = self.clone();
        let server_clone4 = self.clone();

        let input_future = tokio::spawn(async move {
            server_clone1.thread_pools.network_pool.install(|| {
                 block_on(server_clone1.process_network_input());
            });
        });

        let ai_future = tokio::spawn(async move {
            server_clone2.thread_pools.ai_pool.install(|| {
                block_on(server_clone2.run_ai_update());
            });
        });

        let physics_future = tokio::spawn(async move {
            server_clone3.thread_pools.physics_pool.install(|| {
                block_on(server_clone3.run_physics_update(delta_time));
            });
        });

        let game_logic_future = tokio::spawn(async move {
            server_clone4.thread_pools.game_logic_pool.install(|| {
                block_on(server_clone4.run_game_logic_update(delta_time));
            });
        });

        let (input_res, ai_res, physics_res, game_logic_res) =
            tokio::join!(input_future, ai_future, physics_future, game_logic_future);

        if let Err(e) = input_res { error!("Input processing task panicked: {:?}", e); }
        if let Err(e) = ai_res { error!("AI update task panicked: {:?}", e); }
        if let Err(e) = physics_res { error!("Physics update task panicked: {:?}", e); }
        if let Err(e) = game_logic_res { error!("Game logic task panicked: {:?}", e); }

        self.synchronize_state().await;

        let server_clone_broadcast = self.clone();
        tokio::spawn(async move {
            server_clone_broadcast.thread_pools.network_pool.install(|| {
                block_on(server_clone_broadcast.broadcast_world_updates_optimized());
            });
        }).await.unwrap_or_else(|e| error!("Broadcast task panicked: {:?}", e));

        self.frame_counter.fetch_add(1, AtomicOrdering::Relaxed);
    }*/

   
}
