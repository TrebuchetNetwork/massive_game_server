use super::*;

impl MassiveGameServer {
    pub fn recent_live_replay_frames(&self, limit: usize) -> Vec<LiveReplayFrame> {
        let replay = self.live_replay_frames.read();
        let bounded = limit.clamp(1, self.live_replay_capacity.max(1));
        replay.iter().rev().take(bounded).cloned().collect()
    }

    pub fn recent_live_replay_dispute_audits(
        &self,
        limit: usize,
    ) -> Vec<LiveReplayDisputeAuditProof> {
        let audits = self.live_replay_dispute_audits.read();
        let bounded = limit.clamp(1, self.live_replay_dispute_audit_capacity.max(1));
        audits.iter().rev().take(bounded).cloned().collect()
    }

    pub fn build_live_replay_dispute_report(
        &self,
        request: LiveReplayDisputeRequest,
    ) -> LiveReplayDisputeReport {
        let replay = self.live_replay_frames.read();
        let limit = request
            .limit
            .unwrap_or(256)
            .clamp(1, self.live_replay_capacity.max(1));
        let player_filter = request
            .player_id
            .as_ref()
            .map(|raw| raw.trim())
            .filter(|raw| !raw.is_empty());

        let mut selected_frames: Vec<LiveReplayFrame> = replay
            .iter()
            .filter(|frame| request.from_frame.map_or(true, |from| frame.frame >= from))
            .filter(|frame| request.to_frame.map_or(true, |to| frame.frame <= to))
            .filter(|frame| {
                player_filter.map_or(true, |player_id| {
                    frame
                        .sampled_players
                        .iter()
                        .any(|sample| sample.player_id == player_id)
                })
            })
            .cloned()
            .collect();

        if selected_frames.len() > limit {
            let keep_from = selected_frames.len().saturating_sub(limit);
            selected_frames.drain(0..keep_from);
        }

        let effective_from = request
            .from_frame
            .or_else(|| selected_frames.first().map(|frame| frame.frame));
        let effective_to = request
            .to_frame
            .or_else(|| selected_frames.last().map(|frame| frame.frame));

        let relevant_kill_feed: Vec<LiveReplayKillFeedEntry> = self
            .kill_feed
            .read()
            .iter()
            .filter(|entry| effective_from.map_or(true, |from| entry.timestamp >= from))
            .filter(|entry| effective_to.map_or(true, |to| entry.timestamp <= to))
            .map(|entry| LiveReplayKillFeedEntry {
                killer_name: entry.killer_name.clone(),
                victim_name: entry.victim_name.clone(),
                weapon: format!("{:?}", entry.weapon),
                timestamp: entry.timestamp,
            })
            .collect();

        let mut report = LiveReplayDisputeReport {
            generated_at_ms: self.get_server_timestamp_ms(),
            total_captured_frames: replay.len(),
            selected_frames,
            relevant_kill_feed,
            filter: LiveReplayDisputeFilter {
                from_frame: effective_from,
                to_frame: effective_to,
                player_id: player_filter.map(str::to_owned),
            },
            audit: None,
        };
        drop(replay);

        report.audit = Some(self.persist_live_replay_dispute_report(&report));
        report
    }

    pub(super) fn capture_live_replay_frame(&self, frame: u64) {
        if !self.live_replay_enabled {
            return;
        }

        let mut sampled_players = Vec::with_capacity(self.live_replay_player_cap);
        self.player_manager
            .for_each_player(|player_id, player_state| {
                if sampled_players.len() >= self.live_replay_player_cap {
                    return;
                }
                sampled_players.push(LiveReplayPlayerSample {
                    player_id: player_id.as_str().to_string(),
                    username: player_state.username.clone(),
                    x: player_state.x,
                    y: player_state.y,
                    velocity_x: player_state.velocity_x,
                    velocity_y: player_state.velocity_y,
                    health: player_state.health,
                    alive: player_state.alive,
                    team_id: player_state.team_id,
                });
            });

        let frame_sample = LiveReplayFrame {
            frame,
            timestamp_ms: self.get_server_timestamp_ms(),
            players: self.player_manager.player_count(),
            projectiles: self.projectiles.read().len(),
            pickups: self.pickups.read().len(),
            events: self.global_game_events.len(),
            sampled_players,
            kill_feed_size: self.kill_feed.read().len(),
        };

        let mut replay = self.live_replay_frames.write();
        while replay.len() >= self.live_replay_capacity {
            let _ = replay.pop_front();
        }
        replay.push_back(frame_sample);
    }

    fn persist_live_replay_dispute_report(
        &self,
        report: &LiveReplayDisputeReport,
    ) -> LiveReplayDisputeAuditProof {
        let dispute_id = format!("dispute_{}", Uuid::new_v4());
        let payload = serde_json::to_vec(report).unwrap_or_default();
        let payload_sha256 = sha256_hex(&payload);
        let previous_chain_hash = self.live_replay_dispute_chain_head.read().clone();

        let mut chain_material = String::new();
        if let Some(previous) = previous_chain_hash.as_deref() {
            chain_material.push_str(previous);
        }
        chain_material.push(':');
        chain_material.push_str(&payload_sha256);
        chain_material.push(':');
        chain_material.push_str(&report.generated_at_ms.to_string());
        let chain_hash_sha256 = sha256_hex(chain_material.as_bytes());
        let signature_hmac_sha256 = self
            .live_replay_dispute_signing_key
            .as_ref()
            .and_then(|key| hmac_sha256_hex(key.as_slice(), chain_hash_sha256.as_bytes()));

        let mut audit = LiveReplayDisputeAuditProof {
            dispute_id: dispute_id.clone(),
            persisted: false,
            storage_path: if self.live_replay_dispute_persist_enabled {
                Some(self.live_replay_dispute_store_path.to_string_lossy().to_string())
            } else {
                None
            },
            payload_sha256: payload_sha256.clone(),
            chain_hash_sha256: chain_hash_sha256.clone(),
            chain_prev_hash_sha256: previous_chain_hash,
            signature_hmac_sha256: signature_hmac_sha256.clone(),
        };

        if self.live_replay_dispute_persist_enabled {
            let persisted_record = PersistedLiveReplayDisputeRecord {
                dispute_id: dispute_id.clone(),
                generated_at_ms: report.generated_at_ms,
                total_captured_frames: report.total_captured_frames,
                selected_frame_count: report.selected_frames.len(),
                selected_from_frame: report.selected_frames.first().map(|frame| frame.frame),
                selected_to_frame: report.selected_frames.last().map(|frame| frame.frame),
                kill_feed_event_count: report.relevant_kill_feed.len(),
                filter: report.filter.clone(),
                payload_sha256: payload_sha256.clone(),
                chain_hash_sha256: chain_hash_sha256.clone(),
                chain_prev_hash_sha256: audit.chain_prev_hash_sha256.clone(),
                signature_hmac_sha256: signature_hmac_sha256.clone(),
            };

            match append_dispute_record(
                self.live_replay_dispute_store_path.as_path(),
                &persisted_record,
            ) {
                Ok(()) => {
                    *self.live_replay_dispute_chain_head.write() = Some(chain_hash_sha256.clone());
                    audit.persisted = true;
                }
                Err(err) => {
                    warn!(
                        "failed to persist live replay dispute record {}: {}",
                        dispute_id, err
                    );
                }
            }
        }

        {
            let mut audits = self.live_replay_dispute_audits.write();
            while audits.len() >= self.live_replay_dispute_audit_capacity {
                let _ = audits.pop_front();
            }
            audits.push_back(audit.clone());
        }

        audit
    }
}
