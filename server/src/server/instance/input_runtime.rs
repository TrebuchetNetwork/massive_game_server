use super::*;
use crate::core::deterministic_rng::DeterministicRng;

const DEFAULT_ANTI_CHEAT_KICK_THRESHOLD: u32 = 8;

fn anti_cheat_kick_threshold() -> Option<u32> {
    static KICK_THRESHOLD: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *KICK_THRESHOLD.get_or_init(|| {
        let parsed = std::env::var("MGS_ANTI_CHEAT_KICK_THRESHOLD")
            .ok()
            .and_then(|raw| raw.trim().parse::<u32>().ok())
            .unwrap_or(DEFAULT_ANTI_CHEAT_KICK_THRESHOLD);
        if parsed == 0 {
            tracing::warn!(
                "MGS_ANTI_CHEAT_KICK_THRESHOLD=0 disables speed-hack auto-kicks. Use only for local debugging."
            );
            None
        } else {
            Some(parsed.max(1))
        }
    })
}

#[inline]
fn weapon_base_spread_angle_rad(weapon: ServerWeaponType) -> f32 {
    match weapon {
        ServerWeaponType::Pistol => PISTOL_BASE_SPREAD_ANGLE_RAD,
        ServerWeaponType::Rifle => RIFLE_BASE_SPREAD_ANGLE_RAD,
        ServerWeaponType::Sniper => SNIPER_BASE_SPREAD_ANGLE_RAD,
        _ => 0.0,
    }
}

#[inline]
fn weapon_spread_bloom_per_shot_rad(weapon: ServerWeaponType) -> f32 {
    match weapon {
        ServerWeaponType::Pistol => PISTOL_SPREAD_BLOOM_PER_SHOT_RAD,
        ServerWeaponType::Shotgun => SHOTGUN_SPREAD_BLOOM_PER_SHOT_RAD,
        ServerWeaponType::Rifle => RIFLE_SPREAD_BLOOM_PER_SHOT_RAD,
        ServerWeaponType::Sniper => SNIPER_SPREAD_BLOOM_PER_SHOT_RAD,
        ServerWeaponType::Melee => 0.0,
    }
}

#[inline]
fn weapon_spread_bloom_cap_rad(weapon: ServerWeaponType) -> f32 {
    match weapon {
        ServerWeaponType::Pistol => PISTOL_SPREAD_BLOOM_MAX_RAD,
        ServerWeaponType::Shotgun => SHOTGUN_SPREAD_BLOOM_MAX_RAD,
        ServerWeaponType::Rifle => RIFLE_SPREAD_BLOOM_MAX_RAD,
        ServerWeaponType::Sniper => SNIPER_SPREAD_BLOOM_MAX_RAD,
        ServerWeaponType::Melee => 0.0,
    }
}

impl MassiveGameServer {
    pub(super) fn prune_runtime_tracking_state(&self) {
        self.runtime_tracking
            .player_position_history
            .retain(|player_id, _| self.player_manager.get_player_state(player_id).is_some());
        self.runtime_tracking
            .aim_anomaly_states
            .retain(|player_id, _| self.player_manager.get_player_state(player_id).is_some());
        self.queue_state.direct_packets.retain(|peer_id, _| {
            let player_id = self.player_manager.id_pool.get_or_create(peer_id.as_ref());
            self.player_manager.get_player_state(&player_id).is_some()
        });
        self.prune_match_runtime_state();
        if self.commander_mode_enabled {
            self.refresh_commander_runtime_state(self.get_server_timestamp_ms());
        }
    }

    pub(super) fn refresh_commander_runtime_state(&self, now_ms: u64) {
        if !self.commander_mode_enabled {
            return;
        }

        let mut preferred_human_commander: HashMap<u8, PlayerID> = HashMap::new();
        let mut fallback_commander: HashMap<u8, PlayerID> = HashMap::new();
        self.player_manager
            .for_each_player(|player_id, player_state| {
                if player_state.is_spectator
                    || !(player_state.team_id == 1 || player_state.team_id == 2)
                {
                    return;
                }
                fallback_commander
                    .entry(player_state.team_id)
                    .or_insert_with(|| player_id.clone());
                if !self.bot_players.contains_key(player_id) {
                    preferred_human_commander
                        .entry(player_state.team_id)
                        .or_insert_with(|| player_id.clone());
                }
            });

        let mut runtime = self.commander_runtime_state.write();
        for team_id in [1u8, 2u8] {
            let mut clear_team_waypoints = false;
            if let Some(waypoints) = runtime.team_waypoints.get_mut(&team_id) {
                while waypoints
                    .front()
                    .is_some_and(|waypoint| waypoint.expires_at_ms <= now_ms)
                {
                    let _ = waypoints.pop_front();
                }
                if waypoints.is_empty() {
                    clear_team_waypoints = true;
                }
            }
            if clear_team_waypoints {
                runtime.team_waypoints.remove(&team_id);
                runtime.team_attack_bias.remove(&team_id);
            }

            let existing_commander =
                runtime
                    .team_commanders
                    .get(&team_id)
                    .cloned()
                    .and_then(|candidate| {
                        self.player_manager
                            .get_player_state(&candidate)
                            .and_then(|state| {
                                if !state.is_spectator && state.team_id == team_id {
                                    Some(candidate)
                                } else {
                                    None
                                }
                            })
                    });

            if let Some(commander_id) = existing_commander {
                runtime.team_commanders.insert(team_id, commander_id);
                continue;
            }

            if let Some(new_commander) = preferred_human_commander
                .get(&team_id)
                .cloned()
                .or_else(|| fallback_commander.get(&team_id).cloned())
            {
                runtime.team_commanders.insert(team_id, new_commander);
            } else {
                runtime.team_commanders.remove(&team_id);
                runtime.team_waypoints.remove(&team_id);
                runtime.team_attack_bias.remove(&team_id);
                runtime.team_supply_drop_ready_ms.remove(&team_id);
            }
        }
    }

