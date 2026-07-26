// massive_game_server/server/src/server/game_loop.rs
use super::instance::MassiveGameServer;
// Removed unused: use crate::network::signaling::{ChatMessage, handle_dc_send_error};
// Removed unused: use crate::flatbuffers_generated::game_protocol as fb;
use std::sync::{Arc, OnceLock};
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
    AOI_MAX_VISIBLE_WALLS, AOI_RADIUS, AOI_UPDATE_DIVISOR_DEFAULT, AOI_UPDATE_INTERVAL_SECS,
    MOBILE_AOI_MAX_VISIBLE_PICKUPS, MOBILE_AOI_MAX_VISIBLE_PLAYERS,
    MOBILE_AOI_MAX_VISIBLE_PROJECTILES, MOBILE_AOI_MAX_VISIBLE_WALLS, WORLD_MAX_X, WORLD_MAX_Y,
    WORLD_MIN_X, WORLD_MIN_Y,
};
use crate::core::types::PlayerID;
use crate::flatbuffers_generated::game_protocol as fb;
use crate::network::signaling::{next_chat_message_seq, ChatMessage};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

const SHUTDOWN_CHAT_PLAYER_ID: &str = "__server__";
const SHUTDOWN_CHAT_USERNAME: &str = "Server";
const SHUTDOWN_CHAT_MESSAGE: &str = "Server is shutting down. Please reconnect shortly.";
const AOI_UPDATE_DIVISOR_MAX: u64 = 60;

fn parse_aoi_update_divisor(raw: Option<&str>) -> u64 {
    raw.and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(AOI_UPDATE_DIVISOR_DEFAULT)
        .clamp(1, AOI_UPDATE_DIVISOR_MAX)
}

fn cap_accumulator(accumulator: Duration, max_accumulator: Duration) -> (Duration, Duration) {
    if accumulator > max_accumulator {
        (max_accumulator, accumulator - max_accumulator)
    } else {
        (accumulator, Duration::ZERO)
    }
}

fn initial_bot_spawn_deficit(
    target_bot_count: usize,
    effective_bot_capacity: usize,
    current_bot_count: usize,
) -> usize {
    target_bot_count
        .min(effective_bot_capacity)
        .saturating_sub(current_bot_count)
}

