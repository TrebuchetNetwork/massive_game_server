use super::*;

#[derive(Clone, Copy)]
struct InitialStateChunkBuildParams {
    include_walls: bool,
    include_players: bool,
    include_projectiles: bool,
    include_pickups: bool,
    players_self_only: bool,
}

impl MassiveGameServer {
    pub(super) fn update_client_state_after_initial(
        &self,
        peer_id_str: &str,
        shared_data: &SharedBroadcastData,
        last_chat_message_seq_sent: u64,
    ) {
        let frame_num = self.frame_counter.load(AtomicOrdering::Relaxed);
        trace!(
            "[Frame {}] Client {}: Preparing to set initial ClientState in DashMap.",
            frame_num,
            peer_id_str
        );
        let mut client_state = ClientState::default();
        client_state.known_walls_sent = true;
        client_state.last_update_sent_time = Instant::now();
        client_state.last_chat_message_seq_sent = last_chat_message_seq_sent;

        client_state.last_known_match_state = Some(shared_data.match_info_snapshot.match_state);
        client_state.last_known_match_time_remaining =
            Some(shared_data.match_info_snapshot.time_remaining);
        client_state.last_known_team_scores = shared_data.match_info_snapshot.team_scores.clone();
        client_state.match_info_pending = false;

        let snapshot_caps = shared_data.initial_snapshot_caps;
        let self_player_id_arc = self.player_manager.id_pool.get_or_create(peer_id_str);
        if let Some(self_pstate_guard) =
            Self::lookup_player_state_from_shared(shared_data, &self_player_id_arc)
        {
            client_state
                .last_known_player_states
                .insert(self_player_id_arc.clone(), self_pstate_guard.clone());
            client_state
                .last_known_players
                .insert(self_player_id_arc.clone());
        }

        let p_aoi = self.resolve_player_aoi_for_player(shared_data, &self_player_id_arc);
        for visible_player_id in p_aoi
            .visible_players
            .iter()
            .take(snapshot_caps.max_players.saturating_sub(1))
        {
            if let Some(pstate_guard) =
                Self::lookup_player_state_from_shared(shared_data, visible_player_id)
            {
                client_state
                    .last_known_player_states
                    .insert(visible_player_id.clone(), pstate_guard.clone());
            }
            client_state
                .last_known_players
                .insert(visible_player_id.clone());
        }
        client_state.last_known_projectile_ids = p_aoi
            .visible_projectiles
            .iter()
            .take(snapshot_caps.max_projectiles)
            .copied()
            .collect();
        for pickup_id in p_aoi.visible_pickups.iter().take(snapshot_caps.max_pickups) {
            if let Some(pickup) = Self::lookup_pickup_from_shared(shared_data, pickup_id) {
                client_state.last_known_pickup_states.insert(
                    *pickup_id,
                    PickupState {
                        is_active: pickup.is_active,
                    },
                );
            }
        }

        for wall_id in p_aoi.visible_walls.iter().take(AOI_MAX_VISIBLE_WALLS) {
            if let Some(wall_data) = shared_data.active_walls_by_id.get(wall_id) {
                client_state
                    .last_known_wall_states
                    .insert(*wall_id, (wall_data.current_health, wall_data.max_health));
            }
        }
        client_state.last_known_wall_ids = Some(
            p_aoi
                .visible_walls
                .iter()
                .take(AOI_MAX_VISIBLE_WALLS)
                .copied()
                .collect(),
        );

        let key_for_insert = peer_id_str.to_string();
        trace!(
            "[Frame {}] Client {}: ABOUT TO INSERT initial ClientState into client_states_map. Key: {}",
            frame_num,
            peer_id_str,
            key_for_insert
        );
        self.client_states_map
            .write()
            .insert(key_for_insert.clone(), client_state);
        trace!(
            "[Frame {}] Client {}: SUCCESSFULLY INSERTED initial ClientState into client_states_map. Key: {}",
            frame_num,
            peer_id_str,
            key_for_insert
        );
    }

