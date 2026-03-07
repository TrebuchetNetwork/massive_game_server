use super::*;

impl MassiveGameServer {
    fn pickup_announcement_meta(
        pickup_type: &CorePickupType,
    ) -> Option<(&'static str, &'static str)> {
        match pickup_type {
            CorePickupType::SpeedBoost => Some(("Speed Boost", "defend")),
            CorePickupType::DamageBoost => Some(("Damage Boost", "defend")),
            CorePickupType::Shield => Some(("Shield", "defend")),
            CorePickupType::WeaponCrate(_) => Some(("Weapon Crate", "defend")),
            CorePickupType::Health | CorePickupType::Ammo => None,
        }
    }

    #[inline]
    fn should_emit_pickup_countdown(previous_timer: f32, next_timer: f32) -> bool {
        previous_timer > PICKUP_RESPAWN_ANNOUNCE_SECS
            && next_timer <= PICKUP_RESPAWN_ANNOUNCE_SECS
            && next_timer > 0.0
    }

    fn broadcast_pickup_spawn_notice_event(
        &self,
        pickup: &Pickup,
        phase: &str,
        seconds_remaining: f32,
        pickup_label: &str,
        ping_kind: &str,
    ) {
        let payload = serde_json::json!({
            "phase": phase,
            "pickup_id": pickup.id,
            "pickup_label": pickup_label,
            "ping_kind": ping_kind,
            "x": pickup.x,
            "y": pickup.y,
            "seconds_remaining": seconds_remaining.max(0.0),
        });
        if let Some(packet) = self.build_system_event_packet("pickup_spawn_notice", Some(&payload))
        {
            self.enqueue_direct_packet_for_all_players(packet);
        }
    }

    pub(super) fn drain_queued_projectiles_to_authoritative_state(&self) {
        let mut queued_projectiles = Vec::new();
        while let Some(proj) = self.projectiles_to_add.pop() {
            queued_projectiles.push(proj);
        }
        if queued_projectiles.is_empty() {
            return;
        }

        let spatial_updates: Vec<(EntityId, f32, f32)> = queued_projectiles
            .iter()
            .map(|proj| (proj.id, proj.x, proj.y))
            .collect();
        if !spatial_updates.is_empty() {
            self.spatial_index
                .batch_update_projectiles(&spatial_updates);
        }

        self.projectiles.extend(queued_projectiles);
    }

    fn take_authoritative_projectiles_for_processing(&self) -> Vec<Projectile> {
        self.projectiles.take_all()
    }

    fn commit_authoritative_projectile_state(
        &self,
        kept_projectiles: Vec<Projectile>,
        removed_ids: &[EntityId],
    ) {
        for proj_id in removed_ids {
            self.spatial_index.remove_projectile(proj_id);
        }

        self.projectiles.replace_all(kept_projectiles);
    }

    #[inline]
    fn is_projectile_headshot(
        target_state: &crate::core::types::PlayerState,
        hit_x: f32,
        hit_y: f32,
    ) -> bool {
        if !target_state.alive {
            return false;
        }

        let rel_x = hit_x - target_state.x;
        let rel_y = hit_y - target_state.y;
        if !rel_x.is_finite() || !rel_y.is_finite() {
            return false;
        }

        let radius_limit =
            crate::core::constants::PLAYER_RADIUS * crate::core::constants::HEADSHOT_RADIUS_FRAC;
        let dist_sq = rel_x * rel_x + rel_y * rel_y;
        if dist_sq > radius_limit * radius_limit {
            return false;
        }

        let facing_x = target_state.rotation.cos();
        let facing_y = target_state.rotation.sin();
        if !facing_x.is_finite() || !facing_y.is_finite() {
            return false;
        }

        let right_x = -facing_y;
        let right_y = facing_x;
        let forward_offset = rel_x * facing_x + rel_y * facing_y;
        let lateral_offset = (rel_x * right_x + rel_y * right_y).abs();

        forward_offset
            >= crate::core::constants::PLAYER_RADIUS
                * crate::core::constants::HEADSHOT_FORWARD_MIN_FRAC
            && lateral_offset
                <= crate::core::constants::PLAYER_RADIUS
                    * crate::core::constants::HEADSHOT_LATERAL_MAX_FRAC
    }

