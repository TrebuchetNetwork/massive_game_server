use super::*;

impl MassiveGameServer {
    pub async fn run_physics_update(&self, delta_time: f32) {
        let physics_start_time = Instant::now();
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);

        // Stage 1: Wall Respawns (example)
        let respawn_stage_start = Instant::now();
        let respawned_walls = if frame % 30 == 0 {
            //
            let templates = self.wall_respawn_manager.as_ref().check_respawns(); //
            if !templates.is_empty() {
                // CHANGED to debug!
                debug!(
                    "[Frame {}]: Respawning {} walls (took {:?})",
                    frame,
                    templates.len(),
                    respawn_stage_start.elapsed()
                );
                self.process_wall_respawns(templates).await //
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Update wall spatial index if walls were respawned, destroyed, or if it needs periodic rebuild
        let destroyed_walls_count = self.destroyed_wall_ids_this_tick.read().len();
        let needs_wall_index_rebuild = !respawned_walls.is_empty()
            || destroyed_walls_count > 0
            || self.wall_spatial_index.needs_rebuild(frame, 150); // Rebuild every 150 frames

        if needs_wall_index_rebuild {
            let index_rebuild_start = Instant::now();
            let active_walls = self.collect_active_walls_optimized();
            self.wall_spatial_index.rebuild(&active_walls, frame);
            debug!(
                "[Frame {}] Wall spatial index rebuilt in {:?} (respawned: {}, destroyed: {})",
                frame,
                index_rebuild_start.elapsed(),
                respawned_walls.len(),
                destroyed_walls_count
            );
        }

        // Stage 2: Collect Active Walls
        let collect_walls_start = Instant::now();
        let active_walls = self.get_active_walls_cached(frame).await; //
                                                                      // CHANGED to debug!
        debug!(
            "Frame {}: Collected {} active walls (took {:?})",
            frame,
            active_walls.len(),
            collect_walls_start.elapsed()
        );

        // Stage 3: Process Player Physics
        let player_physics_start = Instant::now();
        let player_updates = self
            .process_player_physics_parallel(&active_walls, delta_time)
            .await; //
                    // CHANGED to debug!
        debug!(
            "Frame {}: Processed {} player physics updates (took {:?})",
            frame,
            player_updates.players_to_respawn.len() + player_updates.alive_count,
            player_physics_start.elapsed()
        );

        // Stage 4: Apply Player Updates
        let apply_updates_start = Instant::now();
        self.apply_player_updates(player_updates, &active_walls).await; //
                                                         // CHANGED to debug!
        debug!(
            "Frame {}: Applied player updates (took {:?})",
            frame,
            apply_updates_start.elapsed()
        );

        // Stage 5: Process Projectiles
        let projectiles_start = Instant::now();
        let projectile_results = self
            .process_projectiles_optimized(&active_walls, delta_time)
            .await; //
                    // CHANGED to debug!
        debug!(
            "Frame {}: Processed {} projectiles, {} hits, {} removed (took {:?})",
            frame,
            projectile_results.total_processed,
            projectile_results.hits.len(),
            projectile_results.removed_projectile_ids.len(),
            projectiles_start.elapsed()
        );

        // Stage 6: Apply Projectile Results
        let apply_projectiles_start = Instant::now();
        self.apply_projectile_results(projectile_results).await; //
                                                                 // CHANGED to debug!
        debug!(
            "Frame {}: Applied projectile results (took {:?})",
            frame,
            apply_projectiles_start.elapsed()
        );

        // Stage 7: Process Pickups
        let pickups_start = Instant::now();
        self.process_pickup_respawns(delta_time).await; //
                                                        // CHANGED to debug!
        debug!(
            "Frame {}: Processed pickups (took {:?})",
            frame,
            pickups_start.elapsed()
        );

        // This overall timing can remain info if you want a less frequent summary,
        // but if it's per-frame, debug is better.
        // For a true summary, this should be outside this function, logged less often.
        // Let's make it debug for now.
        debug!(
            "Frame {}: TOTAL physics update took {:?}",
            frame,
            physics_start_time.elapsed()
        );
        metrics::record_subsystem_time("physics", physics_start_time.elapsed().as_secs_f64());

        // The specific log "Collected {} walls from {} partitions"
        // in `collect_active_walls_optimized` can also be changed to `debug!`.
        // In src/server/instance.rs, inside `collect_active_walls_optimized`:
        // Change:
        // info!("Collected {} walls from {} partitions", all_walls.len(), partitions.len()); //
        // To:
        // debug!("Collected {} walls from {} partitions", all_walls.len(), partitions.len());
    }

    // Helper methods:
    async fn process_wall_respawns(&self, templates: Vec<Wall>) -> Vec<EntityId> {
        let mut updated_walls_guard = self.updated_walls_this_tick.write();
        let mut respawned_ids = Vec::with_capacity(templates.len());

        for wall_template in templates {
            let partition_idx = self.world_partition_manager.get_partition_index_for_point(
                wall_template.x + wall_template.width / 2.0,
                wall_template.y + wall_template.height / 2.0,
            );

            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                if partition.respawn_destructible_wall(wall_template.id) {
                    if let Some(respawned_wall_state) = partition.get_wall(wall_template.id) {
                        updated_walls_guard.insert(wall_template.id, respawned_wall_state);
                        respawned_ids.push(wall_template.id);
                    }
                }
            }
        }

        // After respawning walls, update all player AOIs
        if !respawned_ids.is_empty() {
            info!(
                "[Wall Respawn] Updating player AOIs for {} respawned walls",
                respawned_ids.len()
            );
            for mut aoi_entry in self.player_aois.iter_mut() {
                let aoi = aoi_entry.value_mut();
                for wall_id in &respawned_ids {
                    if !aoi.visible_walls.contains(wall_id) {
                        aoi.visible_walls.insert(*wall_id);
                        debug!(
                            "[Wall Respawn] Added respawned wall {} to player's AOI",
                            wall_id
                        );
                    }
                }
            }
        }

        respawned_ids
    }
    pub(super) async fn get_active_walls_cached(&self, frame: u64) -> Arc<Vec<Wall>> {
        // Cache walls for a few frames since they don't change often
        static WALL_CACHE: OnceCell<Arc<ParkingLotRwLock<(u64, Arc<Vec<Wall>>)>>> = OnceCell::new();
        let cache =
            WALL_CACHE.get_or_init(|| Arc::new(ParkingLotRwLock::new((0, Arc::new(Vec::new())))));

        // Keep a read lock while deciding whether to upgrade, eliminating unlock/relock races.
        let cache_read = cache.upgradable_read();
        if cache_read.0 + 5 > frame {
            return cache_read.1.clone();
        }

        // Rebuild cache after atomically upgrading to write access.
        let mut cache_write = parking_lot::RwLockUpgradableReadGuard::upgrade(cache_read);
        if cache_write.0 + 5 > frame {
            return cache_write.1.clone();
        }

        let walls = Arc::new(self.collect_active_walls_optimized());
        cache_write.0 = frame;
        cache_write.1 = walls.clone();
        walls
    }

