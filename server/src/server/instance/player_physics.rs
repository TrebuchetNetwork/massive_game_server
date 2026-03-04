use super::*;
use rayon::prelude::*;

impl MassiveGameServer {
    pub(super) async fn process_player_physics_parallel(
        &self,
        delta_time: f32,
    ) -> PlayerPhysicsResults {
        let sample_timestamp_ms = self.get_server_timestamp_ms();

        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        if frame.is_multiple_of(120) {
            self.prune_runtime_tracking_state();
        }

        // Snapshot player IDs first, then process each shard item in parallel on the
        // physics thread pool. This keeps mutations per-player while avoiding a
        // single-threaded walk across all shards every tick.
        let player_ids = self.player_manager.player_ids_snapshot();
        let (all_to_respawn, total_alive) = self.thread_pools.physics_pool.install(|| {
            player_ids
                .par_iter()
                .fold(
                    || (Vec::<(PlayerID, u8)>::new(), 0usize),
                    |mut acc, player_id| {
                        if let Some(mut player_state) =
                            self.player_manager.get_player_state_mut(player_id)
                        {
                            player_state.update_timers(delta_time);

                            if player_state.is_spectator {
                                self.process_player_movement_optimized(
                                    &mut player_state,
                                    delta_time,
                                );
                                self.record_player_position_sample(
                                    player_id,
                                    sample_timestamp_ms,
                                    player_state.x,
                                    player_state.y,
                                );
                            } else if player_state.alive {
                                acc.1 = acc.1.saturating_add(1);
                                self.process_player_movement_optimized(
                                    &mut player_state,
                                    delta_time,
                                );
                                self.record_player_position_sample(
                                    player_id,
                                    sample_timestamp_ms,
                                    player_state.x,
                                    player_state.y,
                                );
                            } else if player_state.respawn_timer == Some(0.0) {
                                acc.0.push((player_id.clone(), player_state.team_id));
                            }
                        }
                        acc
                    },
                )
                .reduce(
                    || (Vec::<(PlayerID, u8)>::new(), 0usize),
                    |mut left, mut right| {
                        left.0.append(&mut right.0);
                        left.1 = left.1.saturating_add(right.1);
                        left
                    },
                )
        });

        self.apply_player_soft_push_separation(delta_time);

        PlayerPhysicsResults {
            players_to_respawn: all_to_respawn,
            alive_count: total_alive,
        }
    }

