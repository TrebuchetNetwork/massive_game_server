use super::*;

impl MassiveGameServer {
    pub fn maybe_refresh_navigation_mesh(&self) {
        if !self.navmesh_enabled {
            return;
        }

        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let should_rebuild = {
            if self.navmesh.load().is_none() {
                true
            } else {
                let last = self
                    .navmesh_last_rebuild_frame
                    .load(AtomicOrdering::Relaxed);
                frame.saturating_sub(last) >= self.navmesh_rebuild_interval_frames
            }
        };

        if should_rebuild {
            self.rebuild_navigation_mesh(frame);
        }
    }

    fn rebuild_navigation_mesh(&self, frame: u64) {
        let partitions = self.world_partition_manager.get_partitions_for_processing();
        let mut polygons = Vec::with_capacity(partitions.len());
        let inset = 8.0f32;

        for partition in partitions {
            let active_wall_count = partition
                .all_walls_in_partition
                .iter()
                .filter(|entry| {
                    let wall = entry.value();
                    !(wall.is_destructible && wall.current_health <= 0)
                })
                .count();
            if active_wall_count > self.navmesh_cell_wall_limit {
                continue;
            }

            let bounds = partition.bounds;
            if (bounds.max_x - bounds.min_x) <= inset * 2.0
                || (bounds.max_y - bounds.min_y) <= inset * 2.0
            {
                continue;
            }

            polygons.push(vec![
                Vec2::new(bounds.min_x + inset, bounds.min_y + inset),
                Vec2::new(bounds.max_x - inset, bounds.min_y + inset),
                Vec2::new(bounds.max_x - inset, bounds.max_y - inset),
                Vec2::new(bounds.min_x + inset, bounds.max_y - inset),
            ]);
        }

        let navmesh = if polygons.is_empty() {
            None
        } else {
            Some(NavMesh::from_convex_polygons(polygons))
        };
        let polygon_count = navmesh.as_ref().map_or(0, NavMesh::polygon_count);
        self.navmesh.store(navmesh.map(Arc::new));
        self.navmesh_last_rebuild_frame
            .store(frame, AtomicOrdering::Relaxed);

        trace!(
            "[Frame {}] NavMesh rebuilt (enabled={}, polygons={}, cell_wall_limit={})",
            frame,
            self.navmesh_enabled,
            polygon_count,
            self.navmesh_cell_wall_limit
        );
    }

    pub fn navigation_waypoint_towards(&self, start: Vec2, goal: Vec2) -> Vec2 {
        if !self.navmesh_enabled {
            return goal;
        }

        let navmesh_guard = self.navmesh.load();
        let Some(navmesh) = navmesh_guard.as_deref() else {
            return goal;
        };

        if let Some(path) = navmesh.find_path(start, goal) {
            if let Some(next) = path.get(1).copied() {
                return next;
            }
        }

        goal
    }
}