    pub(super) async fn process_projectiles_optimized(
        &self,
        _walls: &[Wall],
        delta_time: f32,
    ) -> ProjectileResults {
        use rayon::prelude::*;

        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        trace!("[Frame {}] Starting optimized projectile processing", frame);

        // Take authoritative projectile state for parallel processing.
        let mut all_projectiles = self.take_authoritative_projectiles_for_processing();

        let total_projectiles = all_projectiles.len();
        trace!(
            "[Frame {}] Processing {} projectiles",
            frame,
            total_projectiles
        );

        if total_projectiles == 0 {
            return ProjectileResults {
                total_processed: 0,
                hits: Vec::new(),
                wall_hits: Vec::new(),
                removed_projectile_ids: Vec::new(),
                kept_projectiles: Vec::new(),
                spatial_updates: Vec::new(),
                wall_impacts: Vec::new(),
            };
        }

        let clamped_lag_ms = self
            .lag_compensation_ms
            .min(crate::core::constants::MAX_LAG_COMPENSATION_MS);
        let lag_compensation_target_ms = self
            .get_server_timestamp_ms()
            .saturating_sub(clamped_lag_ms);

        // Process projectiles in parallel chunks
        // Keep chunks large enough to amortize overhead while still spreading
        // medium loads across worker threads.
        let worker_count = rayon::current_num_threads().max(1);
        let target_chunk = total_projectiles.div_ceil(worker_count);
        let chunk_size = target_chunk.clamp(8, 256);

        let chunk_results: Vec<ProjectileChunkResults> = all_projectiles
            .par_chunks_mut(chunk_size)
            .enumerate()
            .map(|(chunk_idx, chunk)| {
                let mut local_results = ProjectileChunkResults::default();
                let chunk_start_idx = chunk_idx * chunk_size;
                let mut target_ids: Vec<PlayerID> = Vec::with_capacity(32);
                let mut target_xs: Vec<f32> = Vec::with_capacity(32);
                let mut target_ys: Vec<f32> = Vec::with_capacity(32);

                for (local_idx, proj) in chunk.iter_mut().enumerate() {
                    let global_idx = chunk_start_idx + local_idx;

                    // Update position
                    let old_x = proj.x;
                    let old_y = proj.y;
                    proj.x += proj.velocity_x * delta_time;
                    proj.y += proj.velocity_y * delta_time;

                    local_results
                        .spatial_updates
                        .push((proj.id, proj.x, proj.y));

                    // Check bounds
                    if proj.x < WORLD_MIN_X
                        || proj.x > WORLD_MAX_X
                        || proj.y < WORLD_MIN_Y
                        || proj.y > WORLD_MAX_Y
                    {
                        local_results.to_remove.push(global_idx);
                        continue;
                    }

                    // Check lifetime
                    if proj.should_remove() {
                        local_results.to_remove.push(global_idx);
                        continue;
                    }

                    // Continuous wall collision detection against active walls from the
                    // authoritative wall spatial index. This avoids misses for walls that
                    // span multiple partitions but are stored by center partition only.
                    let mut earliest_wall_hit_t: Option<f32> = None;
                    let mut earliest_wall_id: EntityId = 0;
                    let mut earliest_wall_destructible = false;
                    let mut earliest_wall_surface_type = SurfaceType::Concrete.as_u8();
                    let candidate_walls = self
                        .wall_spatial_index
                        .query_line_segment(old_x, old_y, proj.x, proj.y);
                    for wall in candidate_walls {
                        let Some(hit_t) = segment_first_hit_fraction_with_aabb(
                            old_x,
                            old_y,
                            proj.x,
                            proj.y,
                            wall.x,
                            wall.x + wall.width,
                            wall.y,
                            wall.y + wall.height,
                        ) else {
                            continue;
                        };

                        let is_earlier_hit = match earliest_wall_hit_t {
                            Some(existing_t) => hit_t < existing_t,
                            None => true,
                        };
                        if is_earlier_hit {
                            earliest_wall_hit_t = Some(hit_t);
                            earliest_wall_id = wall.id;
                            earliest_wall_destructible = wall.is_destructible;
                            earliest_wall_surface_type = wall.inferred_surface_type().as_u8();
                        }
                    }

                    if let Some(hit_t) = earliest_wall_hit_t {
                        let hit_x = old_x + (proj.x - old_x) * hit_t;
                        let hit_y = old_y + (proj.y - old_y) * hit_t;
                        proj.x = hit_x;
                        proj.y = hit_y;

                        local_results.wall_impacts.push(GameEvent::WallImpact {
                            position: Vec2::new(hit_x, hit_y),
                            wall_id: earliest_wall_id,
                            damage: if earliest_wall_destructible {
                                proj.damage
                            } else {
                                0
                            },
                            surface_type: earliest_wall_surface_type,
                        });

                        if earliest_wall_destructible {
                            local_results
                                .wall_hits
                                .push((earliest_wall_id, proj.damage));
                        }

                        local_results.to_remove.push(global_idx);
                        continue;
                    }

                    // Check player collisions using spatial index.
                    let nearby_players = self.spatial_index.query_nearby_players_with_positions(
                        proj.x,
                        proj.y,
                        PLAYER_RADIUS + 20.0, // Small buffer for fast projectiles
                    );
                    target_ids.clear();
                    target_xs.clear();
                    target_ys.clear();

                    for (target_id, target_x, target_y) in nearby_players {
                        if target_id == proj.owner_id {
                            continue;
                        }
                        let Some(target_state) = self.player_manager.get_player_state(&target_id)
                        else {
                            continue;
                        };
                        if !target_state.alive || target_state.is_spectator {
                            continue;
                        }
                        drop(target_state);
                        let (validated_target_x, validated_target_y) = self
                            .get_rewound_player_position(&target_id, lag_compensation_target_ms)
                            .unwrap_or((target_x, target_y));
                        target_ids.push(target_id);
                        target_xs.push(validated_target_x);
                        target_ys.push(validated_target_y);
                    }

                    if !target_ids.is_empty() {
                        let radius_sq = PLAYER_RADIUS * PLAYER_RADIUS;
                        if let Some(target_idx) = simd::first_index_within_segment_radius(
                            &target_xs, &target_ys, old_x, old_y, proj.x, proj.y, radius_sq,
                        ) {
                            if let Some(target_id) = target_ids.get(target_idx) {
                                let seg_dx = proj.x - old_x;
                                let seg_dy = proj.y - old_y;
                                let seg_len_sq = seg_dx * seg_dx + seg_dy * seg_dy;
                                let target_x = target_xs[target_idx];
                                let target_y = target_ys[target_idx];
                                let t = if seg_len_sq > f32::EPSILON {
                                    (((target_x - old_x) * seg_dx + (target_y - old_y) * seg_dy)
                                        / seg_len_sq)
                                        .clamp(0.0, 1.0)
                                } else {
                                    0.0
                                };
                                let hit_x = old_x + seg_dx * t;
                                let hit_y = old_y + seg_dy * t;

                                proj.x = hit_x;
                                proj.y = hit_y;
                                local_results.hits.push((
                                    proj.owner_id.clone(),
                                    target_id.clone(),
                                    proj.damage,
                                    proj.weapon_type,
                                    hit_x,
                                    hit_y,
                                ));
                                local_results.to_remove.push(global_idx);
                            }
                        }
                    }
                }

                local_results
            })
            .collect();

        let mut merged_results = ProjectileChunkResults::default();
        for mut chunk_result in chunk_results {
            merged_results.to_remove.append(&mut chunk_result.to_remove);
            merged_results.hits.append(&mut chunk_result.hits);
            merged_results.wall_hits.append(&mut chunk_result.wall_hits);
            merged_results
                .spatial_updates
                .append(&mut chunk_result.spatial_updates);
            merged_results
                .wall_impacts
                .append(&mut chunk_result.wall_impacts);
        }

        // Remove dead projectiles
        merged_results.to_remove.sort_unstable();
        merged_results.to_remove.dedup();
        let mut remove_iter = merged_results.to_remove.into_iter().peekable();
        let mut kept_projectiles = Vec::with_capacity(all_projectiles.len());
        let mut removed_ids = Vec::new();

        for (idx, proj) in all_projectiles.into_iter().enumerate() {
            if remove_iter
                .peek()
                .is_some_and(|remove_idx| *remove_idx == idx)
            {
                let _ = remove_iter.next();
                removed_ids.push(proj.id);
            } else {
                kept_projectiles.push(proj);
            }
        }

        trace!(
            "[Frame {}] Projectile processing complete: {} processed, {} hits, {} wall hits, {} removed",
            frame,
            total_projectiles,
            merged_results.hits.len(),
            merged_results.wall_hits.len(),
            removed_ids.len()
        );

        ProjectileResults {
            total_processed: total_projectiles,
            hits: merged_results.hits,
            wall_hits: merged_results.wall_hits,
            removed_projectile_ids: removed_ids,
            kept_projectiles,
            spatial_updates: merged_results.spatial_updates,
            wall_impacts: merged_results.wall_impacts,
        }
    }

