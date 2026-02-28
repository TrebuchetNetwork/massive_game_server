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

        let mut fragment_walls: Vec<Wall> = segment_layout
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
                        current_health: 0,
                        max_health: 0,
                    }
                } else {
                    Wall {
                        id: generate_entity_id(),
                        x: parent_wall.x,
                        y: parent_wall.y + offset,
                        width: parent_wall.width,
                        height: len,
                        is_destructible: true,
                        current_health: 0,
                        max_health: 0,
                    }
                }
            })
            .collect();
        Self::distribute_fragment_health(parent_wall, &mut fragment_walls);
        fragment_walls
    }

    fn allocate_health_by_weights(total: i32, weights: &[f32]) -> Vec<i32> {
        if weights.is_empty() {
            return Vec::new();
        }
        if total <= 0 {
            return vec![0; weights.len()];
        }

        let sanitized_weights: Vec<f64> = weights.iter().map(|w| w.max(0.0) as f64).collect();
        let weight_sum: f64 = sanitized_weights.iter().sum();

        if weight_sum <= f64::EPSILON {
            let mut allocations = vec![total / weights.len() as i32; weights.len()];
            let mut remainder = total - allocations.iter().sum::<i32>();
            for value in allocations.iter_mut() {
                if remainder <= 0 {
                    break;
                }
                *value += 1;
                remainder -= 1;
            }
            return allocations;
        }

        let total_f = total as f64;
        let mut allocations = vec![0; weights.len()];
        let mut allocated = 0i32;
        let mut remainders = Vec::with_capacity(weights.len());

        for (idx, weight) in sanitized_weights.iter().enumerate() {
            let exact = total_f * (*weight / weight_sum);
            let floor = exact.floor() as i32;
            allocations[idx] = floor;
            allocated += floor;
            remainders.push((idx, exact - floor as f64));
        }

        let mut remaining = total - allocated;
        remainders.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        for (idx, _) in remainders {
            if remaining <= 0 {
                break;
            }
            allocations[idx] += 1;
            remaining -= 1;
        }

        allocations
    }

    fn allocate_with_caps(total: i32, caps: &[i32]) -> Vec<i32> {
        if caps.is_empty() {
            return Vec::new();
        }
        let sanitized_caps: Vec<i32> = caps.iter().map(|cap| (*cap).max(0)).collect();
        let cap_sum: i32 = sanitized_caps.iter().sum();
        if total <= 0 || cap_sum <= 0 {
            return vec![0; caps.len()];
        }

        let capped_total = total.min(cap_sum);
        let cap_sum_f = cap_sum as f64;
        let mut allocations = vec![0; caps.len()];
        let mut allocated = 0i32;
        let mut remainders = Vec::with_capacity(caps.len());

        for (idx, cap) in sanitized_caps.iter().enumerate() {
            if *cap <= 0 {
                remainders.push((idx, 0.0));
                continue;
            }
            let exact = capped_total as f64 * (*cap as f64 / cap_sum_f);
            let floor = (exact.floor() as i32).min(*cap);
            allocations[idx] = floor;
            allocated += floor;
            remainders.push((idx, exact - floor as f64));
        }

        remainders.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });

        let mut remaining = capped_total - allocated;
        while remaining > 0 {
            let mut progressed = false;
            for (idx, _) in remainders.iter().copied() {
                if remaining == 0 {
                    break;
                }
                if allocations[idx] < sanitized_caps[idx] {
                    allocations[idx] += 1;
                    remaining -= 1;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }

        allocations
    }

    fn distribute_fragment_health(parent_wall: &Wall, child_walls: &mut [Wall]) {
        if child_walls.is_empty() {
            return;
        }

        let parent_area = (parent_wall.width.max(0.0) * parent_wall.height.max(0.0)).max(1.0);
        let weights: Vec<f32> = child_walls
            .iter()
            .map(|wall| {
                let wall_area = (wall.width.max(0.0) * wall.height.max(0.0)).max(0.0);
                wall_area / parent_area
            })
            .collect();

        let max_health_allocations =
            Self::allocate_health_by_weights(parent_wall.max_health.max(0), &weights);
        let current_health_allocations =
            Self::allocate_with_caps(parent_wall.current_health.max(0), &max_health_allocations);

        for (idx, child_wall) in child_walls.iter_mut().enumerate() {
            child_wall.max_health = *max_health_allocations.get(idx).unwrap_or(&0);
            child_wall.current_health = *current_health_allocations.get(idx).unwrap_or(&0);
        }
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
                    Self::distribute_fragment_health(parent_wall, &mut fragment_state.child_walls);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parent_wall_with_health(current_health: i32, max_health: i32) -> Wall {
        Wall {
            id: 1,
            x: 100.0,
            y: 100.0,
            width: 160.0,
            height: 40.0,
            is_destructible: true,
            current_health,
            max_health,
        }
    }

    #[test]
    fn progressive_fragment_health_preserves_parent_totals() {
        let parent = parent_wall_with_health(65, 100);
        let children = MassiveGameServer::build_progressive_fragment_walls(&parent, 1);
        assert!(
            !children.is_empty(),
            "stage 1 should produce child fragments"
        );

        let child_current_total: i32 = children.iter().map(|child| child.current_health).sum();
        let child_max_total: i32 = children.iter().map(|child| child.max_health).sum();

        assert_eq!(
            child_current_total, parent.current_health,
            "child current health should preserve parent current health"
        );
        assert_eq!(
            child_max_total, parent.max_health,
            "child max health should preserve parent max health"
        );
        assert!(
            children
                .iter()
                .all(|child| child.current_health >= 0 && child.current_health <= child.max_health),
            "each child should remain within [0, max_health]"
        );
    }

    #[test]
    fn allocate_with_caps_never_exceeds_capacity() {
        let caps = [10, 20, 30];
        let allocations = MassiveGameServer::allocate_with_caps(42, &caps);
        assert_eq!(allocations.iter().sum::<i32>(), 42);
        assert!(
            allocations
                .iter()
                .zip(caps.iter())
                .all(|(allocation, cap)| allocation <= cap),
            "allocations should not exceed caps"
        );

        let saturated = MassiveGameServer::allocate_with_caps(999, &caps);
        assert_eq!(saturated, vec![10, 20, 30]);
    }
}