    pub(super) fn process_player_movement_optimized(
        &self,
        player_state: &mut PlayerState,
        delta_time: f32,
    ) {
        let old_x = player_state.x;
        let old_y = player_state.y;
        let mut movement_multiplier = 1.0f32;
        let mut boost_pad_direction = None;

        if player_state.is_spectator {
            player_state.x = (player_state.x + player_state.velocity_x * delta_time)
                .clamp(WORLD_MIN_X + PLAYER_RADIUS, WORLD_MAX_X - PLAYER_RADIUS);
            player_state.y = (player_state.y + player_state.velocity_y * delta_time)
                .clamp(WORLD_MIN_Y + PLAYER_RADIUS, WORLD_MAX_Y - PLAYER_RADIUS);
            if (old_x - player_state.x).abs() > 0.01 || (old_y - player_state.y).abs() > 0.01 {
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            }
            return;
        }

        for zone in self.zones.iter() {
            if !zone.contains(old_x, old_y) {
                continue;
            }
            match zone.zone_type {
                ZoneType::SlowZone => {
                    movement_multiplier = movement_multiplier.min(ZONE_SLOW_MULTIPLIER);
                }
                ZoneType::BoostPad if player_state.zone_boost_cooldown_remaining <= 0.0 => {
                    boost_pad_direction = Some(zone.direction);
                }
                _ => {}
            }
        }

        if let Some(direction) = boost_pad_direction {
            player_state.velocity_x =
                direction.cos() * PLAYER_BASE_SPEED * ZONE_BOOST_SPEED_MULTIPLIER;
            player_state.velocity_y =
                direction.sin() * PLAYER_BASE_SPEED * ZONE_BOOST_SPEED_MULTIPLIER;
            player_state.speed_boost_remaining = player_state
                .speed_boost_remaining
                .max(ZONE_BOOST_DURATION_SECS);
            player_state.zone_boost_cooldown_remaining = ZONE_BOOST_RETRIGGER_COOLDOWN_SECS;
            player_state.mark_field_changed(FIELD_POWERUPS | FIELD_POSITION_ROTATION);
        }

        // Debug logging for bot movement
        if player_state.username.starts_with("Bot")
            && (player_state.velocity_x != 0.0 || player_state.velocity_y != 0.0)
        {
            trace!(
                "Bot {} physics: pos({:.1},{:.1}) vel({:.1},{:.1}) dt={:.3}",
                player_state.username,
                old_x,
                old_y,
                player_state.velocity_x,
                player_state.velocity_y,
                delta_time
            );
        }

        // Apply velocity
        player_state.x += player_state.velocity_x * delta_time * movement_multiplier;
        player_state.y += player_state.velocity_y * delta_time * movement_multiplier;

        // Log position after velocity application
        if player_state.username.starts_with("Bot")
            && (old_x != player_state.x || old_y != player_state.y)
        {
            trace!(
                "Bot {} moved to ({:.1},{:.1})",
                player_state.username,
                player_state.x,
                player_state.y
            );
        }

        // Quick bounds check first
        let half_radius = PLAYER_RADIUS;
        if player_state.x < WORLD_MIN_X + half_radius
            || player_state.x > WORLD_MAX_X - half_radius
            || player_state.y < WORLD_MIN_Y + half_radius
            || player_state.y > WORLD_MAX_Y - half_radius
        {
            player_state.x = player_state
                .x
                .clamp(WORLD_MIN_X + half_radius, WORLD_MAX_X - half_radius);
            player_state.y = player_state
                .y
                .clamp(WORLD_MIN_Y + half_radius, WORLD_MAX_Y - half_radius);
            player_state.velocity_x = 0.0;
            player_state.velocity_y = 0.0;
            player_state.last_valid_position = (player_state.x, player_state.y);
            player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            return;
        }

        // Use spatial index to query nearby walls
        let check_radius = PLAYER_RADIUS + 10.0; // Reduced from 50.0 since spatial index is efficient
        let nearby_walls =
            self.wall_spatial_index
                .query_radius(player_state.x, player_state.y, check_radius);

        // Check collision with nearby walls only
        for wall in nearby_walls.iter() {
            let closest_x = player_state.x.clamp(wall.x, wall.x + wall.width);
            let closest_y = player_state.y.clamp(wall.y, wall.y + wall.height);

            let dist_sq =
                (player_state.x - closest_x).powi(2) + (player_state.y - closest_y).powi(2);
            if dist_sq < PLAYER_RADIUS.powi(2) {
                let impact_speed =
                    (player_state.velocity_x.powi(2) + player_state.velocity_y.powi(2)).sqrt();
                if impact_speed >= crate::core::constants::WALL_SLAM_STUN_SPEED_THRESHOLD
                    && !player_state.is_wall_slam_stunned()
                {
                    player_state.apply_wall_slam_stun();
                    player_state.mark_field_changed(FIELD_POWERUPS);

                    let impact_position = Vec2::new(player_state.x, player_state.y);
                    let impact_damage =
                        (impact_speed * 0.2).round().clamp(1.0, 250.0).round() as i32;
                    self.global_game_events.push(
                        GameEvent::WallImpact {
                            wall_id: wall.id,
                            position: impact_position,
                            damage: impact_damage,
                        },
                        EventPriority::High,
                    );

                    let player_id = player_state.id.clone();
                    let payload = serde_json::json!({
                        "player_id": player_id.as_ref(),
                        "x": impact_position.x,
                        "y": impact_position.y,
                        "stun_secs": crate::core::constants::WALL_SLAM_STUN_DURATION_SECS,
                        "impact_speed": impact_speed,
                    });
                    if let Some(packet) =
                        self.build_system_event_packet("wall_slam", Some(&payload))
                    {
                        self.enqueue_direct_packet_for_peer(player_id.as_ref(), packet);
                    }
                }

                // Collision detected - revert position
                player_state.x = old_x;
                player_state.y = old_y;
                player_state.velocity_x = 0.0;
                player_state.velocity_y = 0.0;
                player_state.last_valid_position = (old_x, old_y);
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
                return;
            }
        }

        // Anti-cheat validation – position-based
        let tolerance = crate::core::constants::speed_hack_tolerance();
        let max_speed_dist = PLAYER_BASE_SPEED * tolerance * delta_time;
        // Fixed slack per tick allowed excessive burst distance; scale with expected movement instead.
        let adaptive_slack = (max_speed_dist * 0.15).clamp(1.0, MAX_POSITION_DELTA_SLACK);
        let max_dist = max_speed_dist + adaptive_slack;
        let actual_dist = ((player_state.x - player_state.last_valid_position.0).powi(2)
            + (player_state.y - player_state.last_valid_position.1).powi(2))
        .sqrt();

        if actual_dist > max_dist {
            player_state.violation_count += 1;
            if player_state.violation_count > POSITION_VALIDATION_VIOLATION_THRESHOLD {
                player_state.x = player_state.last_valid_position.0;
                player_state.y = player_state.last_valid_position.1;
                player_state.velocity_x = 0.0;
                player_state.velocity_y = 0.0;
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            }
        } else {
            player_state.last_valid_position = (player_state.x, player_state.y);
            player_state.violation_count = player_state.violation_count.saturating_sub(1);
        }

        // Anti-cheat validation – acceleration-based
        // Detect impossible velocity changes between ticks that indicate speed hacking.
        let dvx = player_state.velocity_x - player_state.prev_velocity.0;
        let dvy = player_state.velocity_y - player_state.prev_velocity.1;
        let accel_magnitude = (dvx * dvx + dvy * dvy).sqrt();

        if accel_magnitude > MAX_ACCELERATION_PER_TICK {
            player_state.acceleration_violation_count += 1;
            if player_state.acceleration_violation_count > ACCELERATION_VIOLATION_THRESHOLD {
                warn!(
                    "[{}]: Acceleration anomaly (accel={:.1}, threshold={:.1}, count={}).",
                    player_state.id.as_ref(),
                    accel_magnitude,
                    MAX_ACCELERATION_PER_TICK,
                    player_state.acceleration_violation_count
                );
                player_state.violation_count = player_state.violation_count.saturating_add(1);
                // Clamp to the maximum allowed acceleration instead of snapping to zero-like motion.
                let clamp_scale = (MAX_ACCELERATION_PER_TICK / accel_magnitude).clamp(0.0, 1.0);
                player_state.velocity_x = player_state.prev_velocity.0 + dvx * clamp_scale;
                player_state.velocity_y = player_state.prev_velocity.1 + dvy * clamp_scale;
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            }
        } else {
            player_state.acceleration_violation_count =
                player_state.acceleration_violation_count.saturating_sub(1);
        }
        player_state.prev_velocity = (player_state.velocity_x, player_state.velocity_y);

        // Mark as changed if moved
        if (old_x - player_state.x).abs() > 0.01 || (old_y - player_state.y).abs() > 0.01 {
            player_state.mark_field_changed(FIELD_POSITION_ROTATION);
        }

        let mut zone_damage = 0i32;
        for zone in self.zones.iter() {
            if zone.zone_type == ZoneType::DamageZone
                && zone.contains(player_state.x, player_state.y)
            {
                zone_damage = (ZONE_DAMAGE_PER_SEC * delta_time).ceil().max(1.0) as i32;
                break;
            }
        }
        if zone_damage > 0 {
            let died = player_state.apply_damage(zone_damage);
            let pos = Vec2::new(player_state.x, player_state.y);
            self.global_game_events.push(
                GameEvent::PlayerDamaged {
                    target_id: player_state.id.clone(),
                    attacker_id: None,
                    damage: zone_damage,
                    weapon: ServerWeaponType::Melee,
                    position: pos,
                },
                EventPriority::Low,
            );

            if died {
                let victim_name = player_state.username.clone();
                let player_id = player_state.id.clone();
                self.global_game_events.push(
                    GameEvent::PlayerKilled {
                        victim_id: player_id.clone(),
                        killer_id: Arc::from("environment".to_string()),
                        weapon: ServerWeaponType::Melee,
                        position: pos,
                    },
                    EventPriority::Normal,
                );
                self.push_kill_feed_entry(
                    "Environment".to_string(),
                    victim_name,
                    ServerWeaponType::Melee,
                    false,
                );

                // Losing team respawn reduction for zone deaths
                {
                    let victim_team = player_state.team_id;
                    if victim_team != 0 {
                        let match_info_guard = self.match_info.read();
                        let victim_team_score = match_info_guard
                            .team_scores
                            .get(&victim_team)
                            .cloned()
                            .unwrap_or(0);
                        let max_enemy_score = match_info_guard
                            .team_scores
                            .iter()
                            .filter(|(&tid, _)| tid != victim_team)
                            .map(|(_, &s)| s)
                            .max()
                            .unwrap_or(0);
                        drop(match_info_guard);

                        let deficit = max_enemy_score - victim_team_score;
                        if deficit > 0 {
                            let reduction_ticks = (deficit / 5).max(0) as f32;
                            let reduction =
                                reduction_ticks * LOSING_TEAM_RESPAWN_REDUCTION_PER_5PTS;
                            if let Some(ref mut timer) = player_state.respawn_timer {
                                *timer = (*timer - reduction).max(0.5);
                            }
                        }
                    }
                }

                if player_state.is_carrying_flag_team_id != 0 {
                    let dropped_flag_team = player_state.is_carrying_flag_team_id;
                    player_state.is_carrying_flag_team_id = 0;
                    player_state.mark_field_changed(FIELD_FLAG);
                    let mut match_info_guard = self.match_info.write();
                    if let Some(flag_state) =
                        match_info_guard.flag_states.get_mut(&dropped_flag_team)
                    {
                        flag_state.status = fb::FlagStatus::Dropped;
                        flag_state.position = pos;
                        flag_state.carrier_id = None;
                        flag_state.respawn_timer = 30.0;
                    }
                    drop(match_info_guard);
                    self.global_game_events.push(
                        GameEvent::FlagDropped {
                            player_id,
                            flag_team_id: dropped_flag_team,
                            position: pos,
                        },
                        EventPriority::High,
                    );
                }
            }
        }
    }