    // server/src/server/instance.rs

    // server/src/server/instance.rs
    // server/src/server/instance.rs
    pub(super) fn collect_active_walls_optimized(&self) -> Vec<Wall> {
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        // CACHED_WALLS is static and initialized in new()

        let cache_entry_arc = CACHED_WALLS
            .get()
            .expect("Wall cache should have been initialized in MassiveGameServer::new()");

        let structural_walls_from_cache = {
            // Read all structural walls
            let guard = cache_entry_arc.read();
            // Check if cache needs refresh based on frame number.
            // This simple check might need to be more sophisticated if walls change health often
            // outside of just being destroyed/respawned, but for now, let's assume
            // the cache primarily stores the structural layout.
            if guard.0 == frame || (guard.0 != u64::MAX && guard.0 >= frame.saturating_sub(10)) {
                debug!(
                    "[Frame {}] Using cached structural walls (cache frame {}, count {}).",
                    frame,
                    guard.0,
                    guard.1.len()
                );
                guard.1.clone()
            } else {
                // Cache is stale, need to rebuild it
                drop(guard); // Release read lock
                let mut write_guard = cache_entry_arc.write();
                // Double check after acquiring write lock
                if write_guard.0 == frame
                    || (write_guard.0 != u64::MAX && write_guard.0 >= frame.saturating_sub(10))
                {
                    debug!(
                        "[Frame {}] Cache updated by another thread. Using new structural walls.",
                        frame
                    );
                    write_guard.1.clone()
                } else {
                    info!(
                        "[Frame {}] Rebuilding structural wall cache (was for frame {}).",
                        frame, write_guard.0
                    );
                    let mut new_cache_walls = Vec::new();
                    let partitions = self.world_partition_manager.get_partitions_for_processing();
                    for partition in &partitions {
                        for entry in partition.all_walls_in_partition.iter() {
                            new_cache_walls.push(entry.value().clone());
                        }
                    }
                    info!(
                        "[Frame {}] Structural wall cache rebuilt with {} walls.",
                        frame,
                        new_cache_walls.len()
                    );
                    write_guard.0 = frame;
                    write_guard.1 = new_cache_walls.clone();
                    new_cache_walls
                }
            }
        };

        // Now filter these structural walls for "activeness"
        // IMPORTANT: For destructible walls, we need to check their CURRENT health from partitions, not cached health
        let mut active_walls = Vec::new();

        for cached_wall in structural_walls_from_cache {
            if !cached_wall.is_destructible {
                // Non-destructible walls are always active
                active_walls.push(cached_wall);
            } else {
                // For destructible walls, check current health from the partition
                let mut wall_is_active = false;
                let wall_center_x = cached_wall.x + cached_wall.width / 2.0;
                let wall_center_y = cached_wall.y + cached_wall.height / 2.0;
                let partition_idx = self
                    .world_partition_manager
                    .get_partition_index_for_point(wall_center_x, wall_center_y);

                if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                    if let Some(current_wall) = partition.get_wall(cached_wall.id) {
                        if current_wall.current_health > 0 {
                            // Use the current wall state, not the cached one
                            active_walls.push(current_wall);
                            wall_is_active = true;
                        }
                    }
                }

                if !wall_is_active {
                    debug!(
                        "[Frame {}] Filtering out destroyed wall {} (health: 0)",
                        frame, cached_wall.id
                    );
                }
            }
        }