    fn is_player_team_commander(&self, player_id: &PlayerID, team_id: u8) -> bool {
        if !self.commander_mode_enabled || !(team_id == 1 || team_id == 2) {
            return false;
        }
        self.commander_runtime_state
            .read()
            .team_commanders
            .get(&team_id)
            .is_some_and(|commander_id| commander_id == player_id)
    }

    fn commander_attack_bias_for_waypoint(team_id: u8, position: Vec2) -> f32 {
        let own_base = Self::get_flag_base_position(team_id);
        let enemy_base = Self::get_flag_base_position(if team_id == 1 { 2 } else { 1 });
        let dist_to_own_sq = (position.x - own_base.x).powi(2) + (position.y - own_base.y).powi(2);
        let dist_to_enemy_sq =
            (position.x - enemy_base.x).powi(2) + (position.y - enemy_base.y).powi(2);

        if dist_to_enemy_sq + 6_400.0 < dist_to_own_sq {
            0.75
        } else if dist_to_own_sq + 6_400.0 < dist_to_enemy_sq {
            0.35
        } else {
            0.55
        }
    }

    fn spawn_commander_supply_drop(&self, team_id: u8, center: Vec2) -> usize {
        let seed = self.frame_counter.load(AtomicOrdering::Relaxed)
            ^ ((team_id as u64) << 48)
            ^ ((center.x.to_bits() as u64) << 16)
            ^ (center.y.to_bits() as u64);
        let mut rng = DeterministicRng::new(seed);
        let pickup_types = [
            CorePickupType::Health,
            CorePickupType::Ammo,
            CorePickupType::Shield,
            CorePickupType::DamageBoost,
            CorePickupType::SpeedBoost,
            CorePickupType::WeaponCrate(ServerWeaponType::Shotgun),
        ];

        let mut spawned = Vec::with_capacity(COMMANDER_SUPPLY_DROP_PICKUPS);
        {
            let mut pickups = self.pickups.write();
            for idx in 0..COMMANDER_SUPPLY_DROP_PICKUPS {
                let angle = rng.gen_range_f32(0.0, 2.0 * std::f32::consts::PI);
                let radius = rng.gen_range_f32(6.0, 45.0);
                let spawn_x =
                    (center.x + radius * angle.cos()).clamp(WORLD_MIN_X + 40.0, WORLD_MAX_X - 40.0);
                let spawn_y =
                    (center.y + radius * angle.sin()).clamp(WORLD_MIN_Y + 40.0, WORLD_MAX_Y - 40.0);
                let overlaps_wall = self.wall_spatial_index.any_radius(
                    spawn_x,
                    spawn_y,
                    PLAYER_RADIUS + 8.0,
                    |wall| {
                        let closest_x = spawn_x.clamp(wall.x, wall.x + wall.width);
                        let closest_y = spawn_y.clamp(wall.y, wall.y + wall.height);
                        let dx = spawn_x - closest_x;
                        let dy = spawn_y - closest_y;
                        dx * dx + dy * dy < PLAYER_RADIUS * PLAYER_RADIUS
                    },
                );
                if overlaps_wall {
                    continue;
                }

                let pickup = Pickup::new(
                    generate_entity_id(),
                    spawn_x,
                    spawn_y,
                    pickup_types[idx % pickup_types.len()].clone(),
                );
                pickups.push(pickup.clone());
                spawned.push(pickup);
            }
        }

        for pickup in &spawned {
            self.upsert_pickup_in_partition_index(pickup);
        }
        spawned.len()
    }

    fn register_commander_waypoint(
        &self,
        commander_id: &PlayerID,
        team_id: u8,
        position: Vec2,
        now_ms: u64,
    ) {
        if !self.commander_mode_enabled || !(team_id == 1 || team_id == 2) {
            return;
        }

        let attack_bias = Self::commander_attack_bias_for_waypoint(team_id, position);
        let mut should_spawn_supply_drop = false;
        {
            let mut runtime = self.commander_runtime_state.write();
            let waypoints = runtime.team_waypoints.entry(team_id).or_default();
            waypoints.push_back(CommanderWaypoint {
                position,
                expires_at_ms: now_ms.saturating_add(COMMANDER_WAYPOINT_TTL_MS),
            });
            while waypoints.len() > COMMANDER_MAX_WAYPOINTS_PER_TEAM {
                let _ = waypoints.pop_front();
            }
            runtime.team_attack_bias.insert(team_id, attack_bias);

            let ready_at = runtime
                .team_supply_drop_ready_ms
                .get(&team_id)
                .copied()
                .unwrap_or(0);
            if now_ms >= ready_at {
                runtime.team_supply_drop_ready_ms.insert(
                    team_id,
                    now_ms.saturating_add(COMMANDER_SUPPLY_DROP_COOLDOWN_MS),
                );
                should_spawn_supply_drop = true;
            }
        }

        if should_spawn_supply_drop {
            let spawned = self.spawn_commander_supply_drop(team_id, position);
            info!(
                "[Commander] Team {} commander {} set waypoint and triggered supply drop ({} pickups).",
                team_id,
                commander_id.as_ref(),
                spawned
            );
        }
    }

    pub fn commander_primary_waypoint_for_team(&self, team_id: u8) -> Option<Vec2> {
        if !self.commander_mode_enabled || !(team_id == 1 || team_id == 2) {
            return None;
        }
        let now_ms = self.get_server_timestamp_ms();
        let mut runtime = self.commander_runtime_state.write();
        let mut remove_waypoints = false;
        let waypoint = if let Some(waypoints) = runtime.team_waypoints.get_mut(&team_id) {
            while waypoints
                .front()
                .is_some_and(|waypoint| waypoint.expires_at_ms <= now_ms)
            {
                let _ = waypoints.pop_front();
            }
            if waypoints.is_empty() {
                remove_waypoints = true;
                None
            } else {
                waypoints.back().map(|waypoint| waypoint.position)
            }
        } else {
            None
        };
        if remove_waypoints {
            runtime.team_waypoints.remove(&team_id);
            runtime.team_attack_bias.remove(&team_id);
        }
        waypoint
    }

