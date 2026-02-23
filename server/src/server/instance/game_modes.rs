use super::*;

impl MassiveGameServer {
    pub(super) fn update_match_state_authoritative(&self, delta_time: f32) {
        let mut match_info_guard = self.match_info.write();
        let player_count = self.player_manager.player_count();
        let connected_client_count = self
            .data_channels_map
            .len()
            .saturating_add(connected_quic_peer_count());
        let effective_participant_count = player_count.max(connected_client_count);
        let dynamic_mode_transitions = env_bool_value("MGS_DYNAMIC_MODE_TRANSITIONS");

        match match_info_guard.match_state {
            fb::MatchStateType::Waiting => {
                if effective_participant_count >= MIN_PLAYERS_TO_START {
                    match_info_guard.match_state = fb::MatchStateType::Active;
                    match_info_guard.time_remaining = 300.0;
                    if dynamic_mode_transitions {
                        match_info_guard.game_mode = fb::GameModeType::FreeForAll;
                    }
                    info!(
                        "Match starting! Mode: {:?}, players={}, connected_clients={}, effective_participants={}",
                        match_info_guard.game_mode,
                        player_count,
                        connected_client_count,
                        effective_participant_count
                    );
                    if match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag {
                        self.initialize_ctf_flags(&mut match_info_guard);
                    }
                    self.player_manager.for_each_player_mut(|_id, p_state| {
                        p_state.score = 0;
                        p_state.kills = 0;
                        p_state.deaths = 0;
                        p_state.reset_match_stats();
                        p_state.is_carrying_flag_team_id = 0;
                        p_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
                    });
                    self.kill_feed.write().clear();
                }
            }
            fb::MatchStateType::Active => {
                match_info_guard.time_remaining -= delta_time;
                if dynamic_mode_transitions {
                    let elapsed = (300.0 - match_info_guard.time_remaining).max(0.0);
                    if elapsed >= 120.0
                        && match_info_guard.time_remaining > 70.0
                        && match_info_guard.game_mode == fb::GameModeType::FreeForAll
                    {
                        match_info_guard.game_mode = fb::GameModeType::TeamDeathmatch;
                        info!("Dynamic mode transition: FreeForAll -> TeamDeathmatch");
                    } else if match_info_guard.time_remaining <= 70.0
                        && match_info_guard.game_mode != fb::GameModeType::CaptureTheFlag
                    {
                        match_info_guard.game_mode = fb::GameModeType::CaptureTheFlag;
                        self.initialize_ctf_flags(&mut match_info_guard);
                        info!("Dynamic mode transition: TeamDeathmatch -> CaptureTheFlag");
                    }
                }
                if match_info_guard.time_remaining <= 0.0 {
                    match_info_guard.match_state = fb::MatchStateType::Ended;
                    info!("Match ended! (Time up)");
                    if match_info_guard.game_mode == fb::GameModeType::TeamDeathmatch
                        || match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag
                    {
                        let team1_score =
                            match_info_guard.team_scores.get(&1).cloned().unwrap_or(0);
                        let team2_score =
                            match_info_guard.team_scores.get(&2).cloned().unwrap_or(0);

                        if team1_score > team2_score {
                            info!(
                                "Team 1 wins with {} points vs Team 2's {} points!",
                                team1_score, team2_score
                            );
                        } else if team2_score > team1_score {
                            info!(
                                "Team 2 wins with {} points vs Team 1's {} points!",
                                team2_score, team1_score
                            );
                        } else if team1_score == team2_score && team1_score > 0 {
                            info!(
                                "Match ended in a draw! Both teams scored {} points.",
                                team1_score
                            );
                        } else {
                            info!("Match ended with no winner (0-0).");
                        }
                    }
                    drop(match_info_guard);
                    self.capture_match_end_summary("time_expired");
                    return;
                }
            }
            fb::MatchStateType::Ended => {
                match_info_guard.time_remaining -= delta_time;
                if match_info_guard.time_remaining <= -10.0 {
                    match_info_guard.match_state = fb::MatchStateType::Waiting;
                    self.reset_match_state(&mut match_info_guard);
                    info!("Match reset to Waiting.");
                }
            }
            _ => {}
        }
    }

