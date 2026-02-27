use super::*;

impl MassiveGameServer {
    pub(super) async fn process_wall_respawns(&self, templates: Vec<Wall>) -> Vec<EntityId> {
        let mut updated_walls_guard = self.updated_walls_this_tick.write();
        let mut respawned_ids = Vec::with_capacity(templates.len());

        for wall_template in templates {
            let removed_fragment_child_ids =
                self.clear_progressive_fragments_for_parent(wall_template.id);
            if !removed_fragment_child_ids.is_empty() {
                let mut destroyed_guard = self.destroyed_wall_ids_this_tick.write();
                for child_id in removed_fragment_child_ids {
                    destroyed_guard.insert(child_id);
                }
            }

            let partition_idx = self.world_partition_manager.get_partition_index_for_point(
                wall_template.x + wall_template.width / 2.0,
                wall_template.y + wall_template.height / 2.0,
            );

            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                let respawned_wall_state = if partition.respawn_destructible_wall(wall_template.id)
                {
                    partition.get_wall(wall_template.id)
                } else {
                    let mut restored = wall_template.clone();
                    restored.current_health = restored.max_health;
                    partition.upsert_wall(restored.clone());
                    Some(restored)
                };
                if let Some(respawned_wall_state) = respawned_wall_state {
                    updated_walls_guard.insert(wall_template.id, respawned_wall_state);
                    respawned_ids.push(wall_template.id);
                }
            }
        }

        if !respawned_ids.is_empty() {
            self.invalidate_structural_wall_cache();
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
        #[allow(clippy::type_complexity)]
        static WALL_CACHE: OnceCell<Arc<ParkingLotRwLock<(u64, Arc<Vec<Wall>>)>>> = OnceCell::new();
        let cache =
            WALL_CACHE.get_or_init(|| Arc::new(ParkingLotRwLock::new((0, Arc::new(Vec::new())))));
        let force_refresh = !self.updated_walls_this_tick.read().is_empty()
            || !self.destroyed_wall_ids_this_tick.read().is_empty();

        // Keep a read lock while deciding whether to upgrade, eliminating unlock/relock races.
        let cache_read = cache.upgradable_read();
        if !force_refresh && cache_read.0 + 5 > frame {
            return cache_read.1.clone();
        }

        // Rebuild cache after atomically upgrading to write access.
        let mut cache_write = parking_lot::RwLockUpgradableReadGuard::upgrade(cache_read);
        if !force_refresh && cache_write.0 + 5 > frame {
            return cache_write.1.clone();
        }

        let walls = Arc::new(self.collect_active_walls_optimized());
        cache_write.0 = frame;
        cache_write.1 = walls.clone();
        walls
    }

    pub(super) fn collect_active_walls_optimized(&self) -> Vec<Wall> {
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);

        let cache_entry_arc = match CACHED_WALLS.get() {
            Some(arc) => arc,
            None => {
                warn!("Wall cache not yet initialized – rebuilding from partitions");
                let mut fallback = Vec::new();
                let partitions = self.world_partition_manager.get_partitions_for_processing();
                for partition in &partitions {
                    for entry in partition.all_walls_in_partition.iter() {
                        fallback.push(entry.value().clone());
                    }
                }
                return fallback;
            }
        };