        // This log will show the count of *active* walls
        debug!(
            "[Frame {}] Collected {} active walls.",
            frame,
            active_walls.len()
        );
        active_walls
    }

    // Helper function to get default PlayerAoI
    pub(super) fn get_empty_player_aoi() -> PlayerAoI {
        PlayerAoI {
            visible_players: HashSet::new(),
            visible_projectiles: HashSet::new(),
            visible_pickups: HashSet::new(),
            visible_walls: HashSet::new(),
            last_update: Instant::now(), // Added this field
        }
    }

    async fn process_player_physics_parallel(
        &self,
        walls: &[Wall],
        delta_time: f32,
    ) -> PlayerPhysicsResults {
        let wall_arc = Arc::new(walls.to_vec());
        let mut all_to_respawn = Vec::new();
        let mut total_alive = 0;
        let sample_timestamp_ms = self.get_server_timestamp_ms();

        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        if frame % 120 == 0 {
            self.prune_runtime_tracking_state();
        }

        // Process all players using for_each_player_mut
        self.player_manager
            .for_each_player_mut(|player_id, player_state| {
                // Update timers
                player_state.update_timers(delta_time);

                if player_state.alive {
                    total_alive += 1;
                    // Process movement with optimized collision
                    self.process_player_movement_optimized(player_state, &wall_arc, delta_time);
                    self.record_player_position_sample(
                        player_id,
                        sample_timestamp_ms,
                        player_state.x,
                        player_state.y,
                    );
                } else if player_state.respawn_timer == Some(0.0) {
                    all_to_respawn.push((player_id.clone(), player_state.team_id));
                }
            });

        PlayerPhysicsResults {
            players_to_respawn: all_to_respawn,
            alive_count: total_alive,
        }
    }

    fn process_player_movement_optimized(
        &self,
        player_state: &mut PlayerState,
        _walls: &[Wall],
        delta_time: f32,
    ) {
        let old_x = player_state.x;
        let old_y = player_state.y;

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
        player_state.x += player_state.velocity_x * delta_time;
        player_state.y += player_state.velocity_y * delta_time;

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
                // Collision detected - revert position
                player_state.x = old_x;
                player_state.y = old_y;
                player_state.velocity_x = 0.0;
                player_state.velocity_y = 0.0;
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
                return;
            }
        }

        // Prevent player stacking by rejecting moves that overlap nearby players.
        let min_player_distance = PLAYER_RADIUS * 2.0;
        let min_player_distance_sq = min_player_distance * min_player_distance;
        let nearby_players = self.spatial_index.query_nearby_players_with_positions(
            player_state.x,
            player_state.y,
            min_player_distance + 8.0,
        );
        for (other_player_id, other_x, other_y) in nearby_players {
            if other_player_id == player_state.id {
                continue;
            }
            let dist_sq = (player_state.x - other_x).powi(2) + (player_state.y - other_y).powi(2);
            if dist_sq < min_player_distance_sq {
                player_state.x = old_x;
                player_state.y = old_y;
                player_state.velocity_x = 0.0;
                player_state.velocity_y = 0.0;
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
                return;
            }
        }

        // Anti-cheat validation
        let max_speed_dist = PLAYER_BASE_SPEED * MAX_PLAYER_SPEED_MULTIPLIER * delta_time;
        // Fixed slack per tick allowed excessive burst distance; scale with expected movement instead.
        let adaptive_slack = (max_speed_dist * 0.35).clamp(1.0, MAX_POSITION_DELTA_SLACK);
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

        // Mark as changed if moved
        if (old_x - player_state.x).abs() > 0.01 || (old_y - player_state.y).abs() > 0.01 {
            player_state.mark_field_changed(FIELD_POSITION_ROTATION);
        }
    }

    /*async fn process_projectiles_optimized(&self, _walls: &[Wall], delta_time: f32) -> ProjectileResults {
        let mut projectiles_guard = self.projectiles.write();
        let mut results = ProjectileResults {
            total_processed: projectiles_guard.len(),
            hits: Vec::new(),
            wall_hits: Vec::new(),
            to_remove: Vec::new(),
        };

        let mut destroyed_wall_ids_guard = self.destroyed_wall_ids_this_tick.write();

        // Process projectiles
        for (idx, proj) in projectiles_guard.iter_mut().enumerate() {
            // Update position
            proj.x += proj.velocity_x * delta_time;
            proj.y += proj.velocity_y * delta_time;

            // Quick bounds check
            if proj.x < WORLD_MIN_X || proj.x > WORLD_MAX_X ||
               proj.y < WORLD_MIN_Y || proj.y > WORLD_MAX_Y ||
               proj.should_remove() {
                results.to_remove.push(idx);
                continue;
            }

            // Check wall collisions
            let proj_partition_idx = self.world_partition_manager.get_partition_index_for_point(proj.x, proj.y);
            if let Some(partition) = self.world_partition_manager.get_partition(proj_partition_idx) {
                let mut hit_wall = false;
                for mut wall_entry in partition.all_walls_in_partition.iter_mut() {
                    let wall = wall_entry.value_mut();
                    if wall.is_destructible && wall.current_health <= 0 { continue; }

                    if proj.x >= wall.x && proj.x <= wall.x + wall.width &&
                       proj.y >= wall.y && proj.y <= wall.y + wall.height {

                        if let Some(event) = crate::systems::physics::collision::handle_projectile_wall_collision(
                            proj, wall.id, wall, &self.wall_respawn_manager
                        ) {
                            self.global_game_events.push(event.clone(), EventPriority::Normal);
                            if let GameEvent::WallDestroyed { wall_id: destroyed_id, .. } = event {
                                destroyed_wall_ids_guard.insert(destroyed_id);
                            }
                        }
                        results.to_remove.push(idx);
                        hit_wall = true;
                        break;
                    }
                }

                if !hit_wall {
                    // Check player collisions
                    let nearby_players = self.spatial_index.query_nearby_players(proj.x, proj.y, 100.0);
                    for target_id in nearby_players {
                        if target_id == proj.owner_id { continue; }

                        if let Some(target_state) = self.player_manager.get_player_state(&target_id) {
                            if !target_state.alive { continue; }

                            let dist_sq = (target_state.x - proj.x).powi(2) + (target_state.y - proj.y).powi(2);
                            if dist_sq < PLAYER_RADIUS.powi(2) {
                                results.hits.push((
                                    proj.owner_id.clone(),
                                    target_id.clone(),
                                    proj.damage,
                                    proj.weapon_type
                                ));
                                results.to_remove.push(idx);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Remove projectiles in reverse order
        results.to_remove.sort_unstable();
        results.to_remove.dedup();
        for &idx in results.to_remove.iter().rev() {
            if idx < projectiles_guard.len() {
                projectiles_guard.swap_remove(idx);
            }
        }

        drop(projectiles_guard);
        drop(destroyed_wall_ids_guard);
        results
    }*/

    async fn apply_player_updates(&self, updates: PlayerPhysicsResults, active_walls: &[Wall]) {
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
                self.global_game_events.push(
                    GameEvent::PlayerJoined {
                        player_id: player_id.clone(),
                    },
                    EventPriority::High,
                );
            }
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

        let mut projectiles_guard = self.projectiles.write();
        projectiles_guard.extend(queued_projectiles);
    }

    fn take_authoritative_projectiles_for_processing(&self) -> Vec<Projectile> {
        let mut guard = self.projectiles.write();
        std::mem::take(&mut *guard)
    }

    fn commit_authoritative_projectile_state(
        &self,
        kept_projectiles: Vec<Projectile>,
        removed_ids: &[EntityId],
    ) {
        for proj_id in removed_ids {
            self.spatial_index.remove_projectile(proj_id);
        }

        let mut guard = self.projectiles.write();
        *guard = kept_projectiles;
    }
    pub(super) fn process_pickup_respawns_authoritative(&self, pickups: &mut [Pickup], delta_time: f32) {
        for pickup in pickups.iter_mut() {
            if !pickup.is_active {
                if let Some(timer) = &mut pickup.respawn_timer {
                    *timer -= delta_time;
                    if *timer <= 0.0 {
                        pickup.is_active = true;
                        pickup.respawn_timer = None;
                        self.upsert_pickup_in_partition_index(pickup);
                    }
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
                if player_state.alive {
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

    // In massive_game_server/server/src/server/instance.rs

    async fn process_projectiles_optimized(
        &self,
        _walls: &[Wall],
        delta_time: f32,
    ) -> ProjectileResults {
        use rayon::prelude::*;
        #[derive(Default)]
        struct PartitionWallAabbCache {
            ids: Vec<EntityId>,
            min_xs: Vec<f32>,
            max_xs: Vec<f32>,
            min_ys: Vec<f32>,
            max_ys: Vec<f32>,
            destructible: Vec<bool>,
        }

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

        // Build per-partition wall AABB caches once per tick and share across rayon workers.
        let partition_wall_caches: Arc<Vec<PartitionWallAabbCache>> = {
            let partitions = self.world_partition_manager.get_partitions_for_processing();
            let mut caches = Vec::with_capacity(partitions.len());
            for partition in partitions {
                let mut cache = PartitionWallAabbCache::default();
                for wall_entry in partition.all_walls_in_partition.iter() {
                    let wall = wall_entry.value();
                    if wall.is_destructible && wall.current_health <= 0 {
                        continue;
                    }
                    cache.ids.push(wall.id);
                    cache.min_xs.push(wall.x);
                    cache.max_xs.push(wall.x + wall.width);
                    cache.min_ys.push(wall.y);
                    cache.max_ys.push(wall.y + wall.height);
                    cache.destructible.push(wall.is_destructible);
                }
                caches.push(cache);
            }
            Arc::new(caches)
        };
        let lag_compensation_target_ms = self
            .get_server_timestamp_ms()
            .saturating_sub(self.lag_compensation_ms);

        // Process projectiles in parallel chunks
        let chunk_size = 50.max(total_projectiles / rayon::current_num_threads());

        let partition_wall_caches_ref = Arc::clone(&partition_wall_caches);
        let chunk_results: Vec<ProjectileChunkResults> = all_projectiles
            .par_chunks_mut(chunk_size)
            .enumerate()
            .map(|(chunk_idx, chunk)| {
                let mut local_results = ProjectileChunkResults::default();
                let chunk_start_idx = chunk_idx * chunk_size;
                let mut target_ids: Vec<PlayerID> = Vec::with_capacity(32);
                let mut target_xs: Vec<f32> = Vec::with_capacity(32);
                let mut target_ys: Vec<f32> = Vec::with_capacity(32);
                let mut candidate_partition_indices: Vec<usize> = Vec::with_capacity(16);

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

                    // Continuous wall collision detection across all partitions touched by
                    // the projectile segment this tick.
                    candidate_partition_indices.clear();
                    self.world_partition_manager
                        .collect_partition_indices_for_bounds(
                            old_x.min(proj.x),
                            old_x.max(proj.x),
                            old_y.min(proj.y),
                            old_y.max(proj.y),
                            &mut candidate_partition_indices,
                        );

                    let mut earliest_wall_hit_t: Option<f32> = None;
                    let mut earliest_wall_id: EntityId = 0;
                    let mut earliest_wall_destructible = false;

                    for partition_idx in &candidate_partition_indices {
                        let Some(wall_cache) = partition_wall_caches_ref.get(*partition_idx) else {
                            continue;
                        };

                        for wall_idx in 0..wall_cache.ids.len() {
                            let Some(hit_t) = segment_first_hit_fraction_with_aabb(
                                old_x,
                                old_y,
                                proj.x,
                                proj.y,
                                wall_cache.min_xs[wall_idx],
                                wall_cache.max_xs[wall_idx],
                                wall_cache.min_ys[wall_idx],
                                wall_cache.max_ys[wall_idx],
                            ) else {
                                continue;
                            };

                            let is_earlier_hit = match earliest_wall_hit_t {
                                Some(existing_t) => hit_t < existing_t,
                                None => true,
                            };
                            if is_earlier_hit {
                                earliest_wall_hit_t = Some(hit_t);
                                earliest_wall_id = wall_cache.ids[wall_idx];
                                earliest_wall_destructible = wall_cache.destructible[wall_idx];
                            }
                        }
                    }

                    if let Some(hit_t) = earliest_wall_hit_t {
                        let hit_x = old_x + (proj.x - old_x) * hit_t;
                        let hit_y = old_y + (proj.y - old_y) * hit_t;
                        proj.x = hit_x;
                        proj.y = hit_y;

                        if earliest_wall_destructible {
                            local_results
                                .wall_hits
                                .push((earliest_wall_id, proj.damage));
                            local_results.wall_impacts.push(GameEvent::WallImpact {
                                position: Vec2::new(hit_x, hit_y),
                                wall_id: earliest_wall_id,
                                damage: proj.damage,
                            });
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

    fn apply_wall_damage_authoritative(&self, wall_hits: &[(EntityId, i32)]) -> usize {
        if wall_hits.is_empty() {
            return 0;
        }

        let mut wall_damage_by_id: HashMap<EntityId, i32> = HashMap::new();
        for (wall_id, damage) in wall_hits {
            *wall_damage_by_id.entry(*wall_id).or_insert(0) += *damage;
        }

        let partitions_for_lookup = self.world_partition_manager.get_partitions_for_processing();
        let mut wall_partition_lookup: HashMap<EntityId, usize> = HashMap::new();
        for (partition_idx, partition) in partitions_for_lookup.iter().enumerate() {
            for wall_entry in partition.all_walls_in_partition.iter() {
                wall_partition_lookup.insert(*wall_entry.key(), partition_idx);
            }
        }

        let mut destroyed_count = 0usize;
        for (wall_id, total_damage) in wall_damage_by_id {
            if let Some(partition_idx) = wall_partition_lookup.get(&wall_id).copied() {
                if let Some(partition) = partitions_for_lookup.get(partition_idx) {
                    if let Some((destroyed, pos)) =
                        partition.damage_destructible_wall(wall_id, total_damage)
                    {
                        if destroyed {
                            destroyed_count += 1;
                            self.global_game_events.push(
                                GameEvent::WallDestroyed {
                                    wall_id,
                                    position: pos,
                                },
                                EventPriority::High,
                            );
                            self.destroyed_wall_ids_this_tick.write().insert(wall_id);
                            self.wall_respawn_manager.wall_destroyed(wall_id);
                        }
                    }
                }
            }
        }

        destroyed_count
    }

    async fn apply_projectile_results(&self, results: ProjectileResults) {
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

        // Process hits - reuse existing game logic
        for (attacker_id, target_id, damage, weapon) in hits {
            if let Some(mut target_state_entry) =
                self.player_manager.get_player_state_mut(&target_id)
            {
                if target_state_entry.alive {
                    let died = target_state_entry.apply_damage(damage);
                    let target_pos = Vec2::new(target_state_entry.x, target_state_entry.y);

                    self.global_game_events.push(
                        GameEvent::PlayerDamaged {
                            target_id: target_id.clone(),
                            attacker_id: Some(attacker_id.clone()),
                            damage,
                            weapon,
                            position: target_pos,
                        },
                        EventPriority::Normal,
                    );

                    if died {
                        // Store flag carry state before clearing it
                        let victim_was_carrying_flag_id =
                            target_state_entry.is_carrying_flag_team_id;
                        let victim_username = target_state_entry.username.clone();

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
                                    // Normal kill: positive score
                                    attacker_state_entry.score += 100;
                                }

                                attacker_state_entry.mark_field_changed(FIELD_SCORE_STATS);
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

                        // Update kill feed
                        let killer_username = self
                            .player_manager
                            .get_player_state(&attacker_id)
                            .map_or_else(|| "World".to_string(), |p| p.username.clone());

                        self.push_kill_feed_entry(
                            killer_username.clone(),
                            victim_username.clone(),
                            weapon,
                        );

                        // Handle flag dropping if victim was carrying a flag
                        if victim_was_carrying_flag_id != 0 {
                            let mut match_info_guard = self.match_info.write();

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
    }

    async fn process_pickup_respawns(&self, delta_time: f32) {
        let mut pickups_guard = self.pickups.write();
        self.process_pickup_respawns_authoritative(pickups_guard.as_mut_slice(), delta_time);
    }

    fn get_enemy_positions_for_team(&self, team_id: u8) -> Vec<(Vec2, PlayerID)> {
        let mut enemies = Vec::with_capacity(50);
        self.player_manager.for_each_player(|id, state| {
            if state.alive && state.team_id != team_id && state.team_id != 0 {
                enemies.push((Vec2::new(state.x, state.y), id.clone()));
            }
        });
        enemies
    }

    pub fn collect_all_walls_current_state(&self) -> Vec<Wall> {
        let mut all_walls = Vec::new();
        for partition_arc in self.world_partition_manager.get_partitions_for_processing() {
            partition_arc
                .all_walls_in_partition
                .iter()
                .for_each(|wall_entry| {
                    let wall = wall_entry.value();
                    // Send ALL walls including destroyed ones - client needs to render them as rubble/obstacles
                    all_walls.push(wall.clone());
                });
        }
        all_walls
    }
}
