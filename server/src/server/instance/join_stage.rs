use super::*;

impl MassiveGameServer {
    pub(super) fn ensure_join_trace(&self, peer_id: &str, channel_open: bool) {
        let now_ms = self.get_server_timestamp_us();
        let mut trace = self
            .runtime_tracking
            .join_stage_traces
            .entry(peer_id.to_owned())
            .or_insert_with(|| JoinStageTrace {
                join_sequence: self
                    .runtime_tracking
                    .join_sequence_counter
                    .fetch_add(1, AtomicOrdering::Relaxed)
                    + 1,
                first_seen_ms: now_ms,
                ..JoinStageTrace::default()
            });
        if channel_open && trace.first_channel_open_ms.is_none() {
            trace.first_channel_open_ms = Some(now_ms);
        }
    }

    pub fn note_join_enqueued(&self, peer_id: &str) {
        self.ensure_join_trace(peer_id, false);
    }

    pub fn note_join_channel_open(&self, peer_id: &str) {
        self.ensure_join_trace(peer_id, true);
    }

    pub(super) fn mark_join_build_start(&self, peer_id: &str) {
        self.ensure_join_trace(peer_id, false);
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.runtime_tracking.join_stage_traces.get_mut(peer_id) {
            if trace.first_build_start_ms.is_none() {
                trace.first_build_start_ms = Some(now_ms);
            }
            trace.build_attempts = trace.build_attempts.saturating_add(1);
        }
    }

    pub(super) fn mark_join_build_done(&self, peer_id: &str) {
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.runtime_tracking.join_stage_traces.get_mut(peer_id) {
            if trace.first_build_done_ms.is_none() {
                trace.first_build_done_ms = Some(now_ms);
            }
        }
    }

    pub(super) fn mark_join_send_start(&self, peer_id: &str) {
        self.ensure_join_trace(peer_id, true);
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.runtime_tracking.join_stage_traces.get_mut(peer_id) {
            if trace.first_send_start_ms.is_none() {
                trace.first_send_start_ms = Some(now_ms);
            }
            trace.send_attempts = trace.send_attempts.saturating_add(1);
        }
    }

    pub(super) fn mark_join_send_failure(&self, peer_id: &str) {
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.runtime_tracking.join_stage_traces.get_mut(peer_id) {
            if trace.first_send_failure_ms.is_none() {
                trace.first_send_failure_ms = Some(now_ms);
            }
            if trace.first_send_result_ms.is_none() {
                trace.first_send_result_ms = Some(now_ms);
            }
            trace.retry_count = trace.retry_count.saturating_add(1);
            if let Some(previous_retry_ms) = trace.last_retry_at_ms {
                if now_ms > previous_retry_ms {
                    trace.retry_interval_total_ms = trace
                        .retry_interval_total_ms
                        .saturating_add(now_ms.saturating_sub(previous_retry_ms));
                    trace.retry_interval_samples = trace.retry_interval_samples.saturating_add(1);
                }
            }
            trace.last_retry_at_ms = Some(now_ms);
        }
    }

    pub(super) fn mark_join_send_done(&self, peer_id: &str) {
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.runtime_tracking.join_stage_traces.get_mut(peer_id) {
            if trace.first_send_done_ms.is_none() {
                trace.first_send_done_ms = Some(now_ms);
            }
            if trace.first_send_result_ms.is_none() {
                trace.first_send_result_ms = Some(now_ms);
            }
            if trace.completed_ms.is_none() {
                trace.completed_ms = Some(now_ms);
                // Record total join latency from first seen to completion (microseconds → seconds).
                let join_latency_us = now_ms.saturating_sub(trace.first_seen_ms);
                metrics::record_player_join_latency(join_latency_us as f64 / 1_000_000.0);
            }
        }
    }

    pub fn reset_join_stage_report(&self) {
        self.runtime_tracking.join_stage_traces.clear();
        self.runtime_tracking
            .join_sequence_counter
            .store(0, AtomicOrdering::Relaxed);
    }

