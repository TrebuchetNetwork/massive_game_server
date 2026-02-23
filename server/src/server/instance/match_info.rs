use super::*;

impl MassiveGameServer {
    pub(super) fn get_server_timestamp_us(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }

    pub fn get_server_timestamp_ms(&self) -> u64 {
        self.get_server_timestamp_us() / 1000
    }

    pub(crate) fn build_match_info_only_bytes(&self) -> Bytes {
        let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(2048);
        let timestamp_ms = self.get_server_timestamp_ms();
        self.refresh_commander_runtime_state(timestamp_ms);
        let team1_commander_id = self
            .commander_id_for_team(1)
            .map(|commander_id| commander_id.as_str().to_owned());
        let team2_commander_id = self
            .commander_id_for_team(2)
            .map(|commander_id| commander_id.as_str().to_owned());
        let team1_commander_waypoint = self.commander_primary_waypoint_for_team(1);
        let team2_commander_waypoint = self.commander_primary_waypoint_for_team(2);
        let team1_commander_attack_bias = self.commander_attack_bias_for_team(1).unwrap_or(0.0);
        let team2_commander_attack_bias = self.commander_attack_bias_for_team(2).unwrap_or(0.0);

        let match_info_guard = self.match_info.read();
        let team_scores_vec: Vec<_> = match_info_guard
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
        let team1_commander_id_fb = team1_commander_id
            .as_deref()
            .map(|id| fb_safe_str(&mut builder, id));
        let team2_commander_id_fb = team2_commander_id
            .as_deref()
            .map(|id| fb_safe_str(&mut builder, id));
        let team1_commander_waypoint_fb = team1_commander_waypoint.map(|waypoint| {
            fb::Vec2::create(
                &mut builder,
                &fb::Vec2Args {
                    x: waypoint.x,
                    y: waypoint.y,
                },
            )
        });
        let team2_commander_waypoint_fb = team2_commander_waypoint.map(|waypoint| {
            fb::Vec2::create(
                &mut builder,
                &fb::Vec2Args {
                    x: waypoint.x,
                    y: waypoint.y,
                },
            )
        });
        let match_info_fb = fb::MatchInfo::create(
            &mut builder,
            &fb::MatchInfoArgs {
                time_remaining: match_info_guard.time_remaining,
                match_state: match_info_guard.match_state,
                winner_id: None,
                winner_name: None,
                game_mode: match_info_guard.game_mode,
                team_scores: Some(team_scores_fb),
                team1_commander_id: team1_commander_id_fb,
                team2_commander_id: team2_commander_id_fb,
                team1_commander_waypoint: team1_commander_waypoint_fb,
                team2_commander_waypoint: team2_commander_waypoint_fb,
                team1_commander_attack_bias,
                team2_commander_attack_bias,
            },
        );
        drop(match_info_guard);

        let delta_state_args = fb::DeltaStateMessageArgs {
            players: None,
            projectiles: None,
            removed_projectiles: None,
            pickups: None,
            deactivated_pickup_ids: None,
            game_events: None,
            timestamp: timestamp_ms,
            last_processed_input_sequence: 0,
            changed_player_fields: None,
            kill_feed: None,
            match_info: Some(match_info_fb),
            destroyed_wall_ids: None,
            flag_states: None,
            removed_player_ids: None,
            updated_walls: None,
        };
        let delta_state_msg = fb::DeltaStateMessage::create(&mut builder, &delta_state_args);
        let game_msg = fb::GameMessage::create(
            &mut builder,
            &fb::GameMessageArgs {
                msg_type: fb::MessageType::DeltaState,
                actual_message_type: fb::MessagePayload::DeltaStateMessage,
                actual_message: Some(delta_state_msg.as_union_value()),
                protocol_version: GAME_PROTOCOL_VERSION,
            },
        );
        builder.finish(game_msg, None);
        let (buffer, root_index) = builder.collapse();
        Bytes::from(buffer).slice(root_index..)
    }

    pub(crate) async fn send_match_info_only(
        &self,
        peer_id_str: &str,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
    ) {
        let data_bytes = self.build_match_info_only_bytes();
        let sent_packets = self
            .send_packet_batch_optimized(data_channel, &[data_bytes], 80)
            .await;
        if sent_packets == 0 {
            warn!("[{}] Failed to send match_info-only delta.", peer_id_str);
        } else {
            info!(
                "[{}] Sent match_info-only delta to unblock client.",
                peer_id_str
            );
        }
    }
}
