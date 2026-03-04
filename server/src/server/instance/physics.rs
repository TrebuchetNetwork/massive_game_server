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
        let updated_walls_count = self.updated_walls_this_tick.read().len();
        // Keep collision data fresher under heavy destruction/rebuild churn.
        let needs_periodic_rebuild = self.wall_spatial_index.needs_rebuild(frame, 30);

        if needs_periodic_rebuild {
            let index_rebuild_start = Instant::now();
            let active_walls = self.collect_active_walls_optimized();
            self.wall_spatial_index.rebuild(&active_walls, frame);
            debug!(
                "[Frame {}] Wall spatial index rebuilt in {:?} (respawned: {}, destroyed: {}, updated: {})",
                frame,
                index_rebuild_start.elapsed(),
                respawned_walls.len(),
                destroyed_walls_count,
                updated_walls_count
            );
        } else if !respawned_walls.is_empty()
            || destroyed_walls_count > 0
            || updated_walls_count > 0
        {
            let index_update_start = Instant::now();
            let destroyed_ids: Vec<_> = self
                .destroyed_wall_ids_this_tick
                .read()
                .iter()
                .copied()
                .collect();
            let updated_walls: Vec<_> = self
                .updated_walls_this_tick
                .read()
                .values()
                .cloned()
                .collect();

            let mut removed_ids = destroyed_ids;
            removed_ids.extend(updated_walls.iter().map(|w| w.id));

            self.wall_spatial_index
                .update_walls(&removed_ids, &updated_walls, frame);
            debug!(
                "[Frame {}] Wall spatial index updated in {:?} (respawned: {}, destroyed: {}, updated: {})",
                frame,
                index_update_start.elapsed(),
                respawned_walls.len(),
                destroyed_walls_count,
                updated_walls_count
            );
        }

        // Stage 2: Collect Active Walls
        let collect_walls_start = Instant::now();
        let active_walls = self.get_active_walls_cached(frame).await;
        debug!(
            "Frame {}: Collected {} active walls (took {:?})",
            frame,
            active_walls.len(),
            collect_walls_start.elapsed()
        );

        // Stage 3: Process Player Physics
        let player_physics_start = Instant::now();
        let player_updates = self.process_player_physics_parallel(delta_time).await;
        debug!(
            "Frame {}: Processed {} player physics updates (took {:?})",
            frame,
            player_updates.players_to_respawn.len() + player_updates.alive_count,
            player_physics_start.elapsed()
        );

        // Stage 4: Apply Player Updates
        let apply_updates_start = Instant::now();
        self.apply_player_updates(player_updates, &active_walls)
            .await;
        debug!(
            "Frame {}: Applied player updates (took {:?})",
            frame,
            apply_updates_start.elapsed()
        );

        // Stage 5: Process Projectiles
        let projectiles_start = Instant::now();
        let projectile_results = self
            .process_projectiles_optimized(&active_walls, delta_time)
            .await;
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
        self.apply_projectile_results(projectile_results).await;
        debug!(
            "Frame {}: Applied projectile results (took {:?})",
            frame,
            apply_projectiles_start.elapsed()
        );

        // Stage 7: Process Pickups
        let pickups_start = Instant::now();
        self.process_pickup_respawns(delta_time).await;
        debug!(
            "Frame {}: Processed pickups (took {:?})",
            frame,
            pickups_start.elapsed()
        );

        debug!(
            "Frame {}: TOTAL physics update took {:?}",
            frame,
            physics_start_time.elapsed()
        );
        metrics::record_subsystem_time("physics", physics_start_time.elapsed().as_secs_f64());
    }
}
