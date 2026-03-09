use super::*;

impl MassiveGameServer {
    /// Returns `true` if a critical task has panicked during a game tick,
    /// indicating the match state may be corrupted.
    pub fn is_match_degraded(&self) -> bool {
        self.runtime_tracking
            .match_degraded
            .load(AtomicOrdering::Acquire)
    }

    pub fn current_quality_settings(&self) -> QualitySettings {
        *self.runtime_tracking.dynamic_quality_settings.read()
    }

    pub fn record_tick_metrics(&self, frame_duration: Duration) {
        {
            let mut history = self.tick_durations_history.write();
            history.push_back(frame_duration);
            while history.len() > 1000 {
                let _ = history.pop_front();
            }
        }

        let connected_players = self
            .data_channels_map
            .len()
            .saturating_add(connected_quic_peer_count());
        metrics::record_frame_metrics(frame_duration.as_secs_f64(), connected_players);
        metrics::set_match_degraded(self.is_match_degraded());
        let mut tuner = self.runtime_tracking.auto_tuner.write();
        let quality = tuner.ingest_sample(TuningSample {
            frame_time_ms: frame_duration.as_secs_f32() * 1000.0,
            connected_players,
        });
        *self.runtime_tracking.dynamic_quality_settings.write() = quality;
    }
}
