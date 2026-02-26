// massive_game_server/server/src/server/game_loop.rs
use super::instance::MassiveGameServer;
// Removed unused: use crate::network::signaling::{ChatMessage, handle_dc_send_error};
// Removed unused: use crate::flatbuffers_generated::game_protocol as fb;
use std::sync::Arc;
use std::time::{Duration, Instant}; // Removed unused SystemTime, UNIX_EPOCH
use tracing::{error, info, trace, warn};
// Removed unused: use std::collections::VecDeque;
use std::sync::atomic::Ordering as AtomicOrdering;
// Removed unused: use bytes::Bytes;
// Removed unused: use crate::core::types::{PlayerID, PlayerAoI, Vec2, GameEvent, CorePickupType, EventPriority};
// Removed unused: use crate::core::types::EntityId;
// Removed unused: use std::collections::HashSet;
use crate::core::constants::{
    AOI_MAX_VISIBLE_PICKUPS, AOI_MAX_VISIBLE_PLAYERS, AOI_MAX_VISIBLE_PROJECTILES,
    AOI_MAX_VISIBLE_WALLS, AOI_RADIUS, AOI_UPDATE_INTERVAL_SECS,
    AOI_UPDATE_DIVISOR_DEFAULT,
    MOBILE_AOI_MAX_VISIBLE_PLAYERS, MOBILE_AOI_MAX_VISIBLE_PROJECTILES,
    MOBILE_AOI_MAX_VISIBLE_PICKUPS, MOBILE_AOI_MAX_VISIBLE_WALLS,
    WORLD_MAX_X, WORLD_MAX_Y, WORLD_MIN_X, WORLD_MIN_Y,
};
use crate::core::types::PlayerID;
use crate::flatbuffers_generated::game_protocol as fb;
use crate::network::signaling::{next_chat_message_seq, ChatMessage};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

const SHUTDOWN_CHAT_HISTORY_LIMIT: usize = 50;
const SHUTDOWN_CHAT_PLAYER_ID: &str = "__server__";
const SHUTDOWN_CHAT_USERNAME: &str = "Server";
const SHUTDOWN_CHAT_MESSAGE: &str = "Server is shutting down. Please reconnect shortly.";

impl MassiveGameServer {
    async fn notify_players_of_shutdown(self: Arc<Self>) {
        if self.data_channels_map.is_empty() {
            return;
        }

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;
        let shutdown_chat = ChatMessage {
            seq: next_chat_message_seq(),
            player_id: Arc::new(SHUTDOWN_CHAT_PLAYER_ID.to_owned()),
            username: SHUTDOWN_CHAT_USERNAME.to_owned(),
            message: SHUTDOWN_CHAT_MESSAGE.to_owned(),
            timestamp: timestamp_ms,
        };
        {
            let mut chat_q = self.chat_messages_queue.write().await;
            chat_q.push_back(shutdown_chat);
            while chat_q.len() > SHUTDOWN_CHAT_HISTORY_LIMIT {
                chat_q.pop_front();
            }
        }
        {
            let mut match_info = self.match_info.write();
            match_info.match_state = fb::MatchStateType::Ended;
        }

        if timeout(
            Duration::from_millis(350),
            Arc::clone(&self).broadcast_world_updates_optimized(),
        )
        .await
        .is_err()
        {
            warn!("Shutdown broadcast flush timed out.");
        }
    }

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
        let tick_duration = Duration::from_secs_f64(1.0 / self.config.tick_rate as f64);
        // Cap accumulator to prevent spiral-of-death: process at most 3 ticks
        // per iteration so a single long frame cannot snowball into unbounded
        // catch-up work that starves the event loop.
        let max_accumulator = tick_duration * 3;
        let mut bots_spawned = false;

        info!(
            "Game loop started (accumulator mode). Tick rate: {}ms, Delta time: {}s",
            tick_duration.as_millis(),
            delta_time_fixed
        );
        let mut last_logged_quality = self.current_quality_settings();

        // Accumulator-based fixed timestep: instead of relying solely on
        // tokio::time::interval (which can queue up missed ticks and burst
        // them all at once), we measure real elapsed time, feed it into an
        // accumulator, and drain exactly one tick_duration per game tick.
        // This gives consistent dt to the simulation regardless of scheduling
        // jitter or occasional long frames.
        let mut accumulator = Duration::ZERO;
        let mut last_iteration = Instant::now();

        // We still use a short sleep to yield to the tokio runtime between
        // iterations, but the authoritative pacing comes from the accumulator.
        let sleep_duration = tick_duration.mul_f64(0.5).min(Duration::from_millis(4));