    pub(super) fn apply_player_soft_push_separation(&self, delta_time: f32) {
        let mut alive_positions: Vec<(PlayerID, f32, f32)> = Vec::new();
        self.player_manager
            .for_each_player(|player_id, player_state| {
                if player_state.alive && !player_state.is_spectator {
                    alive_positions.push((player_id.clone(), player_state.x, player_state.y));
                }
            });

        if alive_positions.len() < 2 {
            return;
        }

        let min_distance = PLAYER_RADIUS * 2.0;
        let min_distance_sq = min_distance * min_distance;
        let max_push_per_player = (PLAYER_BASE_SPEED * delta_time * 0.5).clamp(0.5, 6.0);
        let mut id_to_index: HashMap<PlayerID, usize> =
            HashMap::with_capacity(alive_positions.len());
        for (idx, (id, _, _)) in alive_positions.iter().enumerate() {
            id_to_index.insert(id.clone(), idx);
        }
        let mut accumulated_push: Vec<(f32, f32)> = vec![(0.0, 0.0); alive_positions.len()];

        for (left_idx, (_, left_x, left_y)) in alive_positions.iter().enumerate() {
            // Use spatial index for fast nearby player lookup instead of O(N^2)
            let nearby =
                self.spatial_index
                    .query_nearby_players(*left_x, *left_y, min_distance * 2.0);

            for right_id in nearby {
                let Some(&right_idx) = id_to_index.get(&right_id) else {
                    continue;
                };
                if left_idx >= right_idx {
                    continue;
                }

                let (_, right_x, right_y) = &alive_positions[right_idx];
                let dx = *right_x - *left_x;
                let dy = *right_y - *left_y;
                let dist_sq = dx * dx + dy * dy;

                if dist_sq >= min_distance_sq {
                    continue;
                }

                let dist = dist_sq.sqrt().max(0.001);
                let overlap = (min_distance - dist).max(0.0);
                if overlap <= 0.0 {
                    continue;
                }

                let push = (overlap * 0.5).min(max_push_per_player);
                let normal_x = dx / dist;
                let normal_y = dy / dist;

                accumulated_push[left_idx].0 -= normal_x * push;
                accumulated_push[left_idx].1 -= normal_y * push;
                accumulated_push[right_idx].0 += normal_x * push;
                accumulated_push[right_idx].1 += normal_y * push;
            }
        }

        if accumulated_push
            .iter()
            .all(|(push_x, push_y)| push_x.abs() <= f32::EPSILON && push_y.abs() <= f32::EPSILON)
        {
            return;
        }

        for ((player_id, _, _), (push_x, push_y)) in alive_positions
            .into_iter()
            .zip(accumulated_push.into_iter())
        {
            if push_x.abs() <= f32::EPSILON && push_y.abs() <= f32::EPSILON {
                continue;
            }

            let Some(mut player_state) = self.player_manager.get_player_state_mut(&player_id)
            else {
                continue;
            };
            if !player_state.alive || player_state.is_spectator {
                continue;
            }

            let next_x = (player_state.x + push_x)
                .clamp(WORLD_MIN_X + PLAYER_RADIUS, WORLD_MAX_X - PLAYER_RADIUS);
            let next_y = (player_state.y + push_y)
                .clamp(WORLD_MIN_Y + PLAYER_RADIUS, WORLD_MAX_Y - PLAYER_RADIUS);

            if self.position_overlaps_any_wall(next_x, next_y) {
                continue;
            }

            if (next_x - player_state.x).abs() > 0.001 || (next_y - player_state.y).abs() > 0.001 {
                player_state.x = next_x;
                player_state.y = next_y;
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            }
        }
    }