    pub fn commander_attack_bias_for_team(&self, team_id: u8) -> Option<f32> {
        if !self.commander_mode_enabled || !(team_id == 1 || team_id == 2) {
            return None;
        }
        let now_ms = self.get_server_timestamp_ms();
        let mut runtime = self.commander_runtime_state.write();
        let mut remove_waypoints = false;
        if let Some(waypoints) = runtime.team_waypoints.get_mut(&team_id) {
            while waypoints
                .front()
                .is_some_and(|waypoint| waypoint.expires_at_ms <= now_ms)
            {
                let _ = waypoints.pop_front();
            }
            if waypoints.is_empty() {
                remove_waypoints = true;
            }
        }
        if remove_waypoints {
            runtime.team_waypoints.remove(&team_id);
            runtime.team_attack_bias.remove(&team_id);
        }
        runtime.team_attack_bias.get(&team_id).copied()
    }

    pub fn commander_id_for_team(&self, team_id: u8) -> Option<PlayerID> {
        if !self.commander_mode_enabled || !(team_id == 1 || team_id == 2) {
            return None;
        }
        self.commander_runtime_state
            .read()
            .team_commanders
            .get(&team_id)
            .cloned()
    }

    pub(super) fn record_player_position_sample(
        &self,
        player_id: &PlayerID,
        timestamp_ms: u64,
        x: f32,
        y: f32,
    ) {
        let mut history = self
            .runtime_tracking
            .player_position_history
            .entry(player_id.clone())
            .or_insert_with(|| InterpolationBuffer::new(MAX_POSITION_HISTORY_SAMPLES));
        history.push(timestamp_ms, Vec2::new(x, y));
    }
    pub(super) fn get_rewound_player_position(
        &self,
        player_id: &PlayerID,
        target_timestamp_ms: u64,
    ) -> Option<(f32, f32)> {
        let history = self
            .runtime_tracking
            .player_position_history
            .get(player_id)?;
        let sample = history.sample_at(target_timestamp_ms)?;
        Some((sample.x, sample.y))
    }