        loop {
            // Yield to the runtime so other tasks (networking, I/O) can progress.
            tokio::time::sleep(sleep_duration).await;

            if crate::server::lifecycle::is_shutdown_requested(self.as_ref()) {
                Arc::clone(&self).notify_players_of_shutdown().await;
                info!("Shutdown requested; exiting game loop.");
                break;
            }

            let now = Instant::now();
            let elapsed = now.duration_since(last_iteration);
            last_iteration = now;

            // Feed elapsed wall-clock time into accumulator (clamped to avoid
            // spiral-of-death when a frame takes much longer than expected).
            accumulator += elapsed;
            if accumulator > max_accumulator {
                let dropped = accumulator - max_accumulator;
                if dropped > tick_duration {
                    warn!(
                        "Accumulator overflow: dropping {:?} of simulation time",
                        dropped
                    );
                }
                accumulator = max_accumulator;
            }

            // Process as many fixed-step ticks as the accumulator allows.
            while accumulator >= tick_duration {
                accumulator -= tick_duration;

                let frame_start_time = Instant::now();
                let current_frame = self.frame_counter.load(AtomicOrdering::Relaxed);

                // Spawn bots after 10 frames to let server stabilize
                if !bots_spawned && current_frame == 10 {
                    let initial_bot_count =
                        self.target_bot_count.load(AtomicOrdering::Relaxed) as usize;
                    info!(
                        "Spawning {} bots after server stabilization (frame {})",
                        initial_bot_count, current_frame
                    );
                    self.spawn_initial_bots(initial_bot_count);
                    bots_spawned = true;
                }

                // Log every 60 frames (1 second at 60 FPS)
                if current_frame.is_multiple_of(60) {
                    info!("Game loop running - Frame: {}", current_frame);
                }

                // Process game tick with fixed delta_time
                if let Err(e) = Arc::clone(&self).process_game_tick(delta_time_fixed).await {
                    error!("Game tick failed: {:?}", e);
                    continue; // Don't stop the game loop on error
                }

                self.frame_counter.fetch_add(1, AtomicOrdering::Relaxed);

                // Stamp the epoch time so health checks can detect a stalled loop.
                let tick_epoch_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_millis() as u64;
                self.last_tick_epoch_ms
                    .store(tick_epoch_ms, AtomicOrdering::Relaxed);

                // Record frame metrics
                let frame_time = frame_start_time.elapsed();
                self.record_tick_metrics(frame_time);

                if current_frame.is_multiple_of(120) {
                    let quality = self.current_quality_settings();
                    if quality.delta_skip_modulus != last_logged_quality.delta_skip_modulus
                        || (quality.aoi_radius_scale - last_logged_quality.aoi_radius_scale).abs()
                            > 0.01
                        || (quality.max_projectiles_scale
                            - last_logged_quality.max_projectiles_scale)
                            .abs()
                            > 0.01
                    {
                        info!(
                            "Adaptive quality updated: aoi_scale={:.2}, projectile_scale={:.2}, delta_skip={}",
                            quality.aoi_radius_scale,
                            quality.max_projectiles_scale,
                            quality.delta_skip_modulus
                        );
                        last_logged_quality = quality;
                    }
                }

                if frame_time > tick_duration + Duration::from_millis(5)
                    && current_frame.is_multiple_of(60)
                {
                    warn!("Frame {} took too long: {:?}", current_frame, frame_time);
                }
            }
        }

        info!("Game loop stopped.");
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

    pub async fn synchronize_state(&self, update_aoi: bool) {
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        trace!("[Frame {}] Starting synchronize_state", frame);
        let sync_loop_start = Instant::now();
        // AoI update divisor: configurable via MGS_AOI_UPDATE_DIVISOR env var.
        // Default = 3 ticks (20 Hz at 60 Hz tick rate). Previous default was 6 (10 Hz)
        // which caused visibility glitches for fast-moving entities.
        let aoi_stride = std::env::var("MGS_AOI_UPDATE_DIVISOR")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(AOI_UPDATE_DIVISOR_DEFAULT)
            .max(1);
        let update_aoi_this_frame = update_aoi && frame.is_multiple_of(aoi_stride);

        let mut players_to_update = Vec::with_capacity(self.player_manager.player_count());
        let client_states = self.client_states_map.read();
        self.player_manager
            .for_each_player(|player_id, player_state| {
                let is_connected_client = update_aoi_this_frame
                    && (self.data_channels_map.contains_key(player_id.as_str())
                        || client_states.contains_key(player_id.as_str()));

                self.spatial_index.update_player_position(
                    player_id.clone(),
                    player_state.x,
                    player_state.y,
                );
                let partition_idx = self
                    .world_partition_manager
                    .get_partition_index_for_point(player_state.x, player_state.y);

                players_to_update.push((
                    player_id.clone(),
                    player_state.x,
                    player_state.y,
                    partition_idx,
                    is_connected_client,
                ));
            });
        drop(client_states);

        for (player_id, x, y, partition_idx, is_connected_client) in players_to_update {
            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                let is_newly_entered_partition = !partition.local_players.contains(&player_id);
                partition.update_player_status(&player_id, x, y, is_newly_entered_partition);
            }

            if is_connected_client {
                self.update_player_aoi(&player_id, x, y);
                self.player_last_sync_positions
                    .insert(player_id.clone(), (x, y));
            }
        }

