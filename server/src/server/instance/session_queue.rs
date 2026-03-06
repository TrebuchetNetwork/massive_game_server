use super::*;

impl MassiveGameServer {
    /// Record that a human player has entered the queue (for quick match
    /// bot-fill delay tracking).
    pub fn note_human_queue_arrival(&self) {
        let mut guard = self.queue_state.quick_match_queue_start.write();
        if guard.is_none() {
            *guard = Some(Instant::now());
        }
    }

    /// Returns `true` when the quick-match bot-fill delay has elapsed and the
    /// lobby has fewer than the minimum required human players.
    pub fn should_quick_match_bot_fill(&self) -> bool {
        if self.match_type != MatchType::QuickMatch {
            return false;
        }
        let Some(delay_secs) = self.match_type.bot_fill_delay_secs() else {
            return false;
        };
        let Some(min_humans) = self.match_type.min_humans_for_bot_fill() else {
            return false;
        };
        let guard = self.queue_state.quick_match_queue_start.read();
        let Some(queue_start) = *guard else {
            return false;
        };
        let elapsed = queue_start.elapsed().as_secs_f32();
        if elapsed < delay_secs {
            return false;
        }
        let mut human_count = 0usize;
        self.player_manager
            .for_each_player(|player_id, player_state| {
                if !self.bot_players.contains_key(player_id) && !player_state.is_spectator {
                    human_count += 1;
                }
            });
        human_count < min_humans
    }

    pub(super) fn enqueue_direct_packet_for_peer(&self, peer_id: &str, packet: Bytes) {
        let mut queue = self
            .queue_state
            .direct_packets
            .entry(peer_id.to_owned())
            .or_default();
        while queue.len() >= self.queue_state.direct_packet_queue_cap {
            let _ = queue.pop_front();
        }
        queue.push_back(packet);
    }

    pub(super) fn drain_direct_packets_for_peer(
        &self,
        peer_id: &str,
        max_packets: usize,
    ) -> Vec<Bytes> {
        if max_packets == 0 {
            return Vec::new();
        }
        let mut drained = Vec::new();
        if let Some(mut queue_entry) = self.queue_state.direct_packets.get_mut(peer_id) {
            for _ in 0..max_packets {
                let Some(packet) = queue_entry.pop_front() else {
                    break;
                };
                drained.push(packet);
            }
            if queue_entry.is_empty() {
                drop(queue_entry);
                self.queue_state.direct_packets.remove(peer_id);
            }
        }
        drained
    }

    pub(super) fn enqueue_direct_packet_for_all_players(&self, packet: Bytes) {
        let mut peers = std::collections::HashSet::new();
        for entry in self.data_channels_map.iter() {
            peers.insert(entry.key().clone());
        }
        for peer_id in connected_quic_peer_ids() {
            peers.insert(peer_id);
        }
        for peer_id in peers {
            self.enqueue_direct_packet_for_peer(&peer_id, packet.clone());
        }
    }

    pub fn remove_quic_player(&self, peer_id: &str) {
        self.player_manager.remove_player(peer_id);
        self.client_states_map.write().remove(peer_id);
        self.player_aois.remove(peer_id);
        self.data_channels_map.remove(peer_id);
        self.queue_state.direct_packets.remove(peer_id);
        self.cleanup_player_tracking_state(peer_id);
    }

    /// Remove per-player tracking data (position history, aim anomaly state, etc.)
    /// to prevent unbounded memory growth after a player disconnects.
    /// Called from both QUIC and WebRTC disconnect paths.
    pub fn cleanup_player_tracking_state(&self, peer_id: &str) {
        let player_id: PlayerID = Arc::from(peer_id.to_owned());
        self.runtime_tracking
            .player_position_history
            .remove(&player_id);
        self.runtime_tracking.aim_anomaly_states.remove(&player_id);
        self.snapshots.player_last_sync_positions.remove(&player_id);
    }
}
