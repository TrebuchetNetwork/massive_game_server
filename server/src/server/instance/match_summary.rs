use super::*;

impl MassiveGameServer {
    pub fn latest_match_end_summary(&self) -> Option<MatchEndSummary> {
        self.latest_match_end_summary.read().clone()
    }

    pub fn latest_killcam_for_player(&self, player_id: &str) -> Option<KillCamData> {
        self.recent_killcams
            .iter()
            .find(|entry| entry.key().as_str() == player_id)
            .map(|entry| entry.value().clone())
    }

    pub(super) fn capture_match_end_summary(&self, reason: &str) {
        let (game_mode, time_remaining, team_scores) = {
            let match_info = self.match_info.read();
            (
                match_info.game_mode,
                match_info.time_remaining,
                match_info.team_scores.clone(),
            )
        };

        let mut players = Vec::new();
        self.player_manager
            .for_each_player(|player_id, player_state| {
                if player_state.is_spectator {
                    return;
                }
                let kd_ratio = if player_state.deaths <= 0 {
                    player_state.kills as f32
                } else {
                    player_state.kills as f32 / player_state.deaths as f32
                };
                players.push(PlayerMatchStats {
                    player_id: player_id.as_str().to_string(),
                    player_name: player_state.username.clone(),
                    team_id: player_state.team_id,
                    kills: player_state.kills,
                    deaths: player_state.deaths,
                    score: player_state.score,
                    damage_dealt: player_state.damage_dealt,
                    damage_taken: player_state.damage_taken,
                    flag_captures: player_state.flag_captures,
                    flag_returns: player_state.flag_returns,
                    weapon_kills: player_state.kills_per_weapon.to_vec(),
                    kd_ratio,
                });
            });
        players.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.kills.cmp(&left.kills))
                .then_with(|| left.player_name.cmp(&right.player_name))
        });

        let mvp_kills = players
            .iter()
            .max_by_key(|stats| stats.kills)
            .filter(|stats| stats.kills > 0)
            .map(|stats| stats.player_name.clone());
        let mvp_damage = players
            .iter()
            .max_by_key(|stats| stats.damage_dealt)
            .filter(|stats| stats.damage_dealt > 0)
            .map(|stats| stats.player_name.clone());
        let mvp_objectives = players
            .iter()
            .max_by_key(|stats| stats.flag_captures + stats.flag_returns)
            .filter(|stats| stats.flag_captures + stats.flag_returns > 0)
            .map(|stats| stats.player_name.clone());

        let winning_team = match game_mode {
            fb::GameModeType::TeamDeathmatch | fb::GameModeType::CaptureTheFlag => team_scores
                .iter()
                .max_by_key(|(_, score)| *score)
                .map(|(team_id, _)| *team_id)
                .unwrap_or(0),
            _ => 0,
        };

        let summary = MatchEndSummary {
            generated_at_ms: self.get_server_timestamp_ms(),
            reason: reason.to_string(),
            map_name: self.map_name.clone(),
            game_mode: format!("{:?}", game_mode),
            match_duration: (300.0 - time_remaining).clamp(0.0, 300.0),
            winning_team,
            players,
            mvp_kills,
            mvp_damage,
            mvp_objectives,
        };

        *self.latest_match_end_summary.write() = Some(summary);
        let latest_summary = self.latest_match_end_summary();
        if let Some(summary_event_packet) =
            self.build_system_event_packet("match_summary", latest_summary.as_ref())
        {
            self.enqueue_direct_packet_for_all_players(summary_event_packet);
        }
        self.persist_match_replay_snapshot(reason);
    }

    pub(super) fn capture_killcam_for_victim(
        &self,
        victim_id: &PlayerID,
        victim_name: &str,
        killer_id: &PlayerID,
        weapon: ServerWeaponType,
    ) {
        let killer_name = self
            .player_manager
            .get_player_state(killer_id)
            .map(|state| state.username.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        let killer_rotation = self
            .player_manager
            .get_player_state(killer_id)
            .map(|state| state.rotation)
            .unwrap_or(0.0);

        let Some(history) = self.player_position_history.get(killer_id) else {
            return;
        };

        let raw_samples = history.recent_samples(30);
        if raw_samples.is_empty() {
            return;
        }

        let mut previous = None::<Vec2>;
        let mut samples = Vec::with_capacity(raw_samples.len());
        for sample in raw_samples {
            let rotation = if let Some(prev) = previous {
                let dx = sample.value.x - prev.x;
                let dy = sample.value.y - prev.y;
                if dx.abs() > 0.01 || dy.abs() > 0.01 {
                    dy.atan2(dx)
                } else {
                    killer_rotation
                }
            } else {
                killer_rotation
            };
            previous = Some(sample.value);
            samples.push(KillCamSample {
                x: sample.value.x,
                y: sample.value.y,
                rotation,
                shooting: false,
                timestamp_ms: sample.timestamp_ms,
            });
        }

        if let Some(last) = samples.last_mut() {
            last.shooting = true;
        }

        self.recent_killcams.insert(
            victim_id.clone(),
            KillCamData {
                victim_id: victim_id.as_str().to_string(),
                victim_name: victim_name.to_string(),
                killer_id: killer_id.as_str().to_string(),
                killer_name,
                weapon: format!("{:?}", weapon),
                generated_at_ms: self.get_server_timestamp_ms(),
                samples,
            },
        );
        if let Some(killcam) = self.latest_killcam_for_player(victim_id.as_str()) {
            if let Some(killcam_event_packet) =
                self.build_system_event_packet("killcam", Some(&killcam))
            {
                self.enqueue_direct_packet_for_peer(victim_id.as_str(), killcam_event_packet);
            }
        }
    }

    pub(super) fn prune_match_runtime_state(&self) {
        self.recent_killcams
            .retain(|player_id, _| self.player_manager.get_player_state(player_id).is_some());
    }

    fn build_system_event_packet<T: Serialize>(
        &self,
        event: &str,
        payload: Option<&T>,
    ) -> Option<Bytes> {
        let message = serde_json::json!({
            "event": event,
            "payload": payload,
            "timestamp_ms": self.get_server_timestamp_ms(),
        })
        .to_string();
        let chat_entry = ChatMessage {
            seq: next_chat_message_seq(),
            player_id: self.player_manager.id_pool.get_or_create("system"),
            username: "System".to_owned(),
            message,
            timestamp: self.get_server_timestamp_ms(),
        };
        Some(build_chat_game_message_bytes(&chat_entry))
    }
}
