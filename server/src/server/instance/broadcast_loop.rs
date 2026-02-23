use super::*;

impl MassiveGameServer {
    pub async fn broadcast_world_updates_optimized(self: Arc<Self>) {
        const BROADCAST_INTERVAL_FRAMES: u64 = 1;
        const MIN_BROADCAST_CONCURRENCY: usize = 8;
        const MAX_BROADCAST_CONCURRENCY: usize = 64;
        const MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN: usize = 8;
        const MASS_JOIN_MEDIUM_PENDING_INITIAL_MIN: usize = 24;
        const MASS_JOIN_HEAVY_PENDING_INITIAL_MIN: usize = 48;
        const MASS_JOIN_DELTA_SKIP_MODULUS: u64 = 2;
        const MASS_JOIN_INITIAL_PER_FRAME_LIGHT: usize = 24;
        const MASS_JOIN_INITIAL_PER_FRAME_MEDIUM: usize = 20;
        const MASS_JOIN_INITIAL_PER_FRAME_HEAVY: usize = 16;
        const MASS_JOIN_MAX_DELTA_PER_FRAME_MEDIUM: usize = 20;
        const MASS_JOIN_MAX_DELTA_PER_FRAME_HEAVY: usize = 10;
        const MASS_JOIN_CONCURRENCY_CAP: usize = 48;
        const TAIL_JOIN_CONNECTED_CLIENTS_MIN: usize = 70;
        const TAIL_JOIN_PENDING_INITIAL_OPEN_MIN: usize = 3;
        const TAIL_JOIN_INITIAL_PER_FRAME_BOOST: usize = 32;
        const TAIL_JOIN_MAX_DELTA_PER_FRAME: usize = 4;
        const TAIL_JOIN_DELTA_SKIP_MODULUS: u64 = 4;
        const TAIL_JOIN_CONCURRENCY_CAP: usize = 36;
        const TAIL_JOIN_AGGRESSIVE_CONNECTED_CLIENTS_MIN: usize = 70;
        const TAIL_JOIN_AGGRESSIVE_PENDING_INITIAL_OPEN_MIN: usize = 6;
        const TAIL_JOIN_AGGRESSIVE_INITIAL_PER_FRAME_BOOST: usize = 56;
        const TAIL_JOIN_AGGRESSIVE_MAX_DELTA_PER_FRAME: usize = 1;
        const TAIL_JOIN_AGGRESSIVE_DELTA_SKIP_MODULUS: u64 = 7;
        const TAIL_JOIN_AGGRESSIVE_CONCURRENCY_CAP: usize = 28;
        // Dedicated policy for the 70+ tail wave where join timeout risk is highest.
        const TAIL_WAVE_70_PLUS_CLIENTS_MIN: usize = 70;
        const TAIL_WAVE_70_PLUS_PENDING_INITIAL_OPEN_MIN: usize = 2;
        const TAIL_WAVE_70_PLUS_INITIAL_PER_FRAME_BOOST: usize = 72;
        const TAIL_WAVE_70_PLUS_MAX_DELTA_PER_FRAME: usize = 1;
        const TAIL_WAVE_70_PLUS_DELTA_SKIP_MODULUS: u64 = 9;
        const TAIL_WAVE_70_PLUS_CONCURRENCY_CAP: usize = 24;
        const EXTREME_TAIL_WAVE_CLIENTS_MIN: usize = 90;
        const EXTREME_TAIL_WAVE_PENDING_INITIAL_OPEN_MIN: usize = 6;
        const EXTREME_TAIL_WAVE_INITIAL_PER_FRAME_BOOST: usize = 96;
        const EXTREME_TAIL_WAVE_MAX_DELTA_PER_FRAME: usize = 0;
        const EXTREME_TAIL_WAVE_DELTA_SKIP_MODULUS: u64 = 15;
        const EXTREME_TAIL_WAVE_CONCURRENCY_CAP: usize = 18;
        const SINGLE_MACHINE_TAIL_CONNECTED_CLIENTS_MIN: usize = 56;
        const SINGLE_MACHINE_TAIL_PENDING_INITIAL_OPEN_MIN: usize = 2;
        const SINGLE_MACHINE_AGGRESSIVE_CONNECTED_CLIENTS_MIN: usize = 64;
        const SINGLE_MACHINE_AGGRESSIVE_PENDING_INITIAL_OPEN_MIN: usize = 4;
        const SINGLE_MACHINE_INITIAL_PER_FRAME_BOOST: usize = 36;
        const SINGLE_MACHINE_MAX_DELTA_PER_FRAME: usize = 4;
        const SINGLE_MACHINE_DELTA_SKIP_MODULUS: u64 = 4;
        const SINGLE_MACHINE_CONCURRENCY_CAP: usize = 20;
        const MAX_DELTA_EVENTS_DEFAULT: usize = 50;
        const MAX_DELTA_EVENTS_TAIL: usize = 12;
        const MAX_DELTA_EVENTS_AGGRESSIVE: usize = 6;
        const MAX_DELTA_EVENTS_EXTREME_TAIL: usize = 2;
        const MAX_DELTA_EVENTS_SINGLE_MACHINE_BACKLOG: usize = 12;

        let current_frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let last_broadcast = self.last_broadcast_frame.load(AtomicOrdering::Relaxed);
        let single_machine_opt = single_machine_optimization_enabled();

        if current_frame < last_broadcast + BROADCAST_INTERVAL_FRAMES && current_frame != 0 {
            trace!(
                "[Frame {}] Skipping broadcast (interval). Last broadcast: {}",
                current_frame,
                last_broadcast
            );
            return;
        }

        let quic_peer_ids = connected_quic_peer_ids();
        if !quic_peer_ids.is_empty() {
            let mut client_states_guard = self.client_states_map.write();
            for peer_id in &quic_peer_ids {
                client_states_guard
                    .entry(peer_id.clone())
                    .or_insert_with(ClientState::default);
            }
        }

        let connected_clients_total = self
            .data_channels_map
            .len()
            .saturating_add(quic_peer_ids.len());
        if connected_clients_total == 0 {
            if current_frame % 30 == 0 {
                // Log every 30 frames
                // Debug: List all keys in the map to see if there's a mismatch
                info!(
                    "[Frame {}] No connected clients in WebRTC/QUIC maps. Checking map contents...",
                    current_frame
                );
                info!(
                    "[Frame {}] Map ptr in broadcast: {:p}",
                    current_frame,
                    Arc::as_ptr(&self.data_channels_map)
                );
                for entry in self.data_channels_map.iter() {
                    info!(
                        "[Frame {}] Found entry in map: key={}",
                        current_frame,
                        entry.key()
                    );
                }
                info!(
                    "[Frame {}] Total entries found: {} (quic={})",
                    current_frame,
                    self.data_channels_map.len(),
                    quic_peer_ids.len()
                );
            }
            return;
        }

        debug!(
            "[Frame {}] Starting broadcast to {} clients. Last broadcast frame: {}",
            current_frame, connected_clients_total, last_broadcast
        );
        self.last_broadcast_frame
            .store(current_frame, AtomicOrdering::Relaxed);

        let client_entries: Vec<_> = {
            let client_states_guard = self.client_states_map.read();
            self.data_channels_map
                .iter()
                .map(|entry| {
                    let peer_id = entry.key().clone();
                    let data_channel = Arc::clone(entry.value());
                    let needs_initial = !client_states_guard
                        .get(&peer_id)
                        .map_or(false, |cs_state| cs_state.known_walls_sent);
                    let channel_open = data_channel.is_open();
                    (peer_id, data_channel, needs_initial, channel_open)
                })
                .collect()
        };
        let connected_clients_open = client_entries
            .iter()
            .filter(|(_, _, _, channel_open)| *channel_open)
            .count();
        if connected_clients_open == 0 && quic_peer_ids.is_empty() {
            trace!(
                "[Frame {}] Skipping broadcast fanout because no data channels are open (tracked={}).",
                current_frame,
                connected_clients_total
            );
            return;
        }

        let mut initial_entries_open: Vec<(String, Arc<crate::core::types::RTCDataChannel>, bool)> =
            Vec::new();
        let mut delta_entries: Vec<(String, Arc<crate::core::types::RTCDataChannel>, bool)> =
            Vec::new();
        let mut pending_initial_closed_count = 0usize;
        let mut pending_delta_closed_count = 0usize;

        for (peer_id, data_channel, needs_initial, channel_open) in client_entries {
            if needs_initial {
                self.ensure_join_trace(&peer_id, channel_open);
                if channel_open {
                    initial_entries_open.push((peer_id, data_channel, true));
                } else {
                    pending_initial_closed_count += 1;
                }
            } else if channel_open {
                delta_entries.push((peer_id, data_channel, false));
            } else {
                pending_delta_closed_count += 1;
            }
        }

        let quic_entries: Vec<(String, bool)> = {
            let client_states_guard = self.client_states_map.read();
            quic_peer_ids
                .iter()
                .filter(|peer_id| !self.data_channels_map.contains_key(peer_id.as_str()))
                .map(|peer_id| {
                    let needs_initial = !client_states_guard
                        .get(peer_id.as_str())
                        .map_or(false, |cs_state| cs_state.known_walls_sent);
                    (peer_id.clone(), needs_initial)
                })
                .collect()
        };

        let pending_initial_open_count = initial_entries_open.len();
        let pending_initial_total_count = pending_initial_open_count + pending_initial_closed_count;
        let tail_connected_clients_min = if single_machine_opt {
            SINGLE_MACHINE_TAIL_CONNECTED_CLIENTS_MIN
        } else {
            TAIL_JOIN_CONNECTED_CLIENTS_MIN
        };
        let tail_pending_initial_open_min = if single_machine_opt {
            SINGLE_MACHINE_TAIL_PENDING_INITIAL_OPEN_MIN
        } else {
            TAIL_JOIN_PENDING_INITIAL_OPEN_MIN
        };
        let aggressive_connected_clients_min = if single_machine_opt {
            SINGLE_MACHINE_AGGRESSIVE_CONNECTED_CLIENTS_MIN
        } else {
            TAIL_JOIN_AGGRESSIVE_CONNECTED_CLIENTS_MIN
        };
        let aggressive_pending_initial_open_min = if single_machine_opt {
            SINGLE_MACHINE_AGGRESSIVE_PENDING_INITIAL_OPEN_MIN
        } else {
            TAIL_JOIN_AGGRESSIVE_PENDING_INITIAL_OPEN_MIN
        };

        let tail_policy_enabled = join_tail_policy_enabled();
        let tail_join_mode = tail_policy_enabled
            && connected_clients_total >= tail_connected_clients_min
            && pending_initial_open_count >= tail_pending_initial_open_min;
        let aggressive_tail_join_mode = tail_policy_enabled
            && connected_clients_total >= aggressive_connected_clients_min
            && pending_initial_open_count >= aggressive_pending_initial_open_min;
        let tail_wave_70_plus_mode = tail_policy_enabled
            && connected_clients_total >= TAIL_WAVE_70_PLUS_CLIENTS_MIN
            && pending_initial_open_count >= TAIL_WAVE_70_PLUS_PENDING_INITIAL_OPEN_MIN;
        let extreme_tail_join_mode = tail_policy_enabled
            && connected_clients_total >= EXTREME_TAIL_WAVE_CLIENTS_MIN
            && pending_initial_open_count >= EXTREME_TAIL_WAVE_PENDING_INITIAL_OPEN_MIN;
        let initial_snapshot_caps = if extreme_tail_join_mode {
            InitialSnapshotCaps::EXTREME_TAIL
        } else if aggressive_tail_join_mode {
            InitialSnapshotCaps::TAIL_AGGRESSIVE
        } else if tail_join_mode {
            InitialSnapshotCaps::TAIL
        } else if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            InitialSnapshotCaps::SINGLE_MACHINE_BACKLOG
        } else {
            InitialSnapshotCaps::DEFAULT
        };

