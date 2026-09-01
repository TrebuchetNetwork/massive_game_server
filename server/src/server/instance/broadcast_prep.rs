use super::*;

impl MassiveGameServer {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn prepare_shared_broadcast_data(
        &self,
        include_active_walls_snapshot: bool,
        initial_snapshot_caps: InitialSnapshotCaps,
        tail_join_mode: bool,
        aggressive_tail_join_mode: bool,
        extreme_tail_join_mode: bool,
        _disable_soa_snapshot_for_backlog: bool,
        max_delta_events_per_client: usize,
        scheduled_peer_ids: &[String],
    ) -> SharedBroadcastData {
        let current_timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Collect events. The old 100-event cap drained at most ~6k events/s
        // (100 x 60 broadcasts), which dense 24-bot combat outproduced — the
        // queue pinned at max_events and dropped everything new
        // indiscriminately (70k+ drops observed live). Drain a much larger
        // batch; per-client delivery is still bounded downstream by
        // max_delta_events_per_client and AoI filtering.
        let events = self.global_game_events.pop_batch(2048);

        // Snapshot destroyed walls
        let destroyed_wall_ids = self
            .destroyed_wall_ids_this_tick
            .read()
            .iter()
            .cloned()
            .collect();

        // Snapshot updated walls
        let updated_walls = self.updated_walls_this_tick.read().clone();

        // Snapshot active walls once per broadcast and index them for fast dynamic wall streaming.
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let active_walls_cached = self.get_active_walls_cached(frame).await;
        let mut active_walls_by_id = HashMap::with_capacity(active_walls_cached.len());
        for wall in active_walls_cached.iter() {
            active_walls_by_id.insert(wall.id, wall.clone());
        }
        let active_walls_snapshot = if include_active_walls_snapshot {
            active_walls_cached.iter().cloned().collect()
        } else {
            Vec::new()
        };

        let use_aoi_snapshot = join_authoritative_aoi_snapshot_enabled();
        let player_aois_snapshot = if use_aoi_snapshot {
            // Resolve AoI from authoritative lock-free snapshot and only keep entries
            // for peers scheduled this frame.
            let authoritative_aoi_snapshot = self.snapshots.player_aoi_snapshot.load();
            if authoritative_aoi_snapshot.is_empty() && !scheduled_peer_ids.is_empty() {
                debug!(
                    "[Frame {}] Authoritative AoI snapshot is empty while {} peers are scheduled.",
                    frame,
                    scheduled_peer_ids.len()
                );
            }

            let mut scheduled_aoi_snapshot = HashMap::with_capacity(scheduled_peer_ids.len());
            for peer_id in scheduled_peer_ids {
                let player_id = self.player_manager.id_pool.get_or_create(peer_id);
                if let Some(aoi) = authoritative_aoi_snapshot.get_aoi(&player_id) {
                    scheduled_aoi_snapshot.insert(player_id, aoi.clone());
                }
            }
            Arc::new(scheduled_aoi_snapshot)
        } else {
            Arc::new(HashMap::new())
        };

        let configured_soa_snapshot = join_soa_snapshot_enabled();
        let use_soa_snapshot = configured_soa_snapshot;
        let mut soa_fallback_active = false;
        let (player_soa_snapshot, player_states_snapshot) = if use_soa_snapshot {
            let mut snapshot = self.snapshots.player_soa_snapshot.load();
            if snapshot.is_empty() && !scheduled_peer_ids.is_empty() {
                debug!(
                    "[Frame {}] Player SoA snapshot is empty while {} peers are scheduled.",
                    frame,
                    scheduled_peer_ids.len()
                );
                snapshot = self.rebuild_player_soa_snapshot_from_authoritative_state();
                self.snapshots
                    .player_soa_snapshot
                    .publish_arc(Arc::clone(&snapshot));
                soa_fallback_active = true;
            }
            (snapshot, HashMap::new())
        } else {
            let mut by_id = HashMap::with_capacity(self.player_manager.player_count());
            self.player_manager
                .for_each_player(|player_id, player_state| {
                    by_id.insert(player_id.clone(), player_state.clone());
                });
            (Arc::new(PlayerSoASnapshot::default()), by_id)
        };

        // Snapshot projectiles/pickups once per tick (reused for all client delta builds).
        let configured_entity_soa_snapshot = join_entity_soa_snapshot_enabled();
        let use_entity_soa_snapshot = configured_entity_soa_snapshot;

        let (projectiles_soa_snapshot, projectiles_snapshot) = {
            if use_entity_soa_snapshot {
                let mut snapshot = self.snapshots.projectile_soa_snapshot.load();
                if snapshot.is_empty() && !scheduled_peer_ids.is_empty() {
                    debug!(
                        "[Frame {}] Projectile SoA snapshot is empty while {} peers are scheduled.",
                        frame,
                        scheduled_peer_ids.len()
                    );
                    snapshot = self.rebuild_projectile_soa_snapshot_from_authoritative_state();
                    self.snapshots
                        .projectile_soa_snapshot
                        .publish_arc(Arc::clone(&snapshot));
                    soa_fallback_active = true;
                }
                (snapshot, Arc::new(HashMap::new()))
            } else {
                let projectiles_guard = self.projectiles.read();
                let mut by_id = HashMap::with_capacity(projectiles_guard.len());
                for projectile in projectiles_guard.iter() {
                    by_id.insert(projectile.id, projectile.clone());
                }
                (Arc::new(ProjectileSoASnapshot::default()), Arc::new(by_id))
            }
        };

        let (pickups_soa_snapshot, pickups_snapshot) = {
            if use_entity_soa_snapshot {
                let mut snapshot = self.snapshots.pickup_soa_snapshot.load();
                if snapshot.is_empty() && !scheduled_peer_ids.is_empty() {
                    debug!(
                        "[Frame {}] Pickup SoA snapshot is empty while {} peers are scheduled.",
                        frame,
                        scheduled_peer_ids.len()
                    );
                    snapshot = self.rebuild_pickup_soa_snapshot_from_authoritative_state();
                    self.snapshots
                        .pickup_soa_snapshot
                        .publish_arc(Arc::clone(&snapshot));
                    soa_fallback_active = true;
                }
                (snapshot, Arc::new(HashMap::new()))
            } else {
                let pickups_guard = self.pickups.read();
                let mut by_id = HashMap::with_capacity(pickups_guard.len());
                for pickup in pickups_guard.iter() {
                    by_id.insert(pickup.id, pickup.clone());
                }
                (Arc::new(PickupSoASnapshot::default()), Arc::new(by_id))
            }
        };

        // Snapshot serialized chat packets once per broadcast.
        let chat_packets = self
            .chat_messages_queue
            .read()
            .await
            .iter()
            .map(|chat_entry| SerializedChatPacket {
                seq: chat_entry.seq,
                bytes: build_chat_game_message_bytes(chat_entry),
            })
            .collect();

        // Snapshot match info (read once)
        self.refresh_commander_runtime_state(current_timestamp_ms);
        let team1_commander_id = self
            .commander_id_for_team(1)
            .map(|commander_id| commander_id.as_ref().to_owned());
        let team2_commander_id = self
            .commander_id_for_team(2)
            .map(|commander_id| commander_id.as_ref().to_owned());
        let team1_commander_waypoint = self.commander_primary_waypoint_for_team(1);
        let team2_commander_waypoint = self.commander_primary_waypoint_for_team(2);
        let team1_commander_attack_bias = self.commander_attack_bias_for_team(1).unwrap_or(0.0);
        let team2_commander_attack_bias = self.commander_attack_bias_for_team(2).unwrap_or(0.0);

        let match_info_guard = self.match_info.read();
        let match_info_snapshot = MatchInfoSnapshot {
            time_remaining: match_info_guard.time_remaining,
            match_state: match_info_guard.match_state,
            game_mode: match_info_guard.game_mode,
            team_scores: match_info_guard.team_scores.clone(),
            flag_states: match_info_guard.flag_states.clone(),
            team1_commander_id,
            team2_commander_id,
            team1_commander_waypoint,
            team2_commander_waypoint,
            team1_commander_attack_bias,
            team2_commander_attack_bias,
        };
        drop(match_info_guard);

        // Snapshot kill feed
        let kill_feed_snapshot = self.kill_feed.read().iter().cloned().collect();

        SharedBroadcastData {
            timestamp_ms: current_timestamp_ms,
            events,
            destroyed_wall_ids,
            updated_walls,
            active_walls_by_id,
            active_walls_snapshot,
            player_aois_snapshot,
            player_soa_snapshot,
            player_states_snapshot,
            projectiles_soa_snapshot,
            pickups_soa_snapshot,
            projectiles_snapshot,
            pickups_snapshot,
            chat_packets,
            match_info_snapshot,
            kill_feed_snapshot,
            max_delta_events_per_client,
            initial_snapshot_caps,
            tail_join_mode,
            aggressive_tail_join_mode,
            extreme_tail_join_mode,
            use_aoi_snapshot,
            soa_fallback_active,
            use_soa_snapshot,
            use_entity_soa_snapshot,
        }
    }
}
