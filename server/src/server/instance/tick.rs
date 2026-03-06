use super::*;
use std::sync::OnceLock;

fn stage2_pool_offload_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("MGS_STAGE2_POOL_OFFLOAD")
            .ok()
            .map(|raw| {
                let normalized = raw.trim().to_ascii_lowercase();
                normalized == "1"
                    || normalized == "true"
                    || normalized == "yes"
                    || normalized == "on"
            })
            .unwrap_or(true)
    })
}

impl MassiveGameServer {
    pub async fn process_game_tick(self: Arc<Self>, dt: f32) -> Result<(), ServerError> {
        let tick_started = Instant::now();
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let has_connected_clients =
            !self.data_channels_map.is_empty() || connected_quic_peer_count() > 0;

        // Stage 1: Input & AI (Potentially parallelizable)
        let stage1_start = Instant::now();
        let mut set = JoinSet::new();

        set.spawn({
            let server_clone = Arc::clone(&self);
            async move {
                let task_name = "network_input";
                trace!("[Frame {}] Starting task: {}", frame, task_name);
                let result = timeout(Duration::from_millis(NET_IO_TIMEOUT_MS), async {
                    server_clone.process_network_input().await;
                })
                .await;
                if result.is_err() && frame.is_multiple_of(60) {
                    warn!(
                        "[Frame {}] Task '{}' timed out after {}ms",
                        frame, task_name, NET_IO_TIMEOUT_MS
                    );
                }
                trace!("[Frame {}] Finished task: {}", frame, task_name);
            }
        });

        let ai_stride = if has_connected_clients {
            AI_UPDATE_STRIDE
        } else {
            AI_UPDATE_STRIDE * 3
        };
        if frame.is_multiple_of(ai_stride) {
            set.spawn({
                let server_clone = Arc::clone(&self);
                async move {
                    let task_name = "ai_update";
                    trace!("[Frame {}] Starting task: {}", frame, task_name);
                    let result = timeout(Duration::from_millis(AI_TIMEOUT_MS), async {
                        server_clone.run_ai_update().await;
                    })
                    .await;
                    if result.is_err() && frame.is_multiple_of(60) {
                        warn!(
                            "[Frame {}] Task '{}' timed out after {}ms",
                            frame, task_name, AI_TIMEOUT_MS
                        );
                    }
                    trace!("[Frame {}] Finished task: {}", frame, task_name);
                }
            });
        }

        while let Some(res) = set.join_next().await {
            if let Err(e) = res {
                if e.is_panic() {
                    // A task panicked -- flag the match as degraded so health
                    // checks and monitoring can detect the corrupted state.
                    error!(
                        "[Frame {}] CRITICAL: Task panicked in Stage 1: {}. Match flagged as degraded.",
                        frame, e
                    );
                    self.runtime_tracking
                        .match_degraded
                        .store(true, AtomicOrdering::Release);
                } else {
                    // Task was cancelled (not a panic) -- less severe but still log.
                    error!("[Frame {}] Task join error in Stage 1: {}", frame, e);
                }
            }
        }
        let stage1_elapsed = stage1_start.elapsed();
        trace!(
            "[Frame {}] Stage 1 (Input/AI) took: {:?}",
            frame,
            stage1_elapsed
        );

        // Stage 2: Physics & Game Logic (Sequential, mutation-heavy)
        let stage2_start = Instant::now();
        self.maybe_refresh_navigation_mesh();
        let (physics_elapsed, game_logic_elapsed) = if stage2_pool_offload_enabled() {
            // Keep Stage 2 off async workers by running it in dedicated compute pools.
            let physics_server = Arc::clone(&self);
            let physics_pool = Arc::clone(&self.thread_pools.physics_pool);
            let runtime_handle = tokio::runtime::Handle::current();
            let physics_elapsed = match tokio::task::spawn_blocking(move || {
                let phase_start = Instant::now();
                physics_pool
                    .install(|| runtime_handle.block_on(physics_server.run_physics_update(dt)));
                phase_start.elapsed()
            })
            .await
            {
                Ok(elapsed) => elapsed,
                Err(join_err) => {
                    self.runtime_tracking
                        .match_degraded
                        .store(true, AtomicOrdering::Release);
                    return Err(ServerError::ThreadingError(format!(
                        "Stage 2 physics offload join failed: {}",
                        join_err
                    )));
                }
            };
            trace!(
                "[Frame {}] Physics update took: {:?} (pool_offload=true)",
                frame,
                physics_elapsed
            );

            let game_logic_server = Arc::clone(&self);
            let game_logic_pool = Arc::clone(&self.thread_pools.game_logic_pool);
            let runtime_handle = tokio::runtime::Handle::current();
            let game_logic_elapsed = match tokio::task::spawn_blocking(move || {
                let phase_start = Instant::now();
                game_logic_pool.install(|| {
                    runtime_handle.block_on(game_logic_server.run_game_logic_update(dt))
                });
                phase_start.elapsed()
            })
            .await
            {
                Ok(elapsed) => elapsed,
                Err(join_err) => {
                    self.runtime_tracking
                        .match_degraded
                        .store(true, AtomicOrdering::Release);
                    return Err(ServerError::ThreadingError(format!(
                        "Stage 2 game-logic offload join failed: {}",
                        join_err
                    )));
                }
            };
            trace!(
                "[Frame {}] Game logic update took: {:?} (pool_offload=true)",
                frame,
                game_logic_elapsed
            );
            (physics_elapsed, game_logic_elapsed)
        } else {
            let physics_start = Instant::now();
            self.run_physics_update(dt).await;
            let physics_elapsed = physics_start.elapsed();
            trace!(
                "[Frame {}] Physics update took: {:?}",
                frame,
                physics_elapsed
            );

            let game_logic_start = Instant::now();
            self.run_game_logic_update(dt).await;
            let game_logic_elapsed = game_logic_start.elapsed();
            trace!(
                "[Frame {}] Game logic update took: {:?}",
                frame,
                game_logic_elapsed
            );
            (physics_elapsed, game_logic_elapsed)
        };

        let stage2_elapsed = stage2_start.elapsed();
        if stage2_elapsed > Duration::from_millis(SLOW_TICK_LOG_MS) && frame.is_multiple_of(60) {
            warn!(
                ?frame,
                ms = stage2_elapsed.as_micros() as f64 / 1000.0,
                physics_ms = physics_elapsed.as_micros() as f64 / 1000.0,
                game_logic_ms = game_logic_elapsed.as_micros() as f64 / 1000.0,
                "Stage 2 (Physics/Logic) exceeded soft budget {}ms",
                SLOW_TICK_LOG_MS
            );
        }

        // Stage 3: State Sync & Broadcast
        let stage3_start = Instant::now();

        let sync_start = Instant::now();
        self.synchronize_state(has_connected_clients).await;
        // AoI is refreshed during synchronize_state, so publish its snapshot afterwards to keep
        // broadcast reads on the latest authoritative frame.
        self.publish_player_aoi_snapshot_if_enabled();
        let sync_elapsed = sync_start.elapsed();
        trace!(
            "[Frame {}] State synchronization took: {:?}",
            frame,
            sync_elapsed
        );

        let broadcast_start_time = Instant::now();
        let broadcast_elapsed_duration;
        let broadcast_timed_out_flag;
        if has_connected_clients {
            let server_for_broadcast_call = Arc::clone(&self);
            let broadcast_future = server_for_broadcast_call.broadcast_world_updates_optimized();

            let timed_broadcast_future =
                tokio::time::timeout(Duration::from_millis(FAN_OUT_TIMEOUT_MS), broadcast_future);

            let b_start_inner = Instant::now();
            broadcast_timed_out_flag = timed_broadcast_future.await.is_err();
            broadcast_elapsed_duration = b_start_inner.elapsed();
        } else {
            broadcast_elapsed_duration = broadcast_start_time.elapsed();
            broadcast_timed_out_flag = false;
        }

        trace!(
            "[Frame {}] Broadcast took: {:?} (timed_out: {})",
            frame,
            broadcast_elapsed_duration,
            broadcast_timed_out_flag
        );

        if broadcast_timed_out_flag && frame.is_multiple_of(60) {
            error!(
                "[Frame {}] Broadcast stage timed out after {}ms (actual: {:?})",
                frame, FAN_OUT_TIMEOUT_MS, broadcast_elapsed_duration
            );
        }
        let _stage3_elapsed = stage3_start.elapsed();
        self.capture_live_replay_frame(frame);

        // Stage 4: Cleanup
        self.destroyed_wall_ids_this_tick.write().clear();
        self.updated_walls_this_tick.write().clear();
        trace!("[Frame {}] Tick-local cleanup complete.", frame);

        let total_tick_processing_elapsed = tick_started.elapsed();

        if total_tick_processing_elapsed > Duration::from_millis(TARGET_TICK_MS + 4)
            && frame.is_multiple_of(10)
        {
            warn!(
                "Frame {} timing breakdown:\n\
                     Total: {:.2}ms\n\
                     - Input/AI (Stage 1): {:.2}ms\n\
                     - Physics (Stage 2a): {:.2}ms\n\
                     - Game Logic (Stage 2b): {:.2}ms\n\
                     - State Sync (Stage 3a): {:.2}ms\n\
                     - Broadcast (Stage 3b): {:.2}ms (timed_out: {})\n\
                     (Target Tick: {}ms)",
                frame,
                total_tick_processing_elapsed.as_secs_f32() * 1000.0,
                stage1_elapsed.as_secs_f32() * 1000.0,
                physics_elapsed.as_secs_f32() * 1000.0,
                game_logic_elapsed.as_secs_f32() * 1000.0,
                sync_elapsed.as_secs_f32() * 1000.0,
                broadcast_elapsed_duration.as_secs_f32() * 1000.0,
                broadcast_timed_out_flag,
                TARGET_TICK_MS
            );
        }

        if total_tick_processing_elapsed > Duration::from_millis(TARGET_TICK_MS)
            && frame.is_multiple_of(60)
        {
            warn!(
                ?frame,
                ms = total_tick_processing_elapsed.as_micros() as f64 / 1000.0,
                target = TARGET_TICK_MS,
                "Tick processing WORK exceeded hard budget (game_loop will log wall-clock overrun)"
            );
        }

        Ok(())
    }
}