        let mut max_delta_events_per_client = if extreme_tail_join_mode {
            MAX_DELTA_EVENTS_EXTREME_TAIL
        } else if aggressive_tail_join_mode {
            MAX_DELTA_EVENTS_AGGRESSIVE
        } else if tail_join_mode {
            MAX_DELTA_EVENTS_TAIL
        } else if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            MAX_DELTA_EVENTS_SINGLE_MACHINE_BACKLOG
        } else {
            MAX_DELTA_EVENTS_DEFAULT
        };
        let soa_adaptive_fallback_active = join_soa_adaptive_fallback_enabled()
            && connected_clients_total >= MASS_JOIN_MEDIUM_PENDING_INITIAL_MIN
            && (pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
                || aggressive_tail_join_mode
                || tail_join_mode);

        // Keep budget decisions tied to total backlog, but only schedule actionable
        // initial sends (open data channels).
        let mut max_initial_per_frame =
            if pending_initial_total_count >= MASS_JOIN_HEAVY_PENDING_INITIAL_MIN {
                MASS_JOIN_INITIAL_PER_FRAME_HEAVY
            } else if pending_initial_total_count >= MASS_JOIN_MEDIUM_PENDING_INITIAL_MIN {
                MASS_JOIN_INITIAL_PER_FRAME_MEDIUM
            } else {
                MASS_JOIN_INITIAL_PER_FRAME_LIGHT
            };
        if tail_join_mode {
            // 70+ client wave: allocate more slots to initial delivery to drain backlog sooner.
            max_initial_per_frame = max_initial_per_frame.max(TAIL_JOIN_INITIAL_PER_FRAME_BOOST);
        }
        if aggressive_tail_join_mode {
            max_initial_per_frame =
                max_initial_per_frame.max(TAIL_JOIN_AGGRESSIVE_INITIAL_PER_FRAME_BOOST);
        }
        if tail_wave_70_plus_mode {
            max_initial_per_frame =
                max_initial_per_frame.max(TAIL_WAVE_70_PLUS_INITIAL_PER_FRAME_BOOST);
        }
        if extreme_tail_join_mode {
            max_initial_per_frame =
                max_initial_per_frame.max(EXTREME_TAIL_WAVE_INITIAL_PER_FRAME_BOOST);
        }
        if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            max_initial_per_frame =
                max_initial_per_frame.max(SINGLE_MACHINE_INITIAL_PER_FRAME_BOOST);
        }

        let scheduled_initial_entries =
            if pending_initial_open_count > max_initial_per_frame && max_initial_per_frame > 0 {
                let start_index = (current_frame as usize) % pending_initial_open_count;
                let mut selected = Vec::with_capacity(max_initial_per_frame);
                for offset in 0..max_initial_per_frame {
                    let idx = (start_index + offset) % pending_initial_open_count;
                    selected.push(initial_entries_open[idx].clone());
                }
                selected
            } else {
                initial_entries_open
            };

        let include_active_walls_snapshot = !scheduled_initial_entries.is_empty();
        let throttle_delta_broadcasts = tail_join_mode
            || tail_wave_70_plus_mode
            || pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN;
        let mut max_delta_per_frame =
            if pending_initial_total_count >= MASS_JOIN_HEAVY_PENDING_INITIAL_MIN {
                MASS_JOIN_MAX_DELTA_PER_FRAME_HEAVY
            } else if pending_initial_total_count >= MASS_JOIN_MEDIUM_PENDING_INITIAL_MIN {
                MASS_JOIN_MAX_DELTA_PER_FRAME_MEDIUM
            } else {
                usize::MAX
            };
        if tail_join_mode {
            max_delta_per_frame = max_delta_per_frame.min(TAIL_JOIN_MAX_DELTA_PER_FRAME);
        }
        if aggressive_tail_join_mode {
            max_delta_per_frame = max_delta_per_frame.min(TAIL_JOIN_AGGRESSIVE_MAX_DELTA_PER_FRAME);
        }
        if tail_wave_70_plus_mode {
            max_delta_per_frame = max_delta_per_frame.min(TAIL_WAVE_70_PLUS_MAX_DELTA_PER_FRAME);
        }
        if extreme_tail_join_mode {
            max_delta_per_frame = max_delta_per_frame.min(EXTREME_TAIL_WAVE_MAX_DELTA_PER_FRAME);
        }
        if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            max_delta_per_frame = max_delta_per_frame.min(SINGLE_MACHINE_MAX_DELTA_PER_FRAME);
        }
        let mut delta_skip_modulus = if extreme_tail_join_mode {
            EXTREME_TAIL_WAVE_DELTA_SKIP_MODULUS
        } else if tail_wave_70_plus_mode {
            TAIL_WAVE_70_PLUS_DELTA_SKIP_MODULUS
        } else if aggressive_tail_join_mode {
            TAIL_JOIN_AGGRESSIVE_DELTA_SKIP_MODULUS
        } else if tail_join_mode {
            TAIL_JOIN_DELTA_SKIP_MODULUS
        } else if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            SINGLE_MACHINE_DELTA_SKIP_MODULUS
        } else {
            MASS_JOIN_DELTA_SKIP_MODULUS
        };

        let quality = self.current_quality_settings();
        max_delta_events_per_client =
            ((max_delta_events_per_client as f32) * quality.max_projectiles_scale)
                .round()
                .clamp(1.0, MAX_DELTA_EVENTS_DEFAULT as f32) as usize;
        delta_skip_modulus = delta_skip_modulus.max(quality.delta_skip_modulus);

        let mut scheduled_client_entries = scheduled_initial_entries;
        let mut scheduled_delta_count = 0usize;
        for (peer_id, data_channel, needs_initial) in delta_entries {
            if throttle_delta_broadcasts && current_frame % delta_skip_modulus != 0 {
                continue;
            }
            if scheduled_delta_count >= max_delta_per_frame {
                continue;
            }
            scheduled_client_entries.push((peer_id, data_channel, needs_initial));
            scheduled_delta_count += 1;
        }

        debug!(
            "[Frame {}] Join scheduler: tracked_clients_total={}, tracked_clients_open={}, pending_initial_total={}, pending_initial_open={}, pending_initial_closed={}, pending_delta_closed={}, tail_policy_enabled={}, tail_join_mode={}, aggressive_tail_join_mode={}, tail_wave_70_plus_mode={}, extreme_tail_join_mode={}, single_machine_opt={}, soa_fallback_active={}, initial_budget={}, delta_budget={}, delta_skip_modulus={}, delta_event_budget={}, snapshot_caps={{players:{}, walls:{}, projectiles:{}, pickups:{}}}, scheduled_initial={}, scheduled_delta={}",
            current_frame,
            connected_clients_total,
            connected_clients_open,
            pending_initial_total_count,
            pending_initial_open_count,
            pending_initial_closed_count,
            pending_delta_closed_count,
            tail_policy_enabled,
            tail_join_mode,
            aggressive_tail_join_mode,
            tail_wave_70_plus_mode,
            extreme_tail_join_mode,
            single_machine_opt,
            soa_adaptive_fallback_active,
            max_initial_per_frame,
            max_delta_per_frame,
            delta_skip_modulus,
            max_delta_events_per_client,
            initial_snapshot_caps.max_players,
            initial_snapshot_caps.max_walls,
            initial_snapshot_caps.max_projectiles,
            initial_snapshot_caps.max_pickups,
            scheduled_client_entries
                .iter()
                .filter(|(_, _, needs_initial)| *needs_initial)
                .count(),
            scheduled_delta_count
        );

        let mut scheduled_peer_ids: Vec<String> = scheduled_client_entries
            .iter()
            .map(|(peer_id, _, _)| peer_id.clone())
            .collect();
        for (peer_id, _) in &quic_entries {
            if !scheduled_peer_ids
                .iter()
                .any(|existing| existing == peer_id)
            {
                scheduled_peer_ids.push(peer_id.clone());
            }
        }

        let shared_broadcast_data = Arc::new(
            self.prepare_shared_broadcast_data(
                include_active_walls_snapshot,
                initial_snapshot_caps,
                tail_join_mode,
                aggressive_tail_join_mode,
                extreme_tail_join_mode,
                soa_adaptive_fallback_active,
                max_delta_events_per_client,
                &scheduled_peer_ids,
            )
            .await,
        );
        trace!("[Frame {}] Prepared shared broadcast data. Events: {}, Destroyed Walls: {}, ChatPackets: {}, KF: {}, use_soa_snapshot={}, use_entity_soa_snapshot={}, soa_fallback_active={}",
            current_frame, shared_broadcast_data.events.len(), shared_broadcast_data.destroyed_wall_ids.len(),
            shared_broadcast_data.chat_packets.len(), shared_broadcast_data.kill_feed_snapshot.len(),
            shared_broadcast_data.use_soa_snapshot, shared_broadcast_data.use_entity_soa_snapshot, shared_broadcast_data.soa_fallback_active);

        let mut broadcast_concurrency = (self
            .config
            .thread_pools
            .networking_threads
            .saturating_mul(4))
        .clamp(MIN_BROADCAST_CONCURRENCY, MAX_BROADCAST_CONCURRENCY);
        if pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN {
            broadcast_concurrency = broadcast_concurrency.min(MASS_JOIN_CONCURRENCY_CAP);
        }
        if tail_join_mode {
            broadcast_concurrency = broadcast_concurrency.min(TAIL_JOIN_CONCURRENCY_CAP);
        }
        if aggressive_tail_join_mode {
            broadcast_concurrency = broadcast_concurrency.min(TAIL_JOIN_AGGRESSIVE_CONCURRENCY_CAP);
        }
        if tail_wave_70_plus_mode {
            broadcast_concurrency = broadcast_concurrency.min(TAIL_WAVE_70_PLUS_CONCURRENCY_CAP);
        }
        if extreme_tail_join_mode {
            broadcast_concurrency = broadcast_concurrency.min(EXTREME_TAIL_WAVE_CONCURRENCY_CAP);
        }
        if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            broadcast_concurrency = broadcast_concurrency.min(SINGLE_MACHINE_CONCURRENCY_CAP);
        }

        if broadcast_work_stealing_enabled() {
            let runtime_handle = tokio::runtime::Handle::current();
            let server_ref = Arc::clone(&self);
            let shared_data_ref = Arc::clone(&shared_broadcast_data);
            let frame_for_log = current_frame;

            self.thread_pools.network_pool.install(move || {
                scheduled_client_entries.into_par_iter().for_each(
                    |(peer_id_str, data_channel_arc, needs_initial)| {
                        let server_ref = Arc::clone(&server_ref);
                        let shared_data_ref = Arc::clone(&shared_data_ref);
                        let runtime_handle = runtime_handle.clone();

                        let client_info = ClientInfo {
                            data_channel: data_channel_arc,
                            needs_initial_state: needs_initial,
                        };

                        let result = runtime_handle.block_on(async {
                            Self::process_client_broadcast(
                                &peer_id_str,
                                &client_info,
                                shared_data_ref.as_ref(),
                                &server_ref,
                            )
                            .await
                        });
                        if let Err(err) = result {
                            error!(
                                "[Frame {}] Work-stealing broadcast failed for {}: {}",
                                frame_for_log, peer_id_str, err
                            );
                        }
                    },
                );
            });
        } else {
            let mut fanout_tasks = JoinSet::new();
            for (peer_id_str, data_channel_arc, needs_initial) in scheduled_client_entries {
                let server_ref = Arc::clone(&self);
                let shared_data_ref = Arc::clone(&shared_broadcast_data);

                fanout_tasks.spawn(async move {
                    let client_info = ClientInfo {
                        data_channel: data_channel_arc,
                        needs_initial_state: needs_initial,
                    };

                    trace!(
                        "[Frame {}] Processing client: {}, Needs Initial: {}",
                        current_frame,
                        peer_id_str,
                        client_info.needs_initial_state
                    );

                    if let Err(e) = Self::process_client_broadcast(
                        &peer_id_str,
                        &client_info,
                        shared_data_ref.as_ref(),
                        &server_ref,
                    )
                    .await
                    {
                        error!(
                            "[Frame {}] Error processing broadcast for client {}: {:?}",
                            current_frame, peer_id_str, e
                        );
                    }
                });

                if fanout_tasks.len() >= broadcast_concurrency {
                    if let Some(join_result) = fanout_tasks.join_next().await {
                        if let Err(join_err) = join_result {
                            error!(
                                "[Frame {}] Broadcast fanout task join error: {}",
                                current_frame, join_err
                            );
                        }
                    }
                }
            }

            while let Some(join_result) = fanout_tasks.join_next().await {
                if let Err(join_err) = join_result {
                    error!(
                        "[Frame {}] Broadcast fanout task join error: {}",
                        current_frame, join_err
                    );
                }
            }
        }

        if !quic_entries.is_empty() {
            let mut quic_tasks = JoinSet::new();
            for (peer_id_str, needs_initial) in quic_entries {
                let server_ref = Arc::clone(&self);
                let shared_data_ref = Arc::clone(&shared_broadcast_data);

                quic_tasks.spawn(async move {
                    if let Err(err) = Self::process_quic_client_broadcast(
                        &peer_id_str,
                        needs_initial,
                        shared_data_ref.as_ref(),
                        &server_ref,
                    )
                    .await
                    {
                        error!(
                            "[Frame {}] Error processing QUIC broadcast for {}: {}",
                            current_frame, peer_id_str, err
                        );
                    }
                });

                if quic_tasks.len() >= broadcast_concurrency {
                    if let Some(join_result) = quic_tasks.join_next().await {
                        if let Err(join_err) = join_result {
                            error!(
                                "[Frame {}] QUIC broadcast task join error: {}",
                                current_frame, join_err
                            );
                        }
                    }
                }
            }

            while let Some(join_result) = quic_tasks.join_next().await {
                if let Err(join_err) = join_result {
                    error!(
                        "[Frame {}] QUIC broadcast task join error: {}",
                        current_frame, join_err
                    );
                }
            }
        }

        debug!(
            "[Frame {}] Broadcast processing loop complete.",
            current_frame
        );
    }
}