        let structural_walls_from_cache = {
            let guard = cache_entry_arc.read();
            if guard.0 == frame || (guard.0 != u64::MAX && guard.0 >= frame.saturating_sub(10)) {
                debug!(
                    "[Frame {}] Using cached structural walls (cache frame {}, count {}).",
                    frame,
                    guard.0,
                    guard.1.len()
                );
                guard.1.clone()
            } else {
                drop(guard);
                let mut write_guard = cache_entry_arc.write();
                if write_guard.0 == frame
                    || (write_guard.0 != u64::MAX && write_guard.0 >= frame.saturating_sub(10))
                {
                    debug!(
                        "[Frame {}] Cache updated by another thread. Using new structural walls.",
                        frame
                    );
                    write_guard.1.clone()
                } else {
                    debug!(
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
                    debug!(
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

        let mut active_walls = Vec::new();

        for cached_wall in structural_walls_from_cache {
            if !cached_wall.is_destructible {
                active_walls.push(cached_wall);
            } else {
                let mut wall_is_active = false;
                let wall_center_x = cached_wall.x + cached_wall.width / 2.0;
                let wall_center_y = cached_wall.y + cached_wall.height / 2.0;
                let partition_idx = self
                    .world_partition_manager
                    .get_partition_index_for_point(wall_center_x, wall_center_y);

                if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                    if let Some(current_wall) = partition.get_wall(cached_wall.id) {
                        if current_wall.current_health > 0 {
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

        debug!(
            "[Frame {}] Collected {} active walls.",
            frame,
            active_walls.len()
        );
        active_walls
    }

    pub(super) fn get_empty_player_aoi() -> PlayerAoI {
        PlayerAoI {
            visible_players: HashSet::new(),
            visible_projectiles: HashSet::new(),
            visible_pickups: HashSet::new(),
            visible_walls: HashSet::new(),
            last_update: Instant::now(),
        }
    }

    pub(super) fn invalidate_structural_wall_cache(&self) {
        if let Some(cache) = CACHED_WALLS.get() {
            cache.write().0 = u64::MAX;
        }
    }

    pub(super) fn resolve_progressive_parent_wall_id(&self, wall_id: EntityId) -> EntityId {
        if !self.progressive_destructible_enabled {
            return wall_id;
        }
        self.progressive_destructible_state
            .read()
            .child_to_parent
            .get(&wall_id)
            .copied()
            .unwrap_or(wall_id)
    }

    pub(super) fn clear_progressive_fragments_for_parent(
        &self,
        parent_wall_id: EntityId,
    ) -> Vec<EntityId> {
        if !self.progressive_destructible_enabled {
            return Vec::new();
        }

        let fragment_state = {
            let mut progressive_state = self.progressive_destructible_state.write();
            let Some(state) = progressive_state.fragmented_walls.remove(&parent_wall_id) else {
                return Vec::new();
            };
            for child_wall in &state.child_walls {
                progressive_state.child_to_parent.remove(&child_wall.id);
            }
            state
        };

        let partition_idx = self.world_partition_manager.get_partition_index_for_point(
            fragment_state.parent_wall.x + fragment_state.parent_wall.width / 2.0,
            fragment_state.parent_wall.y + fragment_state.parent_wall.height / 2.0,
        );
        if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
            for child_wall in &fragment_state.child_walls {
                let _ = partition.remove_wall(child_wall.id);
            }
        }

        fragment_state
            .child_walls
            .into_iter()
            .map(|wall| wall.id)
            .collect()
    }

    pub(super) fn build_progressive_fragment_walls(parent_wall: &Wall, stage: u8) -> Vec<Wall> {
        let is_horizontal = parent_wall.width >= parent_wall.height;
        let major_length = if is_horizontal {
            parent_wall.width
        } else {
            parent_wall.height
        };
        let minor_length = if is_horizontal {
            parent_wall.height
        } else {
            parent_wall.width
        };
        if major_length <= PROGRESSIVE_WALL_MIN_FRAGMENT_LENGTH * 2.0 {
            return Vec::new();
        }

        let segment_layout: Vec<(f32, f32)> = match stage {
            1 => {
                let gap = (major_length * 0.18)
                    .max(minor_length * 1.1)
                    .clamp(10.0, major_length * 0.45);
                let segment_len = (major_length - gap) * 0.5;
                if segment_len < PROGRESSIVE_WALL_MIN_FRAGMENT_LENGTH {
                    return Vec::new();
                }
                vec![(0.0, segment_len), (segment_len + gap, segment_len)]
            }
            2 => {
                let center_gap = (major_length * 0.36)
                    .max(minor_length * 1.5)
                    .clamp(16.0, major_length * 0.60);
                let side_span = (major_length - center_gap) * 0.5;
                let side_gap = (minor_length * 0.45).clamp(4.0, 10.0);
                let segment_len = (side_span - side_gap) * 0.5;
                if segment_len < PROGRESSIVE_WALL_MIN_FRAGMENT_LENGTH {
                    return Self::build_progressive_fragment_walls(parent_wall, 1);
                }
                vec![
                    (0.0, segment_len),
                    (segment_len + side_gap, segment_len),
                    (side_span + center_gap, segment_len),
                    (side_span + center_gap + segment_len + side_gap, segment_len),
                ]
            }
            _ => return Vec::new(),
        };

        segment_layout
            .into_iter()
            .map(|(offset, len)| {
                if is_horizontal {
                    Wall {
                        id: generate_entity_id(),
                        x: parent_wall.x + offset,
                        y: parent_wall.y,
                        width: len,
                        height: parent_wall.height,
                        is_destructible: true,
                        current_health: parent_wall.current_health,
                        max_health: parent_wall.max_health,
                    }
                } else {
                    Wall {
                        id: generate_entity_id(),
                        x: parent_wall.x,
                        y: parent_wall.y + offset,
                        width: parent_wall.width,
                        height: len,
                        is_destructible: true,
                        current_health: parent_wall.current_health,
                        max_health: parent_wall.max_health,
                    }
                }
            })
            .collect()
    }

    pub(super) fn apply_progressive_wall_fragmentation(
        &self,
        parent_wall: &Wall,
        target_stage: u8,
        destroyed_ids: &mut HashSet<EntityId>,
        updated_walls: &mut HashMap<EntityId, Wall>,
    ) -> bool {
        let partition_idx = self.world_partition_manager.get_partition_index_for_point(
            parent_wall.x + parent_wall.width / 2.0,
            parent_wall.y + parent_wall.height / 2.0,
        );
        let Some(partition) = self.world_partition_manager.get_partition(partition_idx) else {
            return false;
        };

        let same_stage_children = {
            let mut progressive_state = self.progressive_destructible_state.write();
            if let Some(fragment_state) =
                progressive_state.fragmented_walls.get_mut(&parent_wall.id)
            {
                if fragment_state.stage == target_stage {
                    fragment_state.parent_wall.current_health = parent_wall.current_health;
                    fragment_state.parent_wall.max_health = parent_wall.max_health;
                    for child_wall in &mut fragment_state.child_walls {
                        child_wall.current_health = parent_wall.current_health;
                        child_wall.max_health = parent_wall.max_health;
                    }
                    Some(fragment_state.child_walls.clone())
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(existing_children) = same_stage_children {
            for child_wall in existing_children {
                partition.upsert_wall(child_wall.clone());
                updated_walls.insert(child_wall.id, child_wall);
            }
            return false;
        }

        let previous_children = {
            let mut progressive_state = self.progressive_destructible_state.write();
            let previous = progressive_state
                .fragmented_walls
                .remove(&parent_wall.id)
                .map(|fragment| fragment.child_walls)
                .unwrap_or_default();
            for child_wall in &previous {
                progressive_state.child_to_parent.remove(&child_wall.id);
            }
            previous
        };

        for child_wall in previous_children {
            let _ = partition.remove_wall(child_wall.id);
            destroyed_ids.insert(child_wall.id);
        }

        if partition.remove_wall(parent_wall.id).is_some() {
            destroyed_ids.insert(parent_wall.id);
        }

        let fragment_walls = Self::build_progressive_fragment_walls(parent_wall, target_stage);
        if fragment_walls.is_empty() {
            partition.upsert_wall(parent_wall.clone());
            updated_walls.insert(parent_wall.id, parent_wall.clone());
            return true;
        }

        for child_wall in &fragment_walls {
            partition.upsert_wall(child_wall.clone());
            updated_walls.insert(child_wall.id, child_wall.clone());
        }

        {
            let mut progressive_state = self.progressive_destructible_state.write();
            for child_wall in &fragment_walls {
                progressive_state
                    .child_to_parent
                    .insert(child_wall.id, parent_wall.id);
            }
            progressive_state.fragmented_walls.insert(
                parent_wall.id,
                ProgressiveWallFragmentState {
                    stage: target_stage,
                    parent_wall: parent_wall.clone(),
                    child_walls: fragment_walls,
                },
            );
        }

        true
    }

    pub(super) fn apply_wall_damage_authoritative(&self, wall_hits: &[(EntityId, i32)]) -> usize {
        if wall_hits.is_empty() {
            return 0;
        }

        let mut wall_damage_by_parent: HashMap<EntityId, i32> = HashMap::new();
        for (wall_id, damage) in wall_hits {
            let parent_wall_id = self.resolve_progressive_parent_wall_id(*wall_id);
            *wall_damage_by_parent.entry(parent_wall_id).or_insert(0) += *damage;
        }

        let partitions_for_lookup = self.world_partition_manager.get_partitions_for_processing();
        let mut wall_partition_lookup: HashMap<EntityId, usize> = HashMap::new();
        for (partition_idx, partition) in partitions_for_lookup.iter().enumerate() {
            for wall_entry in partition.all_walls_in_partition.iter() {
                wall_partition_lookup.insert(*wall_entry.key(), partition_idx);
            }
        }

        let mut destroyed_count = 0usize;
        let mut topology_changed = false;
        let mut destroyed_ids_to_mark: HashSet<EntityId> = HashSet::new();
        let mut updated_walls_to_mark: HashMap<EntityId, Wall> = HashMap::new();

        for (parent_wall_id, total_damage) in wall_damage_by_parent {
            if total_damage <= 0 {
                continue;
            }

            let mut parent_wall_state = self
                .progressive_destructible_state
                .read()
                .fragmented_walls
                .get(&parent_wall_id)
                .map(|fragment| fragment.parent_wall.clone());

            if parent_wall_state.is_none() {
                if let Some(partition_idx) = wall_partition_lookup.get(&parent_wall_id).copied() {
                    if let Some(partition) = partitions_for_lookup.get(partition_idx) {
                        parent_wall_state = partition.get_wall(parent_wall_id);
                    }
                }
            }

            let Some(mut parent_wall) = parent_wall_state else {
                continue;
            };
            if !parent_wall.is_destructible || parent_wall.current_health <= 0 {
                continue;
            }

            let old_health = parent_wall.current_health;
            parent_wall.current_health = (parent_wall.current_health - total_damage).max(0);
            let center = Vec2::new(
                parent_wall.x + parent_wall.width / 2.0,
                parent_wall.y + parent_wall.height / 2.0,
            );

            if parent_wall.current_health <= 0 {
                destroyed_count += 1;

                let removed_children = self.clear_progressive_fragments_for_parent(parent_wall_id);
                if !removed_children.is_empty() {
                    topology_changed = true;
                    for child_id in removed_children {
                        destroyed_ids_to_mark.insert(child_id);
                    }
                }

                if let Some(partition_idx) = wall_partition_lookup.get(&parent_wall_id).copied() {
                    if let Some(partition) = partitions_for_lookup.get(partition_idx) {
                        if partition.get_wall(parent_wall_id).is_some() {
                            let _ = partition.damage_destructible_wall(
                                parent_wall_id,
                                old_health.max(total_damage),
                            );
                        }
                    }
                }

                destroyed_ids_to_mark.insert(parent_wall_id);
                self.global_game_events.push(
                    GameEvent::WallDestroyed {
                        wall_id: parent_wall_id,
                        position: center,
                    },
                    EventPriority::High,
                );
                self.wall_respawn_manager.wall_destroyed(parent_wall_id);
                continue;
            }

            if !self.progressive_destructible_enabled {
                if let Some(partition_idx) = wall_partition_lookup.get(&parent_wall_id).copied() {
                    if let Some(partition) = partitions_for_lookup.get(partition_idx) {
                        let _ = partition.damage_destructible_wall(parent_wall_id, total_damage);
                        if let Some(updated_wall) = partition.get_wall(parent_wall_id) {
                            updated_walls_to_mark.insert(updated_wall.id, updated_wall);
                        }
                    }
                }
                continue;
            }

            let health_ratio =
                parent_wall.current_health as f32 / parent_wall.max_health.max(1) as f32;
            let target_stage = if health_ratio <= PROGRESSIVE_WALL_STAGE2_HEALTH_RATIO {
                2
            } else if health_ratio <= PROGRESSIVE_WALL_STAGE1_HEALTH_RATIO {
                1
            } else {
                0
            };

            if target_stage == 0 {
                if let Some(partition_idx) = wall_partition_lookup.get(&parent_wall_id).copied() {
                    if let Some(partition) = partitions_for_lookup.get(partition_idx) {
                        if partition.get_wall(parent_wall_id).is_some() {
                            let _ =
                                partition.damage_destructible_wall(parent_wall_id, total_damage);
                            if let Some(updated_wall) = partition.get_wall(parent_wall_id) {
                                updated_walls_to_mark.insert(updated_wall.id, updated_wall);
                            }
                        } else {
                            let removed_children =
                                self.clear_progressive_fragments_for_parent(parent_wall_id);
                            if !removed_children.is_empty() {
                                topology_changed = true;
                                for child_id in removed_children {
                                    destroyed_ids_to_mark.insert(child_id);
                                }
                            }
                            partition.upsert_wall(parent_wall.clone());
                            updated_walls_to_mark.insert(parent_wall.id, parent_wall.clone());
                        }
                    }
                }
            } else if self.apply_progressive_wall_fragmentation(
                &parent_wall,
                target_stage,
                &mut destroyed_ids_to_mark,
                &mut updated_walls_to_mark,
            ) {
                topology_changed = true;
            }
        }

        if topology_changed {
            self.invalidate_structural_wall_cache();
        }
        if !destroyed_ids_to_mark.is_empty() {
            let mut destroyed_guard = self.destroyed_wall_ids_this_tick.write();
            for wall_id in destroyed_ids_to_mark {
                destroyed_guard.insert(wall_id);
            }
        }
        if !updated_walls_to_mark.is_empty() {
            self.updated_walls_this_tick
                .write()
                .extend(updated_walls_to_mark);
        }

        destroyed_count
    }
}