fn cached_aoi_update_divisor() -> u64 {
    static AOI_UPDATE_DIVISOR: OnceLock<u64> = OnceLock::new();
    *AOI_UPDATE_DIVISOR.get_or_init(|| {
        parse_aoi_update_divisor(std::env::var("MGS_AOI_UPDATE_DIVISOR").ok().as_deref())
    })
}

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
            player_id: Arc::from(SHUTDOWN_CHAT_PLAYER_ID.to_owned()),
            username: SHUTDOWN_CHAT_USERNAME.to_owned(),
            message: SHUTDOWN_CHAT_MESSAGE.to_owned(),
            timestamp: timestamp_ms,
        };
        {
            let mut chat_q = self.chat_messages_queue.write().await;
            chat_q.push_back(shutdown_chat);
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

    pub async fn run_game_loop(self: Arc<Self>) {
        let tick_rate_hz = self.config.tick_rate.max(1);
        if tick_rate_hz != self.config.tick_rate {
            warn!(
                "Invalid tick_rate={} detected at runtime; clamping to {} Hz.",
                self.config.tick_rate, tick_rate_hz
            );
        }
        let delta_time_fixed = 1.0 / tick_rate_hz as f32;
        let tick_duration = Duration::from_secs_f64(1.0 / tick_rate_hz as f64);
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
            let (capped_accumulator, dropped) = cap_accumulator(accumulator, max_accumulator);
            if dropped > tick_duration {
                warn!(
                    "Accumulator overflow: dropping {:?} of simulation time",
                    dropped
                );
            }
            accumulator = capped_accumulator;

            // Process as many fixed-step ticks as the accumulator allows.
            while accumulator >= tick_duration {
                accumulator -= tick_duration;

                let frame_start_time = Instant::now();
                let current_frame = self.frame_counter.load(AtomicOrdering::Relaxed);

                // Spawn bots after 10 frames to let server stabilize
                if !bots_spawned && current_frame == 10 {
                    let target_bot_count =
                        self.target_bot_count.load(AtomicOrdering::Relaxed) as usize;
                    let current_bot_count = self.bot_players.len();
                    let initial_bot_count = initial_bot_spawn_deficit(
                        target_bot_count,
                        self.effective_bot_capacity(),
                        current_bot_count,
                    );
                    info!(
                        "Spawning {} missing bots after server stabilization (frame {}, current={}, target={})",
                        initial_bot_count, current_frame, current_bot_count, target_bot_count
                    );
                    if initial_bot_count > 0 {
                        self.spawn_initial_bots(initial_bot_count);
                    }
                    bots_spawned = true;
                }

                // Log every 60 frames (1 second at 60 FPS)
                if current_frame.is_multiple_of(60) {
                    info!("Game loop running - Frame: {}", current_frame);
                }

                // Process game tick with fixed delta_time.
                // Stage-level CPU work is offloaded inside process_game_tick.
                let tick_result = Arc::clone(&self).process_game_tick(delta_time_fixed).await;

                if let Err(e) = tick_result {
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

    pub async fn synchronize_state(&self, update_aoi: bool) {
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        trace!("[Frame {}] Starting synchronize_state", frame);
        let sync_loop_start = Instant::now();
        // AoI update divisor is read once from the environment at startup.
        let aoi_stride = cached_aoi_update_divisor();
        let update_aoi_this_frame = update_aoi && frame.is_multiple_of(aoi_stride);
        let connected_client_ids: HashSet<String> = if update_aoi_this_frame {
            let mut connected = HashSet::with_capacity(
                self.data_channels_map.len() + self.client_states_map.read().len(),
            );
            connected.extend(
                self.data_channels_map
                    .iter()
                    .map(|entry| entry.key().clone()),
            );
            {
                let client_states = self.client_states_map.read();
                connected.extend(client_states.keys().cloned());
            }
            connected
        } else {
            HashSet::new()
        };

        let mut players_to_update = Vec::with_capacity(self.player_manager.player_count());
        self.player_manager
            .for_each_player(|player_id, player_state| {
                let is_connected_client =
                    update_aoi_this_frame && connected_client_ids.contains(player_id.as_ref());

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

        for (player_id, x, y, partition_idx, is_connected_client) in players_to_update {
            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                let is_newly_entered_partition = !partition.local_players.contains(&player_id);
                partition.update_player_status(&player_id, x, y, is_newly_entered_partition);
            }

            if is_connected_client {
                self.update_player_aoi(&player_id, x, y);
                self.snapshots
                    .player_last_sync_positions
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

        let player_id_str = player_id.as_ref();

        // Determine per-client AoI limits (mobile vs desktop)
        let is_mobile = self
            .client_states_map
            .read()
            .get(player_id_str)
            .map(|cs| cs.is_mobile)
            .unwrap_or(false);
        let max_visible_players = if is_mobile {
            MOBILE_AOI_MAX_VISIBLE_PLAYERS
        } else {
            AOI_MAX_VISIBLE_PLAYERS
        };
        let max_visible_projectiles = if is_mobile {
            MOBILE_AOI_MAX_VISIBLE_PROJECTILES
        } else {
            AOI_MAX_VISIBLE_PROJECTILES
        };
        let max_visible_pickups = if is_mobile {
            MOBILE_AOI_MAX_VISIBLE_PICKUPS
        } else {
            AOI_MAX_VISIBLE_PICKUPS
        };
        let max_visible_walls = if is_mobile {
            MOBILE_AOI_MAX_VISIBLE_WALLS
        } else {
            AOI_MAX_VISIBLE_WALLS
        };

        let should_skip_update = self
            .player_aois
            .get(player_id_str)
            .map(|aoi| aoi.last_update.elapsed().as_secs_f32() < AOI_UPDATE_INTERVAL_SECS)
            .unwrap_or(false);
        if should_skip_update {
            return;
        }

        let mut next_aoi = crate::core::types::PlayerAoI::new();

        let is_spectator = self
            .player_manager
            .get_player_state(player_id)
            .map(|state| state.is_spectator)
            .unwrap_or(false);
        if is_spectator {
            self.player_manager.for_each_player(|other_id, _| {
                if other_id != player_id && next_aoi.visible_players.len() < max_visible_players {
                    next_aoi.visible_players.insert(other_id.clone());
                }
            });
            let projectile_snapshot = self.snapshots.projectile_soa_snapshot.load();
            for projectile in projectile_snapshot
                .states()
                .iter()
                .take(max_visible_projectiles)
            {
                next_aoi.visible_projectiles.insert(projectile.id);
            }
            let pickup_snapshot = self.snapshots.pickup_soa_snapshot.load();
            for pickup in pickup_snapshot.states().iter().take(max_visible_pickups) {
                if pickup.is_active {
                    next_aoi.visible_pickups.insert(pickup.id);
                }
            }
            let active_walls = self.wall_spatial_index.query_aabb(
                WORLD_MIN_X,
                WORLD_MIN_Y,
                WORLD_MAX_X,
                WORLD_MAX_Y,
            );
            for wall in active_walls.into_iter().take(max_visible_walls) {
                next_aoi.visible_walls.insert(wall.id);
            }
            next_aoi.last_update = Instant::now();
            self.player_aois.insert(player_id_str.to_owned(), next_aoi);
            return;
        }

        // 1. Update visible players (using spatial index)
        let mut nearby_players =
            self.spatial_index
                .query_nearby_players_with_positions(x, y, effective_aoi_radius);
        nearby_players.sort_by(|(_, ax, ay), (_, bx, by)| {
            let a_dist_sq = (*ax - x).powi(2) + (*ay - y).powi(2);
            let b_dist_sq = (*bx - x).powi(2) + (*by - y).powi(2);
            a_dist_sq.total_cmp(&b_dist_sq)
        });
        for (other_id_arc, _, _) in nearby_players.into_iter() {
            if &other_id_arc != player_id {
                next_aoi.visible_players.insert(other_id_arc);
                if next_aoi.visible_players.len() >= max_visible_players {
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
            next_aoi.visible_projectiles.insert(proj_id);
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
                        next_aoi.visible_pickups.insert(pickup.id);
                        if next_aoi.visible_pickups.len() >= max_visible_pickups {
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

        let mut candidate_walls_query = self
            .wall_spatial_index
            .query_aabb(min_aoi_x, min_aoi_y, max_aoi_x, max_aoi_y);
        let candidate_walls = candidate_walls_query.len();
        // Prioritize the nearest walls so the AoI cap keeps the walls that
        // actually surround the player (the mobile cap is much lower than the
        // desktop one, so an arbitrary selection drops nearby walls).
        candidate_walls_query.sort_by(|a, b| {
            let a_dist_sq = (a.x - x).powi(2) + (a.y - y).powi(2);
            let b_dist_sq = (b.x - x).powi(2) + (b.y - y).powi(2);
            a_dist_sq.total_cmp(&b_dist_sq)
        });
        for wall in candidate_walls_query.into_iter().take(max_visible_walls) {
            if wall.is_destructible && wall.current_health <= 0 {
                continue;
            }
            next_aoi.visible_walls.insert(wall.id);
        }

        // Debug logging
        trace!(
            "[AoI Update] Player {}: {} players, {} projectiles, {} pickups ({} active/{} candidates), {} walls ({} candidates)",
            player_id_str,
            next_aoi.visible_players.len(),
            next_aoi.visible_projectiles.len(),
            next_aoi.visible_pickups.len(),
            active_pickups,
            candidate_pickups,
            next_aoi.visible_walls.len(),
            candidate_walls
        );

        next_aoi.last_update = Instant::now();
        self.player_aois.insert(player_id_str.to_owned(), next_aoi);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_aoi_update_divisor_defaults_and_clamps() {
        assert_eq!(parse_aoi_update_divisor(None), AOI_UPDATE_DIVISOR_DEFAULT);
        assert_eq!(
            parse_aoi_update_divisor(Some("invalid")),
            AOI_UPDATE_DIVISOR_DEFAULT
        );
        assert_eq!(parse_aoi_update_divisor(Some("0")), 1);
        assert_eq!(
            parse_aoi_update_divisor(Some("999")),
            AOI_UPDATE_DIVISOR_MAX
        );
        assert_eq!(parse_aoi_update_divisor(Some("6")), 6);
    }

    #[test]
    fn cap_accumulator_preserves_budgeted_elapsed_time() {
        let (capped, dropped) =
            cap_accumulator(Duration::from_millis(24), Duration::from_millis(48));
        assert_eq!(capped, Duration::from_millis(24));
        assert_eq!(dropped, Duration::ZERO);
    }

    #[test]
    fn cap_accumulator_reports_overflow_when_elapsed_time_spikes() {
        let (capped, dropped) =
            cap_accumulator(Duration::from_millis(73), Duration::from_millis(48));
        assert_eq!(capped, Duration::from_millis(48));
        assert_eq!(dropped, Duration::from_millis(25));
    }

    #[test]
    fn initial_bot_spawn_respects_match_capacity_and_existing_population() {
        assert_eq!(initial_bot_spawn_deficit(20, 14, 0), 14);
        assert_eq!(initial_bot_spawn_deficit(20, 14, 14), 0);
        assert_eq!(initial_bot_spawn_deficit(14, 14, 9), 5);
        assert_eq!(initial_bot_spawn_deficit(20, 0, 0), 0);
    }
}