    pub(super) fn process_ctf_logic_authoritative(&self, delta_time: f32) {
        let mut match_info_write_guard = self.match_info.write();
        if match_info_write_guard.game_mode == fb::GameModeType::CaptureTheFlag
            && match_info_write_guard.match_state == fb::MatchStateType::Active
        {
            for flag_state in match_info_write_guard.flag_states.values_mut() {
                if flag_state.status == fb::FlagStatus::Dropped && flag_state.respawn_timer > 0.0 {
                    flag_state.respawn_timer -= delta_time;
                    if flag_state.respawn_timer <= 0.0 {
                        flag_state.respawn_timer = 0.0;
                        flag_state.status = fb::FlagStatus::AtBase;
                        flag_state.position = Self::get_flag_base_position(flag_state.team_id);
                        flag_state.carrier_id = None;
                        self.global_game_events.push(
                            GameEvent::FlagReturned {
                                player_id: Arc::new("server".to_string()),
                                flag_team_id: flag_state.team_id,
                                position: flag_state.position,
                            },
                            EventPriority::High,
                        );
                        info!("Flag of team {} auto-returned to base.", flag_state.team_id);
                    }
                }
            }

            let mut player_snapshots: HashMap<PlayerID, PlayerState> = HashMap::new();
            self.player_manager.for_each_player(|id, state| {
                player_snapshots.insert(id.clone(), state.clone());
            });

            for (player_id_arc, player_state_snapshot) in &player_snapshots {
                if !player_state_snapshot.alive {
                    continue;
                }

                if player_state_snapshot.is_carrying_flag_team_id == 0 {
                    for flag_state in match_info_write_guard.flag_states.values_mut() {
                        let can_interact = match flag_state.status {
                            fb::FlagStatus::AtBase => true,
                            fb::FlagStatus::Dropped => {
                                if flag_state.team_id == player_state_snapshot.team_id {
                                    true
                                } else {
                                    flag_state.respawn_timer <= 0.0
                                }
                            }
                            _ => false,
                        };

                        if can_interact {
                            let dx = player_state_snapshot.x - flag_state.position.x;
                            let dy = player_state_snapshot.y - flag_state.position.y;
                            if (dx * dx + dy * dy)
                                < (PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS)
                            {
                                if flag_state.team_id != player_state_snapshot.team_id {
                                    flag_state.status = fb::FlagStatus::Carried;
                                    flag_state.carrier_id = Some(player_id_arc.clone());
                                    if let Some(mut p_state_mut_entry) =
                                        self.player_manager.get_player_state_mut(player_id_arc)
                                    {
                                        let p_state_mut = &mut *p_state_mut_entry;
                                        p_state_mut.is_carrying_flag_team_id = flag_state.team_id;
                                        p_state_mut.mark_field_changed(FIELD_FLAG);
                                    }
                                    self.global_game_events.push(
                                        GameEvent::FlagGrabbed {
                                            player_id: player_id_arc.clone(),
                                            flag_team_id: flag_state.team_id,
                                            position: flag_state.position,
                                        },
                                        EventPriority::High,
                                    );
                                    info!(
                                        "Player {} grabbed flag of team {}",
                                        player_state_snapshot.username, flag_state.team_id
                                    );
                                    break;
                                } else if flag_state.status == fb::FlagStatus::Dropped
                                    && flag_state.team_id == player_state_snapshot.team_id
                                {
                                    flag_state.status = fb::FlagStatus::AtBase;
                                    flag_state.position =
                                        Self::get_flag_base_position(flag_state.team_id);
                                    flag_state.carrier_id = None;
                                    flag_state.respawn_timer = 0.0;
                                    if let Some(mut p_state_mut_entry) =
                                        self.player_manager.get_player_state_mut(player_id_arc)
                                    {
                                        let p_state_mut = &mut *p_state_mut_entry;
                                        p_state_mut.flag_returns =
                                            p_state_mut.flag_returns.saturating_add(1);
                                        p_state_mut.mark_field_changed(FIELD_SCORE_STATS);
                                    }
                                    self.global_game_events.push(
                                        GameEvent::FlagReturned {
                                            player_id: player_id_arc.clone(),
                                            flag_team_id: flag_state.team_id,
                                            position: flag_state.position,
                                        },
                                        EventPriority::High,
                                    );
                                    info!(
                                        "Player {} returned own team {}'s flag.",
                                        player_state_snapshot.username, flag_state.team_id
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }

                if player_state_snapshot.is_carrying_flag_team_id != 0
                    && player_state_snapshot.is_carrying_flag_team_id
                        != player_state_snapshot.team_id
                {
                    let own_player_team_id = player_state_snapshot.team_id;

                    let own_flag_at_base = match_info_write_guard
                        .flag_states
                        .get(&own_player_team_id)
                        .map_or(false, |ofs| ofs.status == fb::FlagStatus::AtBase);

                    if own_flag_at_base {
                        let own_flag_base_pos = Self::get_flag_base_position(own_player_team_id);
                        let dx = player_state_snapshot.x - own_flag_base_pos.x;
                        let dy = player_state_snapshot.y - own_flag_base_pos.y;

                        if (dx * dx + dy * dy)
                            < (PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS)
                        {
                            let captured_flag_team_id =
                                player_state_snapshot.is_carrying_flag_team_id;

                            if let Some(captured_flag) = match_info_write_guard
                                .flag_states
                                .get_mut(&captured_flag_team_id)
                            {
                                captured_flag.status = fb::FlagStatus::AtBase;
                                captured_flag.position =
                                    Self::get_flag_base_position(captured_flag_team_id);
                                captured_flag.carrier_id = None;
                            }

                            if let Some(mut p_state_mut_entry) =
                                self.player_manager.get_player_state_mut(player_id_arc)
                            {
                                let p_state_mut = &mut *p_state_mut_entry;
                                p_state_mut.is_carrying_flag_team_id = 0;
                                p_state_mut.mark_field_changed(FIELD_FLAG);
                                p_state_mut.score += 100;
                                p_state_mut.flag_captures =
                                    p_state_mut.flag_captures.saturating_add(1);
                                p_state_mut.mark_field_changed(FIELD_SCORE_STATS);
                            }

                            let team_score_mut_ref = match_info_write_guard
                                .team_scores
                                .entry(own_player_team_id)
                                .or_insert(0);
                            *team_score_mut_ref += 1;
                            let current_score = *team_score_mut_ref;

                            self.global_game_events.push(
                                GameEvent::FlagCaptured {
                                    capturer_id: player_id_arc.clone(),
                                    captured_flag_team_id,
                                    capturing_team_id: own_player_team_id,
                                    position: own_flag_base_pos,
                                },
                                EventPriority::High,
                            );
                            info!(
                                "Player {} captured team {}'s flag for team {}! (Score: {})",
                                player_state_snapshot.username,
                                captured_flag_team_id,
                                own_player_team_id,
                                current_score
                            );

                            if current_score >= 3 {
                                match_info_write_guard.match_state = fb::MatchStateType::Ended;
                                info!(
                                    "Team {} wins by capturing {} flags!",
                                    own_player_team_id, current_score
                                );
                                drop(match_info_write_guard);
                                self.capture_match_end_summary("ctf_score_limit");
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    fn initialize_ctf_flags(&self, match_info: &mut ServerMatchInfo) {
        match_info.flag_states.clear();
        let team1_flag_pos = Self::get_flag_base_position(1);
        match_info.flag_states.insert(
            1,
            ServerFlagState {
                team_id: 1,
                status: fb::FlagStatus::AtBase,
                position: team1_flag_pos,
                carrier_id: None,
                respawn_timer: 0.0,
            },
        );
        let team2_flag_pos = Self::get_flag_base_position(2);
        match_info.flag_states.insert(
            2,
            ServerFlagState {
                team_id: 2,
                status: fb::FlagStatus::AtBase,
                position: team2_flag_pos,
                carrier_id: None,
                respawn_timer: 0.0,
            },
        );
        info!(
            "CTF Flags initialized. T1 at {:?}, T2 at {:?}",
            team1_flag_pos, team2_flag_pos
        );
    }

    pub fn get_flag_base_position(team_id: u8) -> Vec2 {
        if team_id == 1 {
            Vec2::new(WORLD_MIN_X + 100.0, 0.0)
        } else if team_id == 2 {
            Vec2::new(WORLD_MAX_X - 100.0, 0.0)
        } else {
            Vec2::new(0.0, 0.0)
        }
    }

    fn reset_match_state(&self, match_info: &mut ServerMatchInfo) {
        match_info.time_remaining = 300.0;
        // Don't clear team scores - preserve them between rounds
        // match_info.team_scores.clear();
        match_info.flag_states.clear();
        if match_info.match_state == fb::MatchStateType::Waiting
            && match_info.game_mode == fb::GameModeType::CaptureTheFlag
        {
            self.initialize_ctf_flags(match_info);
        }
        self.player_manager.for_each_player_mut(|_id, pstate| {
            // Reset individual player stats but keep their contribution to team score
            pstate.score = 0;
            pstate.kills = 0;
            pstate.deaths = 0;
            pstate.reset_match_stats();
            pstate.is_carrying_flag_team_id = 0;
            pstate.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
        });
        self.kill_feed.write().clear();
    }
}