    pub fn join_stage_report(&self) -> JoinStageReport {
        let traces: Vec<JoinStageTrace> = self
            .runtime_tracking
            .join_stage_traces
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        let total_tracked_clients = traces.len();
        let total_completed_clients = traces
            .iter()
            .filter(|trace| trace.completed_ms.is_some())
            .count();

        let mut waves = HashMap::new();
        for (wave_key, wave_label, wave_start, wave_end) in JOIN_STAGE_WAVES {
            let wave_traces: Vec<&JoinStageTrace> = traces
                .iter()
                .filter(|trace| trace.join_sequence >= wave_start)
                .filter(|trace| wave_end.is_none_or(|end| trace.join_sequence <= end))
                .collect();

            let open_channel_wait_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(
                    |trace| match (trace.first_seen_ms, trace.first_channel_open_ms) {
                        (seen, Some(opened)) if opened >= seen => {
                            Some(opened.saturating_sub(seen) as f64 / 1000.0)
                        }
                        _ => None,
                    },
                )
                .collect();
            let queue_wait_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(|trace| {
                    trace.first_build_start_ms.map(|build_start| {
                        let queue_start =
                            trace.first_channel_open_ms.unwrap_or(trace.first_seen_ms);
                        build_start.saturating_sub(queue_start) as f64 / 1000.0
                    })
                })
                .collect();
            let snapshot_build_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(
                    |trace| match (trace.first_build_start_ms, trace.first_build_done_ms) {
                        (Some(build_start), Some(build_done)) if build_done >= build_start => {
                            Some(build_done.saturating_sub(build_start) as f64 / 1000.0)
                        }
                        _ => None,
                    },
                )
                .collect();
            let send_ack_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(
                    |trace| match (trace.first_send_start_ms, trace.completed_ms) {
                        (Some(send_start), Some(completed)) if completed >= send_start => {
                            Some(completed.saturating_sub(send_start) as f64 / 1000.0)
                        }
                        _ => None,
                    },
                )
                .collect();
            let send_result_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(
                    |trace| match (trace.first_send_start_ms, trace.first_send_result_ms) {
                        (Some(send_start), Some(result_ms)) if result_ms >= send_start => {
                            Some(result_ms.saturating_sub(send_start) as f64 / 1000.0)
                        }
                        _ => None,
                    },
                )
                .collect();
            let retry_interval_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(|trace| {
                    if trace.retry_interval_samples == 0 {
                        None
                    } else {
                        Some(
                            trace.retry_interval_total_ms as f64
                                / trace.retry_interval_samples as f64
                                / 1000.0,
                        )
                    }
                })
                .collect();

            let retry_count_avg = if wave_traces.is_empty() {
                0.0
            } else {
                wave_traces
                    .iter()
                    .map(|trace| trace.retry_count as f64)
                    .sum::<f64>()
                    / wave_traces.len() as f64
            };
            let build_attempts_avg = if wave_traces.is_empty() {
                0.0
            } else {
                wave_traces
                    .iter()
                    .map(|trace| trace.build_attempts as f64)
                    .sum::<f64>()
                    / wave_traces.len() as f64
            };
            let send_attempts_avg = if wave_traces.is_empty() {
                0.0
            } else {
                wave_traces
                    .iter()
                    .map(|trace| trace.send_attempts as f64)
                    .sum::<f64>()
                    / wave_traces.len() as f64
            };

            let requested_slots = if let Some(end) = wave_end {
                end.saturating_sub(wave_start).saturating_add(1)
            } else {
                total_tracked_clients.max((wave_start.saturating_sub(1)) as usize) as u64
                    - wave_start.saturating_sub(1)
            };

            waves.insert(
                wave_key.to_owned(),
                JoinStageWaveSummary {
                    label: wave_label.to_owned(),
                    start_sequence: wave_start,
                    end_sequence: wave_end,
                    requested_slots,
                    tracked_clients: wave_traces.len(),
                    completed_clients: wave_traces
                        .iter()
                        .filter(|trace| trace.completed_ms.is_some())
                        .count(),
                    open_channel_wait_ms: summarize_join_stage_latencies(&open_channel_wait_ms),
                    queue_wait_ms: summarize_join_stage_latencies(&queue_wait_ms),
                    snapshot_build_ms: summarize_join_stage_latencies(&snapshot_build_ms),
                    send_result_ms: summarize_join_stage_latencies(&send_result_ms),
                    send_ack_ms: summarize_join_stage_latencies(&send_ack_ms),
                    retry_interval_ms: summarize_join_stage_latencies(&retry_interval_ms),
                    retry_count_avg: round_metric(retry_count_avg),
                    build_attempts_avg: round_metric(build_attempts_avg),
                    send_attempts_avg: round_metric(send_attempts_avg),
                },
            );
        }

        JoinStageReport {
            generated_at_ms: self.get_server_timestamp_ms(),
            total_tracked_clients,
            total_completed_clients,
            waves,
        }
    }
}