    pub(super) fn update_client_state_after_delta(
        &self,
        client_state: &mut ClientState,
        player_id: &PlayerID,
        shared_data: &SharedBroadcastData,
    ) {
        let player_aoi = self.resolve_player_aoi_for_player(shared_data, player_id);

        client_state.last_broadcast_frame = self.frame_counter.load(AtomicOrdering::Relaxed);

        client_state.last_known_projectile_ids.clear();
        for projectile_id in &player_aoi.visible_projectiles {
            client_state
                .last_known_projectile_ids
                .insert(*projectile_id);
        }

        client_state.last_known_pickup_states.clear();
        for pickup_id in &player_aoi.visible_pickups {
            if let Some(pickup) = Self::lookup_pickup_from_shared(shared_data, pickup_id) {
                client_state.last_known_pickup_states.insert(
                    *pickup_id,
                    PickupState {
                        is_active: pickup.is_active,
                    },
                );
            }
        }

        client_state.last_known_players.clear();
        client_state.last_known_players.insert(player_id.clone());
        for visible_player_id in &player_aoi.visible_players {
            client_state
                .last_known_players
                .insert(visible_player_id.clone());
        }

        client_state.last_known_wall_states.clear();
        for wall_id in player_aoi.visible_walls.iter().take(AOI_MAX_VISIBLE_WALLS) {
            if let Some(wall_data) = shared_data.active_walls_by_id.get(wall_id) {
                client_state
                    .last_known_wall_states
                    .insert(*wall_id, (wall_data.current_health, wall_data.max_health));
            }
        }
        client_state.last_known_wall_ids = Some(
            player_aoi
                .visible_walls
                .iter()
                .take(AOI_MAX_VISIBLE_WALLS)
                .copied()
                .collect(),
        );

        trace!(
            "Updated client state for {}: {} projectiles, {} pickups, {} players tracked",
            player_id.as_str(),
            client_state.last_known_projectile_ids.len(),
            client_state.last_known_pickup_states.len(),
            client_state.last_known_players.len()
        );
    }

    pub(super) fn update_client_state_after_delta_with_shared(
        &self,
        client_state: &mut ClientState,
        shared_data: &SharedBroadcastData,
    ) {
        client_state.last_known_match_state = Some(shared_data.match_info_snapshot.match_state);
        client_state.last_known_match_time_remaining =
            Some(shared_data.match_info_snapshot.time_remaining);
        client_state.last_known_team_scores = shared_data.match_info_snapshot.team_scores.clone();
        client_state.match_info_pending = false;
        client_state.last_kill_feed_count_sent = shared_data.kill_feed_snapshot.len();
        for wall_id in &shared_data.destroyed_wall_ids {
            client_state.known_destroyed_wall_ids.insert(*wall_id);
        }
    }

    pub(super) fn resolve_player_aoi_for_player(
        &self,
        shared_data: &SharedBroadcastData,
        player_id: &PlayerID,
    ) -> PlayerAoI {
        if shared_data.use_aoi_snapshot {
            shared_data
                .player_aois_snapshot
                .get(player_id)
                .cloned()
                .unwrap_or_else(Self::get_empty_player_aoi)
        } else {
            self.player_aois
                .get(player_id.as_str())
                .map(|entry| entry.value().clone())
                .unwrap_or_else(Self::get_empty_player_aoi)
        }
    }