        if update_aoi_this_frame && frame.is_multiple_of(30) {
            self.world_partition_manager.update_all_boundary_snapshots();
        }

        trace!(
            "[Frame {}] Finished synchronize_state in {:?}",
            frame,
            sync_loop_start.elapsed()
        );
    }

    pub fn update_player_aoi(&self, player_id: &PlayerID, x: f32, y: f32) {
        let quality = self.current_quality_settings();
        let effective_aoi_radius = AOI_RADIUS * quality.aoi_radius_scale;
        let effective_aoi_radius_sq = effective_aoi_radius * effective_aoi_radius;

        let player_id_str = player_id.as_str();

        // Determine per-client AoI limits (mobile vs desktop)
        let is_mobile = self
            .client_states_map
            .read()
            .get(player_id_str)
            .map(|cs| cs.is_mobile)
            .unwrap_or(false);
        let max_visible_players = if is_mobile { MOBILE_AOI_MAX_VISIBLE_PLAYERS } else { AOI_MAX_VISIBLE_PLAYERS };
        let max_visible_projectiles = if is_mobile { MOBILE_AOI_MAX_VISIBLE_PROJECTILES } else { AOI_MAX_VISIBLE_PROJECTILES };
        let max_visible_pickups = if is_mobile { MOBILE_AOI_MAX_VISIBLE_PICKUPS } else { AOI_MAX_VISIBLE_PICKUPS };
        let max_visible_walls = if is_mobile { MOBILE_AOI_MAX_VISIBLE_WALLS } else { AOI_MAX_VISIBLE_WALLS };

        let mut player_aoi_entry = self
            .player_aois
            .entry(player_id_str.to_string())
            .or_default();

        if player_aoi_entry.value().last_update.elapsed().as_secs_f32() < AOI_UPDATE_INTERVAL_SECS {
            return;
        }

        let player_aoi = player_aoi_entry.value_mut();

        // Clear previous data
        player_aoi.visible_players.clear();
        player_aoi.visible_projectiles.clear();
        player_aoi.visible_pickups.clear();
        player_aoi.visible_walls.clear();

        let is_spectator = self
            .player_manager
            .get_player_state(player_id)
            .map(|state| state.is_spectator)
            .unwrap_or(false);
        if is_spectator {
            self.player_manager.for_each_player(|other_id, _| {
                if other_id != player_id
                    && player_aoi.visible_players.len() < max_visible_players
                {
                    player_aoi.visible_players.insert(other_id.clone());
                }
            });
            {
                let projectiles = self.projectiles.read();
                for projectile in projectiles.iter().take(max_visible_projectiles) {
                    player_aoi.visible_projectiles.insert(projectile.id);
                }
            }
            {
                let pickups = self.pickups.read();
                for pickup in pickups.iter().take(max_visible_pickups) {
                    if pickup.is_active {
                        player_aoi.visible_pickups.insert(pickup.id);
                    }
                }
            }
            let active_walls = self.wall_spatial_index.query_aabb(
                WORLD_MIN_X,
                WORLD_MIN_Y,
                WORLD_MAX_X,
                WORLD_MAX_Y,
            );
            for wall in active_walls.into_iter().take(max_visible_walls) {
                player_aoi.visible_walls.insert(wall.id);
            }
            player_aoi.last_update = Instant::now();
            return;
        }

        // 1. Update visible players (using spatial index)
        let nearby_player_ids = self
            .spatial_index
            .query_nearby_players(x, y, effective_aoi_radius);
        for other_id_arc in nearby_player_ids
            .into_iter()
            .take(max_visible_players.saturating_add(1))
        {
            if &other_id_arc != player_id {
                player_aoi.visible_players.insert(other_id_arc);
                if player_aoi.visible_players.len() >= max_visible_players {
                    break;
                }
            }
        }

        // 2. Update visible projectiles via spatial index (avoid scanning all projectiles)
        let nearby_projectile_ids =
            self.spatial_index
                .query_nearby_projectiles(x, y, effective_aoi_radius);
        for proj_id in nearby_projectile_ids
            .into_iter()
            .take(max_visible_projectiles)
        {
            player_aoi.visible_projectiles.insert(proj_id);
        }

        // Candidate partitions for map/items within this AoI.
        let mut candidate_partition_indices = Vec::with_capacity(64);
        self.world_partition_manager
            .collect_partition_indices_for_aoi(
                x,
                y,
                effective_aoi_radius,
                &mut candidate_partition_indices,
            );

        // 3. Update visible pickups via partition dynamic object index.
        let mut candidate_pickups = 0usize;
        let mut active_pickups = 0;
        'pickups: for partition_idx in candidate_partition_indices.iter().copied() {
            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                for pickup_entry in partition.dynamic_objects.iter() {
                    candidate_pickups += 1;
                    let pickup = pickup_entry.value();
                    if !pickup.is_active {
                        continue;
                    }

                    active_pickups += 1;
                    let dx = pickup.x - x;
                    let dy = pickup.y - y;
                    if (dx * dx + dy * dy) <= effective_aoi_radius_sq {
                        player_aoi.visible_pickups.insert(pickup.id);
                        if player_aoi.visible_pickups.len() >= max_visible_pickups {
                            break 'pickups;
                        }
                    }
                }
            }
        }

        // 4. Update visible walls (check overlapping partitions)
        let min_aoi_x = x - effective_aoi_radius;
        let max_aoi_x = x + effective_aoi_radius;
        let min_aoi_y = y - effective_aoi_radius;
        let max_aoi_y = y + effective_aoi_radius;

        let candidate_walls_query = self
            .wall_spatial_index
            .query_aabb(min_aoi_x, min_aoi_y, max_aoi_x, max_aoi_y);
        let candidate_walls = candidate_walls_query.len();
        for wall in candidate_walls_query
            .into_iter()
            .take(max_visible_walls)
        {
            if wall.is_destructible && wall.current_health <= 0 {
                continue;
            }
            player_aoi.visible_walls.insert(wall.id);
        }

        // Debug logging
        trace!(
            "[AoI Update] Player {}: {} players, {} projectiles, {} pickups ({} active/{} candidates), {} walls ({} candidates)",
            player_id_str,
            player_aoi.visible_players.len(),
            player_aoi.visible_projectiles.len(),
            player_aoi.visible_pickups.len(),
            active_pickups,
            candidate_pickups,
            player_aoi.visible_walls.len(),
            candidate_walls
        );

        player_aoi.last_update = Instant::now();
    }

    #[allow(dead_code)]
    fn update_player_aoi_v3(&self, player_id: &PlayerID, x: f32, y: f32) {
        // const AOI_RADIUS: f32 = 600.0; // Defined in constants
        const AOI_RADIUS_SQUARED: f32 = AOI_RADIUS * AOI_RADIUS;
        // const AOI_UPDATE_INTERVAL_SECS: f32 = 0.1; // Defined in constants

        let player_id_str = player_id.as_str();
        let mut player_aoi_entry = self
            .player_aois
            .entry(player_id_str.to_string())
            .or_default();

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
        relevant_partition_indices.insert(
            self.world_partition_manager
                .get_partition_index_for_point(x, y),
        ); // Center
        relevant_partition_indices.insert(
            self.world_partition_manager
                .get_partition_index_for_point(min_aoi_x, min_aoi_y),
        );
        relevant_partition_indices.insert(
            self.world_partition_manager
                .get_partition_index_for_point(max_aoi_x, min_aoi_y),
        );
        relevant_partition_indices.insert(
            self.world_partition_manager
                .get_partition_index_for_point(min_aoi_x, max_aoi_y),
        );
        relevant_partition_indices.insert(
            self.world_partition_manager
                .get_partition_index_for_point(max_aoi_x, max_aoi_y),
        );

        for partition_idx in relevant_partition_indices {
            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                // Iterate only walls within this relevant partition
                for wall_entry in partition.all_walls_in_partition.iter() {
                    // for `all_walls_in_partition` field
                    let wall = wall_entry.value();
                    // Broad phase check: wall's AABB vs AoI's AABB
                    if wall.x < max_aoi_x
                        && wall.x + wall.width > min_aoi_x
                        && wall.y < max_aoi_y
                        && wall.y + wall.height > min_aoi_y
                    {
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