    fn apply_aim_anomaly_detection(
        &self,
        player_id: &PlayerID,
        input: &PlayerInputData,
        player_state: &mut PlayerState,
        now: Instant,
    ) {
        let mut entry = self
            .runtime_tracking
            .aim_anomaly_states
            .entry(player_id.clone())
            .or_insert_with(|| AimAnomalyState {
                last_rotation: input.rotation,
                last_input_timestamp_ms: input.timestamp,
                suspicion_score: 0.0,
                last_warned_at: now,
            });

        let dt_ms = input
            .timestamp
            .saturating_sub(entry.last_input_timestamp_ms)
            .max(1);
        let dt_sec = dt_ms as f32 / 1000.0;
        let rotation_delta = shortest_angle_diff_radians(input.rotation, entry.last_rotation).abs();
        let rotation_speed = rotation_delta / dt_sec.max(0.001);

        if input.shooting && rotation_speed > AIMBOT_SUSPICION_ROTATION_RAD_PER_SEC {
            let overshoot = rotation_speed / AIMBOT_SUSPICION_ROTATION_RAD_PER_SEC - 1.0;
            entry.suspicion_score += AIMBOT_SUSPICION_SHOT_WEIGHT + overshoot * 0.2;
        } else {
            entry.suspicion_score =
                (entry.suspicion_score - AIMBOT_SUSPICION_DECAY_PER_SEC * dt_sec).max(0.0);
        }

        if entry.suspicion_score >= AIMBOT_SUSPICION_THRESHOLD
            && now.duration_since(entry.last_warned_at) >= Duration::from_secs(2)
        {
            entry.last_warned_at = now;
            player_state.violation_count = player_state.violation_count.saturating_add(1);
            warn!(
                "[{}]: Aim anomaly detected (rotation_speed={:.2} rad/s, suspicion={:.2}).",
                player_id.as_ref(),
                rotation_speed,
                entry.suspicion_score
            );
        }

        entry.last_rotation = input.rotation;
        entry.last_input_timestamp_ms = input.timestamp;
    }
    pub(super) fn sync_pickups_to_partition_index(
        world_partition_manager: &Arc<WorldPartitionManager>,
        pickups: &[Pickup],
    ) {
        for partition in world_partition_manager.get_partitions_for_processing() {
            partition.dynamic_objects.clear();
        }

        for pickup in pickups {
            let partition_idx =
                world_partition_manager.get_partition_index_for_point(pickup.x, pickup.y);
            if let Some(partition) = world_partition_manager.get_partition(partition_idx) {
                partition.add_dynamic_object(pickup.clone());
            }
        }
    }
    pub(super) fn upsert_pickup_in_partition_index(&self, pickup: &Pickup) {
        let partition_idx = self
            .world_partition_manager
            .get_partition_index_for_point(pickup.x, pickup.y);
        if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
            partition.add_dynamic_object(pickup.clone());
        }
    }
    pub(super) fn generate_initial_pickups(
        map_walls: &[Wall],
        target_players: usize,
        seed: u64,
    ) -> Vec<Pickup> {
        let mut pickups: Vec<Pickup> = Vec::new();
        let mut rng = StdRng::seed_from_u64(seed);
        let pickup_types = [
            CorePickupType::Health,
            CorePickupType::Ammo,
            CorePickupType::WeaponCrate(ServerWeaponType::Shotgun),
            CorePickupType::WeaponCrate(ServerWeaponType::Rifle),
            CorePickupType::SpeedBoost,
            CorePickupType::DamageBoost,
            CorePickupType::Shield,
            CorePickupType::WeaponCrate(ServerWeaponType::Sniper),
        ];

        let strategic_locations = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(WORLD_MIN_X / 2.0, WORLD_MIN_Y / 2.0),
            Vec2::new(WORLD_MAX_X / 2.0, WORLD_MIN_Y / 2.0),
            Vec2::new(WORLD_MIN_X / 2.0, WORLD_MAX_Y / 2.0),
            Vec2::new(WORLD_MAX_X / 2.0, WORLD_MAX_Y / 2.0),
            Vec2::new(WORLD_MIN_X + 250.0, 0.0),
            Vec2::new(WORLD_MAX_X - 250.0, 0.0),
        ];
        let strategic_anchor_count = strategic_locations.len();
        let mut spawn_anchors = strategic_locations;
        let extra_anchor_count = (target_players / 12).clamp(0, 32);
        for _ in 0..extra_anchor_count {
            spawn_anchors.push(Vec2::new(
                rng.gen_range(WORLD_MIN_X + 120.0..WORLD_MAX_X - 120.0),
                rng.gen_range(WORLD_MIN_Y + 120.0..WORLD_MAX_Y - 120.0),
            ));
        }
        let desired_pickups = (8 + (target_players / 8)).clamp(8, 48);
        const PICKUP_SPACING_MIN: f32 = 70.0;
        const PICKUP_SPACING_MIN_SQ: f32 = PICKUP_SPACING_MIN * PICKUP_SPACING_MIN;
        const PICKUP_WALL_GRID_CELL_SIZE: f32 = 220.0;

        // Build a coarse spatial index once so each spawn attempt only checks
        // nearby walls instead of scanning the entire map wall list.
        let mut wall_buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (wall_idx, wall) in map_walls.iter().enumerate() {
            if wall.is_destructible && wall.current_health <= 0 {
                continue;
            }
            let min_x = ((wall.x - PICKUP_COLLECTION_RADIUS - WORLD_MIN_X)
                / PICKUP_WALL_GRID_CELL_SIZE)
                .floor() as i32;
            let max_x = ((wall.x + wall.width + PICKUP_COLLECTION_RADIUS - WORLD_MIN_X)
                / PICKUP_WALL_GRID_CELL_SIZE)
                .floor() as i32;
            let min_y = ((wall.y - PICKUP_COLLECTION_RADIUS - WORLD_MIN_Y)
                / PICKUP_WALL_GRID_CELL_SIZE)
                .floor() as i32;
            let max_y = ((wall.y + wall.height + PICKUP_COLLECTION_RADIUS - WORLD_MIN_Y)
                / PICKUP_WALL_GRID_CELL_SIZE)
                .floor() as i32;

            for gy in min_y..=max_y {
                for gx in min_x..=max_x {
                    wall_buckets.entry((gx, gy)).or_default().push(wall_idx);
                }
            }
        }

        for i in 0..desired_pickups {
            let base_pos = spawn_anchors[i % spawn_anchors.len()];
            let jitter = if i < strategic_anchor_count {
                50.0
            } else {
                110.0
            };
            let mut placed = false;
            for _attempt in 0..24 {
                let x_offset = rng.gen_range(-jitter..jitter);
                let y_offset = rng.gen_range(-jitter..jitter);
                let x = (base_pos.x + x_offset).clamp(WORLD_MIN_X + 50.0, WORLD_MAX_X - 50.0);
                let y = (base_pos.y + y_offset).clamp(WORLD_MIN_Y + 50.0, WORLD_MAX_Y - 50.0);

                let cell_x = ((x - WORLD_MIN_X) / PICKUP_WALL_GRID_CELL_SIZE).floor() as i32;
                let cell_y = ((y - WORLD_MIN_Y) / PICKUP_WALL_GRID_CELL_SIZE).floor() as i32;
                let mut obstructed = false;
                'wall_scan: for gy in (cell_y - 1)..=(cell_y + 1) {
                    for gx in (cell_x - 1)..=(cell_x + 1) {
                        let Some(bucket_wall_indices) = wall_buckets.get(&(gx, gy)) else {
                            continue;
                        };
                        for wall_idx in bucket_wall_indices {
                            let wall = &map_walls[*wall_idx];
                            if x + PICKUP_COLLECTION_RADIUS > wall.x
                                && x - PICKUP_COLLECTION_RADIUS < wall.x + wall.width
                                && y + PICKUP_COLLECTION_RADIUS > wall.y
                                && y - PICKUP_COLLECTION_RADIUS < wall.y + wall.height
                            {
                                obstructed = true;
                                break 'wall_scan;
                            }
                        }
                    }
                }
                if !obstructed {
                    let too_close_to_existing = pickups.iter().any(|existing| {
                        let dx = existing.x - x;
                        let dy = existing.y - y;
                        (dx * dx + dy * dy) < PICKUP_SPACING_MIN_SQ
                    });
                    if too_close_to_existing {
                        continue;
                    }

                    let pickup_type = pickup_types[i % pickup_types.len()].clone();
                    pickups.push(Pickup::new(generate_entity_id(), x, y, pickup_type));
                    placed = true;
                    break;
                }
            }
            if !placed {
                warn!(
                    "Could not place pickup {} near {:?} after 24 attempts.",
                    i, base_pos
                );
            }
        }
        pickups
    }

    pub fn spawn_initial_bots(&self, count: usize) {
        info!("Spawning {} initial bots...", count);
        // No longer reducing count here - use what's passed in
        let team_spawn_areas = MapGenerator::get_team_spawn_areas();
        let seed = self.frame_counter.load(AtomicOrdering::Relaxed)
            ^ ((count as u64) << 32)
            ^ self.bot_name_counter.load(AtomicOrdering::Relaxed);
        let mut rng = DeterministicRng::new(seed);

        for i in 0..count {
            let bot_name_num = self.bot_name_counter.fetch_add(1, AtomicOrdering::SeqCst);
            let bot_names = [
                "Alpha", "Beta", "Gamma", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
                "Juliet", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo",
                "Sierra", "Tango", "Uniform", "Victor", "Whiskey", "Xray", "Yankee", "Zulu",
            ];
            let bot_name = format!(
                "Bot {}",
                bot_names
                    .get(bot_name_num as usize % bot_names.len())
                    .unwrap_or(&"X")
            );
            let bot_player_id_str = format!("bot_{}", Uuid::new_v4());

            let team_id = (i % 2) + 1;

            let potential_spawns_for_team: Vec<Vec2> = team_spawn_areas
                .iter()
                .filter(|(_, sp_team_id)| *sp_team_id == team_id as u8)
                .map(|(pos, _)| *pos)
                .collect();

            let spawn_pos = if !potential_spawns_for_team.is_empty() {
                // Use team spawn point with some random offset
                let pick_idx =
                    rng.gen_range_i32(0, potential_spawns_for_team.len() as i32) as usize;
                let base_spawn = potential_spawns_for_team[pick_idx];
                let offset_radius = 50.0; // Small offset to prevent stacking
                let angle = rng.gen_range_f32(0.0, 2.0 * std::f32::consts::PI);
                let offset_x = offset_radius * angle.cos();
                let offset_y = offset_radius * angle.sin();
                Vec2::new(
                    (base_spawn.x + offset_x)
                        .clamp(WORLD_MIN_X + PLAYER_RADIUS, WORLD_MAX_X - PLAYER_RADIUS),
                    (base_spawn.y + offset_y)
                        .clamp(WORLD_MIN_Y + PLAYER_RADIUS, WORLD_MAX_Y - PLAYER_RADIUS),
                )
            } else {
                // Fallback: use respawn manager
                self.respawn_manager.get_respawn_position(
                    self,
                    &Arc::from(bot_player_id_str.clone()),
                    Some(team_id as u8),
                    &[],
                )
            };

            if let Some(player_id_arc) = self.player_manager.add_player(
                bot_player_id_str.clone(),
                bot_name.clone(),
                spawn_pos.x,
                spawn_pos.y,
            ) {
                if let Some(mut p_state) = self.player_manager.get_player_state_mut(&player_id_arc)
                {
                    p_state.team_id = team_id as u8;
                    p_state.is_bot = true;
                    p_state.bot_behavior = BotBehaviorState::Idle.as_u8();
                    p_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_MISC);
                }

                let bot_controller = BotController {
                    player_id: player_id_arc.clone(),
                    target_position: None,
                    target_enemy_id: None,
                    last_decision_time: Instant::now(),
                    last_decision_tick: 0,
                    ai_update_accumulator_secs: 0.0,
                    behavior_state: BotBehaviorState::Idle,
                    current_path: VecDeque::new(),
                    path_recalculation_timer: Instant::now(),
                    last_weapon_switch_time: Instant::now(),
                    last_weapon_switch_tick: 0,
                    last_position: Vec2::new(spawn_pos.x, spawn_pos.y),
                    stuck_timer: 0.0,
                    stuck_check_position: Vec2::new(spawn_pos.x, spawn_pos.y),
                    personality: crate::systems::ai::optimized_bot_ai::BotPersonality::random(),
                    path_compute_tick: 0,
                    last_path_target: None,
                };
                self.bot_players.insert(player_id_arc, bot_controller);
                debug!(
                    "Spawned bot: {} (ID: {}) on team {} at ({:.1}, {:.1})",
                    bot_name, bot_player_id_str, team_id, spawn_pos.x, spawn_pos.y
                );
            } else {
                error!("Failed to add bot {} to player manager.", bot_name);
            }
        }
    }

    fn apply_input_to_player_state(
        &self,
        player_state: &mut PlayerState,
        input: &PlayerInputData,
        current_server_time: Instant,
    ) {
        if player_state.is_spectator {
            if input.sequence == 0 {
                return;
            }
            if input.sequence <= player_state.last_processed_input_sequence {
                return;
            }
            player_state.last_processed_input_sequence = input.sequence;

            if (input.rotation - player_state.rotation).abs() > 0.001 {
                player_state.rotation = input.rotation;
            }

            let mut forward_intent = 0.0_f32;
            let mut strafe_intent = 0.0_f32;
            if input.move_forward {
                forward_intent += 1.0;
            }
            if input.move_backward {
                forward_intent -= 1.0;
            }
            if input.move_left {
                strafe_intent -= 1.0;
            }
            if input.move_right {
                strafe_intent += 1.0;
            }

            if forward_intent != 0.0 || strafe_intent != 0.0 {
                let move_magnitude =
                    (forward_intent * forward_intent + strafe_intent * strafe_intent).sqrt();
                forward_intent /= move_magnitude;
                strafe_intent /= move_magnitude;

                let cos_rot = player_state.rotation.cos();
                let sin_rot = player_state.rotation.sin();
                let forward_x = cos_rot * forward_intent;
                let forward_y = sin_rot * forward_intent;
                let strafe_x = -sin_rot * strafe_intent;
                let strafe_y = cos_rot * strafe_intent;
                let spectator_speed = PLAYER_BASE_SPEED * 1.35;
                player_state.velocity_x = (forward_x + strafe_x) * spectator_speed;
                player_state.velocity_y = (forward_y + strafe_y) * spectator_speed;
            } else {
                player_state.velocity_x = 0.0;
                player_state.velocity_y = 0.0;
            }
            player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            return;
        }

        if !player_state.alive {
            player_state.velocity_x = 0.0;
            player_state.velocity_y = 0.0;
            return;
        }

        if input.sequence == 0 {
            return;
        }

        if input.sequence <= player_state.last_processed_input_sequence {
            // warn!("[{}]: Received out-of-order or duplicate input (seq: {}, last_processed: {}). Ignoring.", player_state.id, input.sequence, player_state.last_processed_input_sequence);
            return;
        }
        player_state.last_processed_input_sequence = input.sequence;
        let player_id_for_anti_cheat = player_state.id.clone();
        self.apply_aim_anomaly_detection(
            &player_id_for_anti_cheat,
            input,
            player_state,
            current_server_time,
        );
        player_state.mark_field_changed(FIELD_POSITION_ROTATION);

        if (input.rotation - player_state.rotation).abs() > 0.001 {
            player_state.rotation = input.rotation;
            player_state.mark_field_changed(FIELD_POSITION_ROTATION);
        }

        if player_state.is_wall_slam_stunned() {
            player_state.velocity_x = 0.0;
            player_state.velocity_y = 0.0;
            player_state.mark_field_changed(FIELD_POSITION_ROTATION | FIELD_POWERUPS);
            return;
        }

        if player_state.set_killstreak_reward_preference_from_input_slot(input.use_ability_slot) {
            player_state.mark_field_changed(FIELD_SCORE_STATS);
        } else if input.use_ability_slot != 0 {
            match input.use_ability_slot {
                1 if player_state.ability_1_cooldown_remaining <= 0.0 => {
                    player_state.ability_1_cooldown_remaining = ABILITY_DASH_COOLDOWN_SECS;
                    player_state.dash_remaining = ABILITY_DASH_DURATION_SECS;
                    player_state.activate_dash_melee_chain_window();
                    player_state.mark_field_changed(FIELD_POWERUPS | FIELD_POSITION_ROTATION);
                }
                2 if player_state.ability_2_cooldown_remaining <= 0.0 => {
                    player_state.ability_2_cooldown_remaining = ABILITY_DODGE_COOLDOWN_SECS;
                    player_state.dodge_roll_remaining = ABILITY_DODGE_DURATION_SECS;
                    player_state.invulnerable_remaining = ABILITY_DODGE_DURATION_SECS;
                    player_state.activate_dodge_shot_chain_window();
                    player_state.mark_field_changed(FIELD_POWERUPS | FIELD_POSITION_ROTATION);
                }
                _ => {}
            }
        }

        // Calculate movement relative to the latest input rotation.
        let mut forward_intent = 0.0_f32;
        let mut strafe_intent = 0.0_f32;

        if input.move_forward {
            forward_intent += 1.0;
        }
        if input.move_backward {
            forward_intent -= 1.0;
        }
        if input.move_left {
            strafe_intent -= 1.0;
        }
        if input.move_right {
            strafe_intent += 1.0;
        }

        let speed_boost_active = player_state.speed_boost_remaining > 0.0
            || player_state.streak_speed_boost_remaining > 0.0;
        let effective_speed = if speed_boost_active {
            PLAYER_BASE_SPEED * SPEED_BOOST_MULTIPLIER
        } else {
            PLAYER_BASE_SPEED
        };
        let mut effective_speed = effective_speed;
        if player_state.dash_remaining > 0.0 {
            effective_speed *= ABILITY_DASH_SPEED_MULTIPLIER;
        }
        if player_state.dodge_roll_remaining > 0.0 {
            effective_speed *= ABILITY_DODGE_SPEED_MULTIPLIER;
        }

        if forward_intent == 0.0
            && strafe_intent == 0.0
            && (player_state.dash_remaining > 0.0 || player_state.dodge_roll_remaining > 0.0)
        {
            forward_intent = 1.0;
        }
        if forward_intent != 0.0 || strafe_intent != 0.0 {
            // Normalize movement vector
            let move_magnitude =
                (forward_intent * forward_intent + strafe_intent * strafe_intent).sqrt();
            forward_intent /= move_magnitude;
            strafe_intent /= move_magnitude;

            // Apply rotation to movement direction
            let cos_rot = player_state.rotation.cos();
            let sin_rot = player_state.rotation.sin();

            // Forward movement in the direction of rotation
            let forward_x = cos_rot * forward_intent;
            let forward_y = sin_rot * forward_intent;

            // Strafe movement perpendicular to rotation (90 degrees)
            let strafe_x = -sin_rot * strafe_intent;
            let strafe_y = cos_rot * strafe_intent;

            // Combine forward and strafe movement
            player_state.velocity_x = (forward_x + strafe_x) * effective_speed;
            player_state.velocity_y = (forward_y + strafe_y) * effective_speed;

            // Debug logging for bot movement
            if player_state.username.starts_with("Bot") {
                trace!("Bot {} velocity set to ({:.1}, {:.1}) from input forward={:.1} strafe={:.1} rot={:.2}",
                    player_state.username, player_state.velocity_x, player_state.velocity_y, forward_intent, strafe_intent, player_state.rotation);
            }
        } else {
            player_state.velocity_x = 0.0;
            player_state.velocity_y = 0.0;
        }
        player_state.mark_field_changed(FIELD_POSITION_ROTATION);

        if (input.ping_x != 0.0 || input.ping_y != 0.0)
            && player_state.team_id != 0
            && player_state.ping_cooldown_remaining <= 0.0
        {
            let ping_x = input.ping_x.clamp(WORLD_MIN_X, WORLD_MAX_X);
            let ping_y = input.ping_y.clamp(WORLD_MIN_Y, WORLD_MAX_Y);
            let now_ms = self.get_server_timestamp_ms();
            self.refresh_commander_runtime_state(now_ms);
            player_state.ping_cooldown_remaining = TEAM_PING_COOLDOWN_SECS;
            player_state.mark_field_changed(FIELD_POWERUPS);
            self.global_game_events.push(
                GameEvent::TeamPing {
                    player_id: player_state.id.clone(),
                    team_id: player_state.team_id,
                    position: Vec2::new(ping_x, ping_y),
                },
                EventPriority::High,
            );
            if self.is_player_team_commander(&player_state.id, player_state.team_id) {
                self.register_commander_waypoint(
                    &player_state.id,
                    player_state.team_id,
                    Vec2::new(ping_x, ping_y),
                    now_ms,
                );
            }
        }

        // Shooting logic for firearms
        if input.shooting
            && player_state.weapon != ServerWeaponType::Melee
            && player_state.can_shoot(current_server_time)
        {
            if let Some(last_shot_time) = player_state.last_shot_time {
                let elapsed = current_server_time
                    .saturating_duration_since(last_shot_time)
                    .as_secs_f32();
                if elapsed.is_finite() && elapsed > 0.0 {
                    player_state.weapon_spread_bloom = (player_state.weapon_spread_bloom
                        - (elapsed * WEAPON_SPREAD_BLOOM_DECAY_PER_SEC))
                        .max(0.0);
                }
            } else {
                player_state.weapon_spread_bloom = 0.0;
            }

            let bloom_for_shot = player_state.weapon_spread_bloom.max(0.0);
            let next_bloom = (bloom_for_shot
                + weapon_spread_bloom_per_shot_rad(player_state.weapon))
            .min(weapon_spread_bloom_cap_rad(player_state.weapon));
            player_state.weapon_spread_bloom = if next_bloom.is_finite() {
                next_bloom.max(0.0)
            } else {
                0.0
            };

            player_state.last_shot_time = Some(current_server_time);
            player_state.ammo -= 1;
            player_state.sync_active_weapon_to_loadout_slot();
            player_state.mark_field_changed(FIELD_WEAPON_AMMO);

            let spawn_offset = PLAYER_RADIUS + 5.0;
            let proj_spawn_x = player_state.x + player_state.rotation.cos() * spawn_offset;
            let proj_spawn_y = player_state.y + player_state.rotation.sin() * spawn_offset;

            let damage_multiplier = player_state.effective_damage_multiplier();

            self.global_game_events.push(
                GameEvent::WeaponFired {
                    player_id: player_state.id.clone(),
                    weapon: player_state.weapon,
                    position: Vec2 {
                        x: proj_spawn_x,
                        y: proj_spawn_y,
                    },
                },
                EventPriority::Normal,
            );

            match player_state.weapon {
                ServerWeaponType::Shotgun => {
                    let spread_seed = self.frame_counter.load(AtomicOrdering::Relaxed)
                        ^ (input.timestamp << 1)
                        ^ ((input.sequence as u64) << 33)
                        ^ ((player_state.x.to_bits() as u64) << 17)
                        ^ (player_state.y.to_bits() as u64)
                        ^ ((player_state.rotation.to_bits() as u64) << 7);
                    let mut spread_rng = DeterministicRng::new(spread_seed);
                    let spread_budget =
                        SHOTGUN_SPREAD_ANGLE_RAD + (bloom_for_shot * SHOTGUN_BLOOM_SPREAD_SCALE);
                    for _ in 0..SHOTGUN_PELLET_COUNT {
                        let angle_offset = spread_budget * spread_rng.gen_range_f32(-1.0, 1.0);
                        let dir_x = player_state.rotation.cos() * angle_offset.cos()
                            - player_state.rotation.sin() * angle_offset.sin();
                        let dir_y = player_state.rotation.sin() * angle_offset.cos()
                            + player_state.rotation.cos() * angle_offset.sin();
                        self.projectiles_to_add.push(Projectile::new(
                            player_state.id.clone(),
                            player_state.weapon,
                            proj_spawn_x,
                            proj_spawn_y,
                            dir_x,
                            dir_y,
                            damage_multiplier,
                        ));
                    }
                }
                // ServerWeaponType::Melee is handled by the separate melee_attack check below
                _ => {
                    // Pistol, Rifle, Sniper
                    let spread_seed = self.frame_counter.load(AtomicOrdering::Relaxed)
                        ^ (input.timestamp << 1)
                        ^ ((input.sequence as u64) << 35)
                        ^ ((player_state.x.to_bits() as u64) << 11)
                        ^ ((player_state.y.to_bits() as u64) << 3)
                        ^ ((player_state.rotation.to_bits() as u64) << 13)
                        ^ 0xA11CE5_u64;
                    let mut spread_rng = DeterministicRng::new(spread_seed);
                    let spread_budget =
                        weapon_base_spread_angle_rad(player_state.weapon) + bloom_for_shot;
                    let spread_angle = spread_budget * spread_rng.gen_range_f32(-1.0, 1.0);
                    let shot_rotation = player_state.rotation + spread_angle;

                    self.projectiles_to_add.push(Projectile::new(
                        player_state.id.clone(),
                        player_state.weapon,
                        proj_spawn_x,
                        proj_spawn_y,
                        shot_rotation.cos(),
                        shot_rotation.sin(),
                        damage_multiplier,
                    ));
                }
            }
        }

        // Melee Attack Logic (V key)
        if input.melee_attack && player_state.can_shoot(current_server_time) {
            // Using can_shoot for cooldown & alive check
            player_state.last_shot_time = Some(current_server_time); // Apply melee cooldown
            player_state.start_melee_windup(player_state.rotation);
            player_state.mark_field_changed(FIELD_POWERUPS | FIELD_POSITION_ROTATION);

            let telegraph_pos = Vec2 {
                x: player_state.x + player_state.rotation.cos() * (PLAYER_RADIUS + 1.0),
                y: player_state.y + player_state.rotation.sin() * (PLAYER_RADIUS + 1.0),
            };
            self.global_game_events.push(
                GameEvent::WeaponFired {
                    player_id: player_state.id.clone(),
                    weapon: ServerWeaponType::Melee,
                    position: telegraph_pos,
                },
                EventPriority::Normal,
            );
            debug!("[{}] initiated melee windup.", player_state.id);
        }

        if input.reload {
            player_state.start_reload(current_server_time);
        }

        if input.change_weapon_slot != 0 {
            if input.change_weapon_slot == 1 || input.change_weapon_slot == 2 {
                let _ = player_state.start_weapon_swap_to_slot(input.change_weapon_slot);
            } else {
                let new_weapon = match input.change_weapon_slot {
                    3 => Some(ServerWeaponType::Rifle),
                    4 => Some(ServerWeaponType::Sniper),
                    5 => Some(ServerWeaponType::Melee),
                    _ => None,
                };
                if let Some(weapon) = new_weapon {
                    player_state.replace_active_slot_weapon(weapon);
                }
            }
        }
    }

    pub(super) fn execute_pending_melee_attack(&self, player_state: &mut PlayerState) {
        if !player_state.melee_pending_attack || player_state.melee_windup_remaining > 0.0 {
            return;
        }

        player_state.melee_pending_attack = false;
        let attack_rotation = if player_state.melee_windup_rotation.is_finite() {
            player_state.melee_windup_rotation
        } else if player_state.rotation.is_finite() {
            player_state.rotation
        } else {
            0.0
        };
        player_state.melee_windup_rotation = 0.0;

        if !player_state.alive || player_state.is_spectator {
            player_state.mark_field_changed(FIELD_POWERUPS);
            return;
        }

        // Apply a short forward lunge after windup for clearer melee intent and reach.
        let lunge_distance = crate::core::constants::MELEE_LUNGE_DISTANCE.max(0.0);
        if lunge_distance > 0.0 {
            let start_x = player_state.x;
            let start_y = player_state.y;
            let target_x = (start_x + attack_rotation.cos() * lunge_distance)
                .clamp(WORLD_MIN_X + PLAYER_RADIUS, WORLD_MAX_X - PLAYER_RADIUS);
            let target_y = (start_y + attack_rotation.sin() * lunge_distance)
                .clamp(WORLD_MIN_Y + PLAYER_RADIUS, WORLD_MAX_Y - PLAYER_RADIUS);
            if self.has_clear_line_of_sight(start_x, start_y, target_x, target_y)
                && !self.position_overlaps_any_wall(target_x, target_y)
            {
                player_state.x = target_x;
                player_state.y = target_y;
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            }
        }

        let melee_event = GameEvent::MeleeHit {
            attacker_id: player_state.id.clone(),
            target_id: None,
            position: Vec2 {
                x: player_state.x + attack_rotation.cos() * (PLAYER_RADIUS + 1.0),
                y: player_state.y + attack_rotation.sin() * (PLAYER_RADIUS + 1.0),
            },
        };
        self.melee_hit_events.push(melee_event);
        player_state.mark_field_changed(FIELD_POWERUPS | FIELD_POSITION_ROTATION);
        debug!("[{}] resolved melee attack after windup.", player_state.id);
    }

    pub async fn process_network_input(&self) {
        let network_start = Instant::now();
        let current_server_time = Instant::now();
        let kick_threshold = anti_cheat_kick_threshold();

        // First, collect all player inputs with their IDs
        let mut all_inputs = Vec::new();
        let mut anti_cheat_kicks: HashSet<PlayerID> = HashSet::new();
        self.player_manager
            .for_each_player_mut(|player_id, player_state| {
                player_state.clear_changed_fields();
                let input_count = player_state
                    .input_queue
                    .len()
                    .min(MAX_INPUTS_PROCESSED_PER_TICK_PER_PLAYER);
                let mut inputs = Vec::with_capacity(input_count);
                for _ in 0..input_count {
                    if let Some(input) = player_state.input_queue.pop_front() {
                        inputs.push(input);
                    }
                }
                if !inputs.is_empty() {
                    all_inputs.push((player_id.clone(), inputs));
                }
            });

        // Then process each player's inputs
        for (player_id, inputs) in all_inputs {
            if let Some(mut player_state_entry) =
                self.player_manager.get_player_state_mut(&player_id)
            {
                for input in inputs {
                    self.apply_input_to_player_state(
                        &mut player_state_entry,
                        &input,
                        current_server_time,
                    );
                }

                if let Some(threshold) = kick_threshold {
                    if !self.bot_players.contains_key(&player_id)
                        && !player_state_entry.is_spectator
                        && player_state_entry.violation_count >= threshold
                    {
                        anti_cheat_kicks.insert(player_id.clone());
                    }
                }
            }
        }

        for peer_id in anti_cheat_kicks {
            if let Some(player_state) = self.player_manager.get_player_state(&peer_id) {
                warn!(
                    "[{}]: Auto-kicking player due to anti-cheat violations (count={}, threshold={}).",
                    peer_id.as_ref(),
                    player_state.violation_count,
                    kick_threshold.unwrap_or(DEFAULT_ANTI_CHEAT_KICK_THRESHOLD)
                );
            }
            self.remove_quic_player(peer_id.as_ref());
            let _ = crate::network::connection_manager::shared_connection_manager()
                .remove(peer_id.as_ref());
        }
        metrics::record_subsystem_time("network", network_start.elapsed().as_secs_f64());
    }

    pub async fn run_ai_update(&self) {
        let delta_time = crate::core::constants::TICK_DURATION_SECS_F32;
        if self.commander_mode_enabled {
            self.refresh_commander_runtime_state(self.get_server_timestamp_ms());
        }
        // Use the optimized bot AI that processes bots in batches
        OptimizedBotAI::update_bots_batch(self, delta_time);
    }
}