    #[inline]
    pub(super) fn lookup_player_state_from_shared<'a>(
        shared_data: &'a SharedBroadcastData,
        player_id: &PlayerID,
    ) -> Option<&'a PlayerState> {
        if shared_data.use_soa_snapshot {
            shared_data.player_soa_snapshot.get_state(player_id)
        } else {
            shared_data.player_states_snapshot.get(player_id)
        }
    }

    #[inline]
    pub(super) fn lookup_projectile_from_shared<'a>(
        shared_data: &'a SharedBroadcastData,
        projectile_id: &EntityId,
    ) -> Option<&'a Projectile> {
        if shared_data.use_entity_soa_snapshot {
            shared_data
                .projectiles_soa_snapshot
                .get_state(projectile_id)
        } else {
            shared_data.projectiles_snapshot.get(projectile_id)
        }
    }

    #[inline]
    pub(super) fn lookup_pickup_from_shared<'a>(
        shared_data: &'a SharedBroadcastData,
        pickup_id: &EntityId,
    ) -> Option<&'a Pickup> {
        if shared_data.use_entity_soa_snapshot {
            shared_data.pickups_soa_snapshot.get_state(pickup_id)
        } else {
            shared_data.pickups_snapshot.get(pickup_id)
        }
    }

    pub(super) fn build_delta_state_optimized(
        &self,
        peer_id_str: &str,
        client_state: &ClientState,
        shared_data: &SharedBroadcastData,
    ) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(16384);
        let build_start = Instant::now();
        let player_id = self.player_manager.id_pool.get_or_create(peer_id_str);
        let (own_team_id, own_is_spectator) =
            Self::lookup_player_state_from_shared(shared_data, &player_id)
                .map(|state| (state.team_id, state.is_spectator))
                .unwrap_or((0, false));

        trace!("[{}] DeltaBuilder: Started", peer_id_str);

        let player_aoi = self.resolve_player_aoi_for_player(shared_data, &player_id);

        let mut players_fb_vec = Vec::new();
        let mut player_fields_mask_vec: Vec<u8> = Vec::new();
        let mut removed_player_ids_vec = Vec::new();
        let encode_changed_mask = |mask: u16| -> u8 {
            if mask == 0xFFFF {
                u8::MAX
            } else {
                (mask & 0x00FF) as u8
            }
        };

        if let Some(self_state) = Self::lookup_player_state_from_shared(shared_data, &player_id) {
            let is_new = !client_state.last_known_players.contains(&player_id);
            if is_new || self_state.changed_fields > 0 {
                let mask = if is_new {
                    0xFFFF
                } else {
                    self_state.changed_fields
                };
                players_fb_vec.push(create_fb_player_state_for_delta(
                    &mut builder,
                    &self_state,
                    mask,
                ));
                player_fields_mask_vec.push(encode_changed_mask(mask));
            }
        }

        for visible_player_id in &player_aoi.visible_players {
            if visible_player_id != &player_id {
                if let Some(player_state) =
                    Self::lookup_player_state_from_shared(shared_data, visible_player_id)
                {
                    let is_new = !client_state.last_known_players.contains(visible_player_id);
                    if is_new || player_state.changed_fields > 0 {
                        let mask = if is_new {
                            0xFFFF
                        } else {
                            player_state.changed_fields
                        };
                        players_fb_vec.push(create_fb_player_state_for_delta(
                            &mut builder,
                            &player_state,
                            mask,
                        ));
                        player_fields_mask_vec.push(encode_changed_mask(mask));
                    }
                }
            }
        }

        for known_player_id in &client_state.last_known_players {
            if !player_aoi.visible_players.contains(known_player_id)
                && known_player_id != &player_id
            {
                removed_player_ids_vec.push(builder.create_string(known_player_id.as_str()));
            }
        }

        let players_fb = builder.create_vector(&players_fb_vec);
        let changed_player_fields_fb = if !player_fields_mask_vec.is_empty() {
            Some(builder.create_vector(&player_fields_mask_vec))
        } else {
            None
        };
        let removed_players_fb = builder.create_vector(&removed_player_ids_vec);

        let mut new_projectiles_vec = Vec::new();
        let mut removed_projectile_ids_vec = Vec::new();

        for proj_id in &player_aoi.visible_projectiles {
            if !client_state.last_known_projectile_ids.contains(proj_id) {
                if let Some(proj) = Self::lookup_projectile_from_shared(shared_data, proj_id) {
                    let id_str = fb_safe_entity_id(&mut builder, proj.id);
                    let owner_str = builder.create_string(proj.owner_id.as_str());

                    let proj_fb = fb::ProjectileState::create(
                        &mut builder,
                        &fb::ProjectileStateArgs {
                            id: Some(id_str),
                            x: proj.x,
                            y: proj.y,
                            owner_id: Some(owner_str),
                            weapon_type: map_server_weapon_to_fb(proj.weapon_type),
                            velocity_x: proj.velocity_x,
                            velocity_y: proj.velocity_y,
                        },
                    );
                    new_projectiles_vec.push(proj_fb);
                }
            }
        }

        for known_proj_id in &client_state.last_known_projectile_ids {
            if !player_aoi.visible_projectiles.contains(known_proj_id) {
                let id_str = fb_safe_entity_id(&mut builder, *known_proj_id);
                removed_projectile_ids_vec.push(id_str);
            }
        }

        let projectiles_fb = builder.create_vector(&new_projectiles_vec);
        let removed_projectiles_fb = builder.create_vector(&removed_projectile_ids_vec);

        let mut pickups_delta_vec = Vec::new();
        let mut deactivated_pickup_ids_vec = Vec::new();

        for pickup_id in &player_aoi.visible_pickups {
            if let Some(pickup) = Self::lookup_pickup_from_shared(shared_data, pickup_id) {
                let should_send = if let Some(last_known_state) =
                    client_state.last_known_pickup_states.get(pickup_id)
                {
                    last_known_state.is_active != pickup.is_active
                } else {
                    true
                };

                if should_send {
                    let (pickup_type_fb, weapon_type_fb) =
                        map_core_pickup_to_fb(&pickup.pickup_type);
                    let id_str = fb_safe_entity_id(&mut builder, pickup.id);

                    let pickup_fb = fb::Pickup::create(
                        &mut builder,
                        &fb::PickupArgs {
                            id: Some(id_str),
                            x: pickup.x,
                            y: pickup.y,
                            pickup_type: pickup_type_fb,
                            weapon_type: weapon_type_fb.unwrap_or(fb::WeaponType::Pistol),
                            is_active: pickup.is_active,
                        },
                    );
                    pickups_delta_vec.push(pickup_fb);
                }
            }
        }

        for (known_pickup_id, _) in &client_state.last_known_pickup_states {
            if !player_aoi.visible_pickups.contains(known_pickup_id) {
                let id_str = fb_safe_entity_id(&mut builder, *known_pickup_id);
                deactivated_pickup_ids_vec.push(id_str);
            }
        }

        let pickups_fb = builder.create_vector(&pickups_delta_vec);
        let deactivated_pickups_fb = builder.create_vector(&deactivated_pickup_ids_vec);

        let game_events_fb = if shared_data.max_delta_events_per_client == 0 {
            None
        } else {
            let events_vec: Vec<_> = shared_data
                .events
                .iter()
                .filter(|event| match event {
                    GameEvent::TeamPing { team_id, .. } => {
                        own_is_spectator || (own_team_id != 0 && *team_id == own_team_id)
                    }
                    _ => true,
                })
                .take(shared_data.max_delta_events_per_client)
                .map(|event| build_game_event_fb(&mut builder, event))
                .collect();
            if events_vec.is_empty() {
                None
            } else {
                Some(builder.create_vector(&events_vec))
            }
        };

        let kill_feed_vec: Vec<_> = shared_data
            .kill_feed_snapshot
            .iter()
            .skip(client_state.last_kill_feed_count_sent)
            .map(|entry| {
                let killer_name_fb = builder.create_string(&entry.killer_name);
                let victim_name_fb = builder.create_string(&entry.victim_name);
                fb::KillFeedEntry::create(
                    &mut builder,
                    &fb::KillFeedEntryArgs {
                        killer_name: Some(killer_name_fb),
                        victim_name: Some(victim_name_fb),
                        weapon: map_server_weapon_to_fb(entry.weapon),
                        timestamp: entry.timestamp as f32,
                        killer_position: None,
                        victim_position: None,
                        is_headshot: false,
                    },
                )
            })
            .collect();
        let kill_feed_fb = builder.create_vector(&kill_feed_vec);

        let match_info_fb = {
            let match_snapshot = &shared_data.match_info_snapshot;
            let team_scores_changed =
                client_state.last_known_team_scores != match_snapshot.team_scores;
            let time_changed = client_state
                .last_known_match_time_remaining
                .map_or(true, |t| (t - match_snapshot.time_remaining).abs() > 0.5);
            let state_changed = client_state
                .last_known_match_state
                .map_or(true, |s| s != match_snapshot.match_state);
            if client_state.match_info_pending
                || state_changed
                || time_changed
                || team_scores_changed
            {
                let team_scores_vec: Vec<_> = match_snapshot
                    .team_scores
                    .iter()
                    .map(|(team_id, score)| {
                        fb::TeamScoreEntry::create(
                            &mut builder,
                            &fb::TeamScoreEntryArgs {
                                team_id: *team_id as i8,
                                score: *score,
                            },
                        )
                    })
                    .collect();
                let team_scores_fb = builder.create_vector(&team_scores_vec);
                Some(fb::MatchInfo::create(
                    &mut builder,
                    &fb::MatchInfoArgs {
                        time_remaining: match_snapshot.time_remaining,
                        match_state: match_snapshot.match_state,
                        winner_id: None,
                        winner_name: None,
                        game_mode: match_snapshot.game_mode,
                        team_scores: Some(team_scores_fb),
                    },
                ))
            } else {
                None
            }
        };

        let destroyed_walls_vec: Vec<_> = shared_data
            .destroyed_wall_ids
            .iter()
            .filter(|id| !client_state.known_destroyed_wall_ids.contains(*id))
            .map(|id| fb_safe_entity_id(&mut builder, *id))
            .collect();
        let destroyed_wall_ids_fb = if !destroyed_walls_vec.is_empty() {
            Some(builder.create_vector(&destroyed_walls_vec))
        } else {
            None
        };

        let mut updated_walls_vec = Vec::new();
        let mut updated_wall_ids_sent = HashSet::new();

        for (wall_id, wall_data) in shared_data.updated_walls.iter() {
            if player_aoi.visible_walls.contains(wall_id) {
                let id_fb = fb_safe_entity_id(&mut builder, wall_data.id);
                let wall_fb = fb::Wall::create(
                    &mut builder,
                    &fb::WallArgs {
                        id: Some(id_fb),
                        x: wall_data.x,
                        y: wall_data.y,
                        width: wall_data.width,
                        height: wall_data.height,
                        is_destructible: wall_data.is_destructible,
                        current_health: wall_data.current_health,
                        max_health: wall_data.max_health,
                    },
                );
                updated_walls_vec.push(wall_fb);
                updated_wall_ids_sent.insert(*wall_id);
                if updated_walls_vec.len() >= AOI_MAX_VISIBLE_WALLS {
                    break;
                }
            }
        }

        if updated_walls_vec.len() < AOI_MAX_VISIBLE_WALLS {
            for visible_wall_id in &player_aoi.visible_walls {
                if updated_wall_ids_sent.contains(visible_wall_id) {
                    continue;
                }

                let wall_data = match shared_data.active_walls_by_id.get(visible_wall_id) {
                    Some(wall) => wall,
                    None => continue,
                };

                let should_send = client_state
                    .last_known_wall_states
                    .get(visible_wall_id)
                    .map_or(true, |(known_health, known_max_health)| {
                        *known_health != wall_data.current_health
                            || *known_max_health != wall_data.max_health
                    });

                if !should_send {
                    continue;
                }

                let id_fb = fb_safe_entity_id(&mut builder, wall_data.id);
                let wall_fb = fb::Wall::create(
                    &mut builder,
                    &fb::WallArgs {
                        id: Some(id_fb),
                        x: wall_data.x,
                        y: wall_data.y,
                        width: wall_data.width,
                        height: wall_data.height,
                        is_destructible: wall_data.is_destructible,
                        current_health: wall_data.current_health,
                        max_health: wall_data.max_health,
                    },
                );
                updated_walls_vec.push(wall_fb);
                updated_wall_ids_sent.insert(*visible_wall_id);
                if updated_walls_vec.len() >= AOI_MAX_VISIBLE_WALLS {
                    break;
                }
            }
        }

        let updated_walls_fb = if !updated_walls_vec.is_empty() {
            Some(builder.create_vector(&updated_walls_vec))
        } else {
            None
        };

        let delta_state_args = fb::DeltaStateMessageArgs {
            players: Some(players_fb),
            projectiles: Some(projectiles_fb),
            removed_projectiles: Some(removed_projectiles_fb),
            pickups: Some(pickups_fb),
            deactivated_pickup_ids: Some(deactivated_pickups_fb),
            game_events: game_events_fb,
            timestamp: shared_data.timestamp_ms,
            last_processed_input_sequence: 0,
            changed_player_fields: changed_player_fields_fb,
            kill_feed: Some(kill_feed_fb),
            match_info: match_info_fb,
            destroyed_wall_ids: destroyed_wall_ids_fb,
            flag_states: None,
            removed_player_ids: Some(removed_players_fb),
            updated_walls: updated_walls_fb,
        };

        let delta_state = fb::DeltaStateMessage::create(&mut builder, &delta_state_args);

        let game_msg = fb::GameMessage::create(
            &mut builder,
            &fb::GameMessageArgs {
                msg_type: fb::MessageType::DeltaState,
                actual_message_type: fb::MessagePayload::DeltaStateMessage,
                actual_message: Some(delta_state.as_union_value()),
                protocol_version: GAME_PROTOCOL_VERSION,
            },
        );

        builder.finish(game_msg, None);
        let (buffer, root_index) = builder.collapse();
        let bytes = Bytes::from(buffer).slice(root_index..);

        trace!(
            "[{}] DeltaBuilder: Completed in {:?}",
            peer_id_str,
            build_start.elapsed()
        );
        Ok(bytes)
    }

    pub(super) fn build_initial_state_sequence_optimized(
        &self,
        peer_id_str: &str,
        shared_data: &SharedBroadcastData,
    ) -> Result<VecDeque<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
        let mut chunks = VecDeque::new();

        if !join_initial_state_chunking_enabled() {
            chunks.push_back(self.build_initial_state_optimized(peer_id_str, shared_data)?);
            return Ok(chunks);
        }

        let walls_chunk = self.build_initial_state_chunk_optimized(
            peer_id_str,
            shared_data,
            InitialStateChunkBuildParams {
                include_walls: true,
                include_players: true,
                include_projectiles: false,
                include_pickups: false,
                players_self_only: true,
            },
        )?;
        chunks.push_back(walls_chunk);

        let players_chunk = self.build_initial_state_chunk_optimized(
            peer_id_str,
            shared_data,
            InitialStateChunkBuildParams {
                include_walls: false,
                include_players: true,
                include_projectiles: false,
                include_pickups: false,
                players_self_only: false,
            },
        )?;
        chunks.push_back(players_chunk);

        let dynamic_chunk = self.build_initial_state_chunk_optimized(
            peer_id_str,
            shared_data,
            InitialStateChunkBuildParams {
                include_walls: false,
                include_players: false,
                include_projectiles: true,
                include_pickups: true,
                players_self_only: false,
            },
        )?;
        chunks.push_back(dynamic_chunk);

        Ok(chunks)
    }

    pub(super) fn build_initial_state_optimized(
        &self,
        peer_id_str: &str,
        shared_data: &SharedBroadcastData,
    ) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        self.build_initial_state_chunk_optimized(
            peer_id_str,
            shared_data,
            InitialStateChunkBuildParams {
                include_walls: true,
                include_players: true,
                include_projectiles: true,
                include_pickups: true,
                players_self_only: false,
            },
        )
    }

    fn build_initial_state_chunk_optimized(
        &self,
        peer_id_str: &str,
        shared_data: &SharedBroadcastData,
        params: InitialStateChunkBuildParams,
    ) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        const MAX_MESSAGE_SIZE_BYTES: usize = 160000;

        let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(32768);
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let snapshot_caps = shared_data.initial_snapshot_caps;
        debug!(
            "[Frame {}] Client {}: Building InitialStateMessage chunk (walls={}, players={}, projectiles={}, pickups={}, self_only={}).",
            frame,
            peer_id_str,
            params.include_walls,
            params.include_players,
            params.include_projectiles,
            params.include_pickups,
            params.players_self_only
        );

        let self_player_id_arc = self.player_manager.id_pool.get_or_create(peer_id_str);

        let active_walls_to_send: Cow<'_, [Wall]> = if shared_data.active_walls_snapshot.is_empty()
        {
            let mut fallback_walls: Vec<Wall> =
                shared_data.active_walls_by_id.values().cloned().collect();
            fallback_walls.sort_by_key(|wall| wall.id);
            Cow::Owned(fallback_walls)
        } else {
            Cow::Borrowed(shared_data.active_walls_snapshot.as_slice())
        };

        let mut walls_fb_vec = Vec::new();
        if params.include_walls {
            walls_fb_vec.reserve(active_walls_to_send.len().min(snapshot_caps.max_walls));
            for wall_data in active_walls_to_send.iter().take(snapshot_caps.max_walls) {
                let id_fb = fb_safe_entity_id(&mut builder, wall_data.id);
                walls_fb_vec.push(fb::Wall::create(
                    &mut builder,
                    &fb::WallArgs {
                        id: Some(id_fb),
                        x: wall_data.x,
                        y: wall_data.y,
                        width: wall_data.width,
                        height: wall_data.height,
                        is_destructible: wall_data.is_destructible,
                        current_health: wall_data.current_health,
                        max_health: wall_data.max_health,
                    },
                ));
            }
        }
        let walls_fb = builder.create_vector(&walls_fb_vec);

        let mut players_fb_vec = Vec::new();
        let mut player_aoi_data_for_initial_state = Self::get_empty_player_aoi();

        if let Some(self_pstate_guard) =
            Self::lookup_player_state_from_shared(shared_data, &self_player_id_arc)
        {
            player_aoi_data_for_initial_state =
                self.resolve_player_aoi_for_player(shared_data, &self_player_id_arc);
            if params.include_players {
                players_fb_vec.push(create_fb_player_state_for_delta(
                    &mut builder,
                    self_pstate_guard,
                    0xFFFF,
                ));
            }
        } else {
            warn!(
                "[Frame {} Client {}] InitialState chunk: self player state not found!",
                frame, peer_id_str
            );
        }

        if params.include_players && !params.players_self_only {
            for visible_player_id in player_aoi_data_for_initial_state
                .visible_players
                .iter()
                .take(
                    snapshot_caps
                        .max_players
                        .saturating_sub(players_fb_vec.len()),
                )
            {
                if visible_player_id == &self_player_id_arc {
                    continue;
                }
                if let Some(pstate_guard) =
                    Self::lookup_player_state_from_shared(shared_data, visible_player_id)
                {
                    players_fb_vec.push(create_fb_player_state_for_delta(
                        &mut builder,
                        pstate_guard,
                        0xFFFF,
                    ));
                }
            }
        }
        let players_fb = builder.create_vector(&players_fb_vec);

        let mut projectiles_fb_vec = Vec::new();
        if params.include_projectiles {
            for proj_id in player_aoi_data_for_initial_state
                .visible_projectiles
                .iter()
                .take(snapshot_caps.max_projectiles)
            {
                if let Some(proj) = Self::lookup_projectile_from_shared(shared_data, proj_id) {
                    let id_fb = fb_safe_entity_id(&mut builder, proj.id);
                    let owner_id_fb = fb_safe_str(&mut builder, proj.owner_id.as_str());
                    projectiles_fb_vec.push(fb::ProjectileState::create(
                        &mut builder,
                        &fb::ProjectileStateArgs {
                            id: Some(id_fb),
                            x: proj.x,
                            y: proj.y,
                            owner_id: Some(owner_id_fb),
                            weapon_type: map_server_weapon_to_fb(proj.weapon_type),
                            velocity_x: proj.velocity_x,
                            velocity_y: proj.velocity_y,
                        },
                    ));
                }
            }
        }
        let projectiles_fb = builder.create_vector(&projectiles_fb_vec);

        let mut pickups_fb_vec = Vec::new();
        if params.include_pickups {
            for pickup_id in player_aoi_data_for_initial_state
                .visible_pickups
                .iter()
                .take(snapshot_caps.max_pickups)
            {
                if let Some(pickup) = Self::lookup_pickup_from_shared(shared_data, pickup_id) {
                    if pickup.is_active {
                        let (fb_pickup_type, fb_weapon_type_opt) =
                            map_core_pickup_to_fb(&pickup.pickup_type);
                        let id_fb = fb_safe_entity_id(&mut builder, pickup.id);
                        pickups_fb_vec.push(fb::Pickup::create(
                            &mut builder,
                            &fb::PickupArgs {
                                id: Some(id_fb),
                                x: pickup.x,
                                y: pickup.y,
                                pickup_type: fb_pickup_type,
                                weapon_type: fb_weapon_type_opt.unwrap_or(fb::WeaponType::Pistol),
                                is_active: pickup.is_active,
                            },
                        ));
                    }
                }
            }
        }
        let pickups_fb = builder.create_vector(&pickups_fb_vec);

        let match_snapshot = &shared_data.match_info_snapshot;
        let fb_team_scores_vec: Vec<_> = match_snapshot
            .team_scores
            .iter()
            .map(|(team_id, score)| {
                fb::TeamScoreEntry::create(
                    &mut builder,
                    &fb::TeamScoreEntryArgs {
                        team_id: *team_id as i8,
                        score: *score,
                    },
                )
            })
            .collect();
        let team_scores_fb = builder.create_vector(&fb_team_scores_vec);

        let match_info_fb = fb::MatchInfo::create(
            &mut builder,
            &fb::MatchInfoArgs {
                time_remaining: match_snapshot.time_remaining,
                match_state: match_snapshot.match_state,
                winner_id: None,
                winner_name: None,
                game_mode: match_snapshot.game_mode,
                team_scores: Some(team_scores_fb),
            },
        );

        let fb_flag_states_vec: Vec<_> = match_snapshot
            .flag_states
            .values()
            .map(|fs| {
                let carrier_id_fb = fs
                    .carrier_id
                    .as_ref()
                    .map(|id| fb_safe_str(&mut builder, id.as_str()));
                let pos_fb = fb::Vec2::create(
                    &mut builder,
                    &fb::Vec2Args {
                        x: fs.position.x,
                        y: fs.position.y,
                    },
                );
                fb::FlagState::create(
                    &mut builder,
                    &fb::FlagStateArgs {
                        team_id: fs.team_id as i8,
                        status: fs.status,
                        position: Some(pos_fb),
                        carrier_id: carrier_id_fb,
                        respawn_timer: fs.respawn_timer,
                    },
                )
            })
            .collect();
        let flag_states_fb = builder.create_vector(&fb_flag_states_vec);

        let map_name_fb = fb_safe_str(&mut builder, &self.map_name);
        let timestamp_initial = shared_data.timestamp_ms;
        let player_id_fb_initial = fb_safe_str(&mut builder, peer_id_str);

        let initial_state_args = fb::InitialStateMessageArgs {
            player_id: Some(player_id_fb_initial),
            walls: Some(walls_fb),
            players: Some(players_fb),
            projectiles: Some(projectiles_fb),
            pickups: Some(pickups_fb),
            match_info: Some(match_info_fb),
            flag_states: Some(flag_states_fb),
            timestamp: timestamp_initial,
            map_name: Some(map_name_fb),
        };
        let initial_state_msg = fb::InitialStateMessage::create(&mut builder, &initial_state_args);

        let game_msg_args = fb::GameMessageArgs {
            msg_type: fb::MessageType::InitialState,
            actual_message_type: fb::MessagePayload::InitialStateMessage,
            actual_message: Some(initial_state_msg.as_union_value()),
            protocol_version: GAME_PROTOCOL_VERSION,
        };
        let game_msg = fb::GameMessage::create(&mut builder, &game_msg_args);
        builder.finish(game_msg, None);

        let finished_len = builder.finished_data().len();
        debug!(
            "[Frame {} Client {}] InitialState chunk built. Size: {} bytes (walls={}, players={}, projectiles={}, pickups={}).",
            frame,
            peer_id_str,
            finished_len,
            walls_fb_vec.len(),
            players_fb_vec.len(),
            projectiles_fb_vec.len(),
            pickups_fb_vec.len()
        );

        if finished_len > MAX_MESSAGE_SIZE_BYTES {
            return Err("Initial state chunk too large".into());
        }

        let (buffer, root_index) = builder.collapse();
        Ok(Bytes::from(buffer).slice(root_index..))
    }

    pub(super) async fn send_chat_messages_optimized(
        &self,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
        last_seq_sent: u64,
        chat_packets: &[SerializedChatPacket],
    ) -> u64 {
        const CHAT_PACKET_TIMEOUT_MS: u64 = 30;

        let packets_to_send: Vec<&SerializedChatPacket> = chat_packets
            .iter()
            .filter(|packet| packet.seq > last_seq_sent)
            .take(MAX_CHAT_PER_BATCH)
            .collect();
        if packets_to_send.is_empty() {
            return last_seq_sent;
        }

        let serialized_packets: Vec<Bytes> = packets_to_send
            .iter()
            .map(|packet| packet.bytes.clone())
            .collect();
        let sent_packets = self
            .send_packet_batch_optimized(data_channel, &serialized_packets, CHAT_PACKET_TIMEOUT_MS)
            .await;

        let mut max_seq_in_batch = last_seq_sent;
        for packet in packets_to_send.iter().take(sent_packets) {
            if packet.seq > max_seq_in_batch {
                max_seq_in_batch = packet.seq;
            }
        }
        max_seq_in_batch
    }
}