    pub(super) async fn apply_player_updates(
        &self,
        updates: PlayerPhysicsResults,
        active_walls: &[Wall],
    ) {
        // Precompute enemy snapshots once for this respawn batch.
        let enemies_for_team_1 = self.get_enemy_positions_for_team(1);
        let enemies_for_team_2 = self.get_enemy_positions_for_team(2);
        let no_enemies: Vec<(Vec2, PlayerID)> = Vec::new();

        // Batch respawns
        for (player_id, team_id) in updates.players_to_respawn {
            let assigned_team = if team_id == 1 || team_id == 2 {
                Some(team_id)
            } else {
                None
            };
            let enemies = match assigned_team {
                Some(1) => enemies_for_team_1.as_slice(),
                Some(2) => enemies_for_team_2.as_slice(),
                _ => no_enemies.as_slice(),
            };
            let spawn_pos = self.respawn_manager.get_respawn_position_with_walls(
                &player_id,
                assigned_team,
                enemies,
                active_walls,
            );

            if let Some(mut p_state) = self.player_manager.get_player_state_mut(&player_id) {
                p_state.respawn(spawn_pos.x, spawn_pos.y);
                self.record_player_position_sample(
                    &player_id,
                    self.get_server_timestamp_ms(),
                    spawn_pos.x,
                    spawn_pos.y,
                );
            }
        }
    }

    fn get_enemy_positions_for_team(&self, team_id: u8) -> Vec<(Vec2, PlayerID)> {
        let mut enemies = Vec::with_capacity(50);
        self.player_manager.for_each_player(|id, state| {
            if state.alive && !state.is_spectator && state.team_id != team_id && state.team_id != 0
            {
                enemies.push((Vec2::new(state.x, state.y), id.clone()));
            }
        });
        enemies
    }
}