    pub(super) async fn apply_projectile_results(&self, results: ProjectileResults) {
        let ProjectileResults {
            total_processed: _,
            hits,
            wall_hits,
            removed_projectile_ids,
            kept_projectiles,
            spatial_updates,
            wall_impacts,
        } = results;

        for wall_impact in wall_impacts {
            self.global_game_events
                .push(wall_impact, EventPriority::Normal);
        }
        if !spatial_updates.is_empty() {
            self.spatial_index
                .batch_update_projectiles(&spatial_updates);
        }
        self.commit_authoritative_projectile_state(kept_projectiles, &removed_projectile_ids);

        let destroyed_walls = self.apply_wall_damage_authoritative(&wall_hits);
        if destroyed_walls > 0 {
            trace!(
                "Applied authoritative wall damage from projectile results (destroyed_walls={}).",
                destroyed_walls
            );
        }

        // Process hits sequentially with a local health tracker to prevent the
        // concurrent damage race condition: two projectiles from parallel chunks
        // can both detect the same target as alive, producing duplicate hit entries.
        // The local tracker ensures that once a player's tracked HP reaches 0,
        // subsequent hits in the same tick see them as dead and skip kill logic.
        //
        // Map: target_id -> (tracked_health, tracked_shield, already_dead)
        let mut health_tracker: HashMap<PlayerID, (i32, i32, bool)> = HashMap::new();

        for (attacker_id, target_id, base_damage, weapon, hit_x, hit_y) in hits {
            let Some(attacker_state_entry) = self.player_manager.get_player_state(&attacker_id)
            else {
                continue;
            };
            if attacker_state_entry.is_spectator {
                continue;
            }
            let attacker_pos = Vec2::new(attacker_state_entry.x, attacker_state_entry.y);
            drop(attacker_state_entry);

            if !self.has_clear_line_of_sight(attacker_pos.x, attacker_pos.y, hit_x, hit_y) {
                continue;
            }

            let mut effective_base_damage = base_damage;
            if let Some(mut attacker_state_entry) =
                self.player_manager.get_player_state_mut(&attacker_id)
            {
                if attacker_state_entry.consume_dodge_shot_chain_bonus() {
                    effective_base_damage = ((effective_base_damage as f32)
                        * crate::core::constants::DODGE_TO_SHOT_DAMAGE_MULTIPLIER)
                        .round()
                        .max(1.0) as i32;
                    attacker_state_entry.mark_field_changed(FIELD_POWERUPS);
                    info!(
                        "Dodge->shot chain bonus: {} landed shot with +20% damage (base={} boosted={})",
                        attacker_state_entry.username,
                        base_damage,
                        effective_base_damage
                    );
                }
            }

            if let Some(mut target_state_entry) =
                self.player_manager.get_player_state_mut(&target_id)
            {
                // Initialize health tracker entry from authoritative state if not yet seen.
                let tracker = health_tracker.entry(target_id.clone()).or_insert_with(|| {
                    (
                        target_state_entry.health,
                        target_state_entry.shield_current,
                        !target_state_entry.alive,
                    )
                });

                // Skip if player is already dead (from authoritative state or earlier
                // hit in this tick) or is a spectator.
                if tracker.2 || target_state_entry.is_spectator {
                    continue;
                }

                let distance =
                    ((hit_x - attacker_pos.x).powi(2) + (hit_y - attacker_pos.y).powi(2)).sqrt();
                let is_headshot = Self::is_projectile_headshot(&target_state_entry, hit_x, hit_y);
                let damage_after_falloff = crate::systems::combat::weapons::apply_distance_falloff(
                    weapon,
                    effective_base_damage,
                    distance,
                );
                let falloff_multiplier = if effective_base_damage > 0 {
                    (damage_after_falloff as f32 / effective_base_damage as f32).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let mut damage = damage_after_falloff;
                if is_headshot {
                    damage = ((damage as f32) * crate::core::constants::HEADSHOT_DAMAGE_MULTIPLIER)
                        .round()
                        .max(1.0) as i32;
                }
                if damage <= 0 {
                    continue;
                }

                // Apply damage to local tracker first to determine if this hit
                // causes death, preventing duplicate kill events.
                let mut remaining_damage = damage;
                let shield_before = tracker.1;
                if tracker.1 > 0 {
                    let shield_absorbed = remaining_damage.min(tracker.1);
                    tracker.1 -= shield_absorbed;
                    remaining_damage -= shield_absorbed;
                }
                let shield_broken = shield_before > 0 && tracker.1 <= 0;
                tracker.0 = (tracker.0 - remaining_damage).max(0);
                let died_from_this_hit = tracker.0 == 0;
                if died_from_this_hit {
                    tracker.2 = true; // Mark as dead for subsequent hits in this tick
                }

                // Now apply the damage to the authoritative player state.
                // record_incoming_damage for assist tracking, then apply_damage
                // which handles shield, health, and die() internally.
                target_state_entry.record_incoming_damage(&attacker_id, damage, Instant::now());

                let died = target_state_entry.apply_damage(damage);
                let target_pos = Vec2::new(target_state_entry.x, target_state_entry.y);

                // Apply knockback on projectile hit
                if !died {
                    let dx = target_state_entry.x - attacker_pos.x;
                    let dy = target_state_entry.y - attacker_pos.y;
                    let dist = (dx * dx + dy * dy).sqrt().max(1.0);
                    let kb_force =
                        damage as f32 * crate::core::constants::KNOCKBACK_FORCE_PER_DAMAGE;
                    target_state_entry.velocity_x += (dx / dist) * kb_force;
                    target_state_entry.velocity_y += (dy / dist) * kb_force;

                    // Clamp velocity to prevent wall-clipping on stacked knockback
                    let speed = (target_state_entry.velocity_x.powi(2)
                        + target_state_entry.velocity_y.powi(2))
                    .sqrt();
                    if speed > crate::core::constants::KNOCKBACK_MAX_VELOCITY {
                        let scale = crate::core::constants::KNOCKBACK_MAX_VELOCITY / speed;
                        target_state_entry.velocity_x *= scale;
                        target_state_entry.velocity_y *= scale;
                    }

                    target_state_entry.mark_field_changed(FIELD_POSITION_ROTATION);
                }

                if let Some(mut attacker_state_entry) =
                    self.player_manager.get_player_state_mut(&attacker_id)
                {
                    attacker_state_entry.record_damage_dealt(damage);
                    attacker_state_entry.mark_field_changed(FIELD_SCORE_STATS);
                }

                self.global_game_events.push(
                    GameEvent::PlayerDamaged {
                        target_id: target_id.clone(),
                        attacker_id: Some(attacker_id.clone()),
                        damage,
                        weapon,
                        position: target_pos,
                        falloff_multiplier,
                    },
                    EventPriority::Normal,
                );
                if shield_broken {
                    self.global_game_events.push(
                        GameEvent::ShieldBroken {
                            player_id: target_id.clone(),
                            position: target_pos,
                        },
                        EventPriority::Normal,
                    );
                }

                // Only process kill logic if the local tracker determined this
                // specific hit caused the death. This prevents duplicate kills
                // when multiple projectiles hit the same target in one tick.
                if died_from_this_hit && died {
                    // Store flag carry state before clearing it
                    let victim_was_carrying_flag_id = target_state_entry.is_carrying_flag_team_id;
                    let victim_username = target_state_entry.username.clone();
                    let victim_streak = target_state_entry.current_streak;

                    // Clear flag carry state on the victim
                    if victim_was_carrying_flag_id != 0 {
                        target_state_entry.is_carrying_flag_team_id = 0;
                        target_state_entry.mark_field_changed(FIELD_FLAG);
                    }

                    // Handle death (existing logic from run_physics_update)
                    if attacker_id != target_id {
                        // Get team information for friendly fire check
                        let attacker_team = self
                            .player_manager
                            .get_player_state(&attacker_id)
                            .map(|p| p.team_id)
                            .unwrap_or(0);
                        let victim_team = target_state_entry.team_id;

                        if let Some(mut attacker_state_entry) =
                            self.player_manager.get_player_state_mut(&attacker_id)
                        {
                            attacker_state_entry.kills += 1;
                            attacker_state_entry.record_kill_with_weapon(weapon);

                            // Check for friendly fire
                            if attacker_team != 0
                                && victim_team != 0
                                && attacker_team == victim_team
                            {
                                // Friendly fire: double negative score
                                attacker_state_entry.score -= 200;
                                info!(
                                    "Friendly fire penalty: {} killed teammate {}, -200 score",
                                    attacker_state_entry.username, victim_username
                                );
                            } else {
                                let streak = self.advance_killstreak(&mut attacker_state_entry);
                                let base_kill_points =
                                    self.hot_zone_kill_points_at_position(target_pos);
                                let kill_points =
                                    Self::apply_momentum_score_bonus(base_kill_points, streak);
                                attacker_state_entry.score += kill_points;
                                if kill_points > base_kill_points {
                                    info!(
                                        "Momentum bonus: {} streak {} boosted kill score {} -> {}",
                                        attacker_state_entry.username,
                                        streak,
                                        base_kill_points,
                                        kill_points
                                    );
                                }
                                if base_kill_points > crate::core::constants::POINTS_PER_KILL {
                                    info!(
                                        "Hot zone bonus: {} gained {} points for elimination at ({:.1}, {:.1})",
                                        attacker_state_entry.username,
                                        kill_points,
                                        target_pos.x,
                                        target_pos.y
                                    );
                                }
                                if streak
                                    >= crate::core::constants::KILLSTREAK_DAMAGE_BOOST_THRESHOLD
                                {
                                    let a_pos =
                                        Vec2::new(attacker_state_entry.x, attacker_state_entry.y);
                                    drop(attacker_state_entry);
                                    self.global_game_events.push(
                                        GameEvent::Killstreak {
                                            player_id: attacker_id.clone(),
                                            streak,
                                            position: a_pos,
                                        },
                                        EventPriority::High,
                                    );
                                } else {
                                    attacker_state_entry.mark_field_changed(FIELD_SCORE_STATS);
                                    drop(attacker_state_entry);
                                }
                            }

                            // We may have already dropped attacker_state_entry above
                            if let Some(mut a) =
                                self.player_manager.get_player_state_mut(&attacker_id)
                            {
                                a.mark_field_changed(FIELD_SCORE_STATS);
                            }
                        }

                        // --- Assist tracking ---
                        {
                            let assist_ids =
                                target_state_entry.get_assist_ids(&attacker_id, Instant::now());
                            for assister_id in assist_ids {
                                if let Some(mut assister) =
                                    self.player_manager.get_player_state_mut(&assister_id)
                                {
                                    assister.score += crate::core::constants::POINTS_ASSIST;
                                    assister.mark_field_changed(FIELD_SCORE_STATS);
                                }
                                self.global_game_events.push(
                                    GameEvent::AssistKill {
                                        assister_id: assister_id.clone(),
                                        victim_id: target_id.clone(),
                                        points: crate::core::constants::POINTS_ASSIST,
                                    },
                                    EventPriority::Normal,
                                );
                            }
                        }
                    }

                    // Update team scores for TeamDeathmatch
                    {
                        let match_info_guard = self.match_info.read();
                        if match_info_guard.game_mode == fb::GameModeType::TeamDeathmatch {
                            drop(match_info_guard);

                            // Get attacker and victim team IDs
                            let attacker_team = self
                                .player_manager
                                .get_player_state(&attacker_id)
                                .map(|p| p.team_id)
                                .unwrap_or(0);
                            let victim_team = target_state_entry.team_id;

                            // Award point to attacker's team if it's a valid team kill
                            if attacker_team != 0
                                && victim_team != 0
                                && attacker_team != victim_team
                            {
                                let mut match_info_write = self.match_info.write();
                                let team_score = match_info_write
                                    .team_scores
                                    .entry(attacker_team)
                                    .or_insert(0);
                                *team_score += 1;
                                info!("Team {} scored! New score: {} (kill by player on victim from team {})",
                                      attacker_team, *team_score, victim_team);
                            }
                        }
                    }

                    self.global_game_events.push(
                        GameEvent::PlayerKilled {
                            victim_id: target_id.clone(),
                            killer_id: attacker_id.clone(),
                            weapon,
                            position: target_pos,
                        },
                        EventPriority::High,
                    );

                    // Losing team respawn reduction
                    {
                        let victim_team = target_state_entry.team_id;
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
                                if let Some(ref mut timer) = target_state_entry.respawn_timer {
                                    *timer = (*timer - reduction).max(0.5);
                                }
                            }
                        }
                    }

                    // Update kill feed
                    let killer_username = self
                        .player_manager
                        .get_player_state(&attacker_id)
                        .map_or_else(|| "World".to_string(), |p| p.username.clone());

                    self.capture_killcam_for_victim(
                        &target_id,
                        &victim_username,
                        &attacker_id,
                        weapon,
                    );

                    let kill_context = if self.kill_feed.read().is_empty() {
                        KillContext::FirstBlood
                    } else if victim_streak >= 5 {
                        KillContext::Shutdown
                    } else if distance >= 520.0 {
                        KillContext::LongRange
                    } else {
                        KillContext::Normal
                    };
                    self.push_kill_feed_entry_with_context(
                        killer_username.clone(),
                        victim_username.clone(),
                        weapon,
                        is_headshot,
                        kill_context,
                    );

                    // Handle flag dropping if victim was carrying a flag
                    if victim_was_carrying_flag_id != 0 {
                        let mut match_info_guard = self.match_info.write();

                        // Award score to attacker's team if applicable.
                        if let Some(attacker_state_for_score) =
                            self.player_manager.get_player_state(&attacker_id)
                        {
                            if attacker_state_for_score.team_id != 0
                                && attacker_state_for_score.team_id != victim_was_carrying_flag_id
                            {
                                let team_score_mut_ref = match_info_guard
                                    .team_scores
                                    .entry(attacker_state_for_score.team_id)
                                    .or_insert(0);
                                *team_score_mut_ref += 1;
                                info!(
                                    "Team {} scored +1 via projectile kill on flag carrier by {}",
                                    attacker_state_for_score.team_id,
                                    attacker_id.as_ref()
                                );
                            }
                        }

                        // Drop the flag
                        if let Some(flag_state) = match_info_guard
                            .flag_states
                            .get_mut(&victim_was_carrying_flag_id)
                        {
                            flag_state.status = fb::FlagStatus::Dropped;
                            flag_state.position = target_pos;
                            flag_state.carrier_id = None;
                            flag_state.respawn_timer = 30.0;

                            // Push flag dropped event after releasing match_info lock
                            drop(match_info_guard);

                            self.global_game_events.push(
                                GameEvent::FlagDropped {
                                    player_id: target_id.clone(),
                                    flag_team_id: victim_was_carrying_flag_id,
                                    position: target_pos,
                                },
                                EventPriority::High,
                            );

                            info!("(Projectile Kill) Flag of team {} dropped at ({:.1}, {:.1}) by {} killing {}",
                                  victim_was_carrying_flag_id, target_pos.x, target_pos.y, killer_username, victim_username);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn process_pickup_respawns_authoritative(
        &self,
        pickups: &mut [Pickup],
        delta_time: f32,
    ) {
        for pickup in pickups.iter_mut() {
            if !pickup.is_active {
                let mut emit_countdown: Option<(&'static str, &'static str)> = None;
                let mut emit_spawned: Option<(&'static str, &'static str)> = None;
                if let Some(timer) = &mut pickup.respawn_timer {
                    let previous_timer = *timer;
                    *timer -= delta_time;
                    if let Some((pickup_label, ping_kind)) =
                        Self::pickup_announcement_meta(&pickup.pickup_type)
                    {
                        if Self::should_emit_pickup_countdown(previous_timer, *timer) {
                            emit_countdown = Some((pickup_label, ping_kind));
                        }
                        if *timer <= 0.0 {
                            emit_spawned = Some((pickup_label, ping_kind));
                        }
                    }
                    if *timer <= 0.0 {
                        pickup.is_active = true;
                        pickup.respawn_timer = None;
                        self.upsert_pickup_in_partition_index(pickup);
                    }
                }
                if let Some((pickup_label, ping_kind)) = emit_countdown {
                    self.broadcast_pickup_spawn_notice_event(
                        pickup,
                        "countdown",
                        PICKUP_RESPAWN_ANNOUNCE_SECS,
                        pickup_label,
                        ping_kind,
                    );
                }
                if let Some((pickup_label, ping_kind)) = emit_spawned {
                    self.broadcast_pickup_spawn_notice_event(
                        pickup,
                        "spawned",
                        0.0,
                        pickup_label,
                        ping_kind,
                    );
                }
            }
        }
    }

    pub(super) fn collect_pickup_collection_candidates(
        &self,
        pickups: &[Pickup],
    ) -> Vec<PickupCollectionCandidate> {
        let mut players = Vec::with_capacity(self.player_manager.player_count());
        self.player_manager
            .for_each_player(|player_id, player_state| {
                if player_state.alive && !player_state.is_spectator {
                    players.push((player_id.clone(), player_state.x, player_state.y));
                }
            });
        collect_pickup_candidates(&players, pickups)
    }

    pub(super) fn apply_pickup_collection_authoritative(
        &self,
        pickups: &mut [Pickup],
        pickup_candidates: &[PickupCollectionCandidate],
    ) {
        let pickup_radius_sq = PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS;
        for pickup_candidate in pickup_candidates {
            let Some(pickup) = pickups.get_mut(pickup_candidate.pickup_index) else {
                continue;
            };
            if !pickup.is_active {
                continue;
            }

            let pickup_x = pickup.x;
            let pickup_y = pickup.y;
            let pickup_id = pickup.id;
            let pickup_type = pickup.pickup_type.clone();

            let mut collected = false;
            if let Some(mut player_state_for_pickup) = self
                .player_manager
                .get_player_state_mut(&pickup_candidate.player_id)
            {
                if !player_state_for_pickup.alive {
                    continue;
                }
                if player_state_for_pickup.is_spectator {
                    continue;
                }

                let dx = player_state_for_pickup.x - pickup_x;
                let dy = player_state_for_pickup.y - pickup_y;
                if dx * dx + dy * dy > pickup_radius_sq {
                    continue;
                }

                collected = apply_pickup_effect(&mut player_state_for_pickup, &pickup_type);
            }

            if !collected {
                continue;
            }

            pickup.is_active = false;
            pickup.respawn_timer = Some(pickup.get_respawn_duration());
            let pickup_partition_state = pickup.clone();
            self.upsert_pickup_in_partition_index(&pickup_partition_state);
            self.global_game_events.push(
                GameEvent::PowerupCollected {
                    player_id: pickup_candidate.player_id.clone(),
                    pickup_id,
                    pickup_type,
                    position: Vec2::new(pickup_x, pickup_y),
                },
                EventPriority::Normal,
            );
        }
    }

    pub(super) async fn process_pickup_respawns(&self, delta_time: f32) {
        let mut pickups_guard = self.pickups.write();
        self.process_pickup_respawns_authoritative(pickups_guard.as_mut_slice(), delta_time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickup_countdown_emits_only_on_threshold_crossing() {
        assert!(MassiveGameServer::should_emit_pickup_countdown(7.0, 4.9));
        assert!(!MassiveGameServer::should_emit_pickup_countdown(4.9, 4.0));
        assert!(!MassiveGameServer::should_emit_pickup_countdown(7.0, 0.0));
        assert!(!MassiveGameServer::should_emit_pickup_countdown(7.0, -1.0));
    }

    #[test]
    fn pickup_announcement_meta_filters_common_pickups() {
        assert!(MassiveGameServer::pickup_announcement_meta(&CorePickupType::Health).is_none());
        assert!(MassiveGameServer::pickup_announcement_meta(&CorePickupType::Ammo).is_none());
        assert_eq!(
            MassiveGameServer::pickup_announcement_meta(&CorePickupType::SpeedBoost),
            Some(("Speed Boost", "defend"))
        );
        assert_eq!(
            MassiveGameServer::pickup_announcement_meta(&CorePickupType::DamageBoost),
            Some(("Damage Boost", "defend"))
        );
        assert_eq!(
            MassiveGameServer::pickup_announcement_meta(&CorePickupType::Shield),
            Some(("Shield", "defend"))
        );
        assert_eq!(
            MassiveGameServer::pickup_announcement_meta(&CorePickupType::WeaponCrate(
                ServerWeaponType::Sniper
            )),
            Some(("Weapon Crate", "defend"))
        );
    }
}
