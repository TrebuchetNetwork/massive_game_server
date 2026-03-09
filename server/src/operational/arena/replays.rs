use super::types::{
    ArenaError, ArenaMatchReplayRecord, ArenaMatchReplayResponse, ArenaReplayEvent,
    ArenaReplayEventFeedResponse, ArenaReplayListResponse, ArenaReplayView, QueuedMatchView,
};
use super::ArenaService;
use crate::operational::bot_sandbox::{BotMatchOutcome, BotMatchReplay};
use futures_util::stream::{self, StreamExt};
use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use warp::Reply;

pub(super) const DEFAULT_ARENA_REPLAY_STREAM_BACKLOG: usize = 64;
pub(super) const MAX_ARENA_STREAM_BACKLOG: usize = 2_048;
pub(super) const MAX_ARENA_REPLAY_EVENTS_LIMIT: usize = 2_048;
pub(super) const MAX_ARENA_REPLAY_WARNINGS: usize = 24;

impl ArenaService {
    pub(super) fn recent_replays(&self, limit: usize) -> ArenaReplayListResponse {
        let bounded = limit.clamp(1, 200);
        let history = self.inner.recent_replays.lock();
        let replays = history.iter().rev().take(bounded).cloned().collect();
        ArenaReplayListResponse {
            generated_at: super::scoring::unix_now(),
            total_replays: history.len(),
            replays,
        }
    }

    pub(super) fn push_recent_replay(&self, replay: ArenaReplayView) {
        let mut history = self.inner.recent_replays.lock();
        while history.len() >= self.inner.replay_history_capacity {
            history.pop_front();
        }
        history.push_back(replay);
    }

    pub(super) fn next_replay_event_sequence(&self) -> u64 {
        self.inner
            .replay_event_sequence
            .fetch_add(1, Ordering::Relaxed)
            + 1
    }

    pub(super) fn push_replay_event(&self, event: ArenaReplayEvent) {
        {
            let mut replay_events = self.inner.replay_events.lock();
            while replay_events.len() >= self.inner.replay_event_history_capacity {
                replay_events.pop_front();
            }
            replay_events.push_back(event.clone());
        }
        let _ = self.inner.replay_event_tx.send(event);
    }

    pub(super) fn record_match_replay_events(
        &self,
        queued_match: &QueuedMatchView,
        seed: u64,
        completed_at: u64,
        outcome: &BotMatchOutcome,
        replay: &BotMatchReplay,
    ) {
        let mut events = Vec::with_capacity(replay.frames.len().saturating_add(2));
        let start_event = ArenaReplayEvent {
            sequence: self.next_replay_event_sequence(),
            emitted_at: completed_at,
            match_id: queued_match.match_id.clone(),
            mode: outcome.mode.clone(),
            event_type: "match_start".to_owned(),
            tick: Some(0),
            action_model_a: None,
            action_model_b: None,
            health_model_a: None,
            health_model_b: None,
            score_model_a: Some(0),
            score_model_b: Some(0),
            objective_a: Some(0),
            objective_b: Some(0),
            winner_model_id: None,
            draw: None,
            duration_ms: None,
        };
        self.push_replay_event(start_event.clone());
        events.push(start_event);

        for frame in &replay.frames {
            let tick_event = ArenaReplayEvent {
                sequence: self.next_replay_event_sequence(),
                emitted_at: completed_at,
                match_id: queued_match.match_id.clone(),
                mode: outcome.mode.clone(),
                event_type: "tick".to_owned(),
                tick: Some(frame.tick),
                action_model_a: Some(frame.action_model_a.clone()),
                action_model_b: Some(frame.action_model_b.clone()),
                health_model_a: Some(frame.health_model_a),
                health_model_b: Some(frame.health_model_b),
                score_model_a: Some(frame.score_model_a),
                score_model_b: Some(frame.score_model_b),
                objective_a: Some(frame.objective_a),
                objective_b: Some(frame.objective_b),
                winner_model_id: None,
                draw: None,
                duration_ms: None,
            };
            self.push_replay_event(tick_event.clone());
            events.push(tick_event);
        }

        let completed_event = ArenaReplayEvent {
            sequence: self.next_replay_event_sequence(),
            emitted_at: completed_at,
            match_id: queued_match.match_id.clone(),
            mode: outcome.mode.clone(),
            event_type: if outcome.draw {
                "match_draw".to_owned()
            } else {
                "match_end".to_owned()
            },
            tick: Some(outcome.ticks_executed),
            action_model_a: None,
            action_model_b: None,
            health_model_a: None,
            health_model_b: None,
            score_model_a: Some(outcome.model_a_score),
            score_model_b: Some(outcome.model_b_score),
            objective_a: Some(outcome.objective_a),
            objective_b: Some(outcome.objective_b),
            winner_model_id: outcome.winner_model_id.clone(),
            draw: Some(outcome.draw),
            duration_ms: Some(outcome.duration_ms),
        };
        self.push_replay_event(completed_event.clone());
        events.push(completed_event);

        let replay_record = ArenaMatchReplayRecord {
            match_id: queued_match.match_id.clone(),
            mode: outcome.mode.clone(),
            model_a_id: queued_match.model_a_id.clone(),
            model_b_id: queued_match.model_b_id.clone(),
            seed,
            max_ticks: replay.max_ticks,
            ticks_executed: outcome.ticks_executed,
            duration_ms: outcome.duration_ms,
            winner_model_id: outcome.winner_model_id.clone(),
            draw: outcome.draw,
            warnings: outcome
                .warnings
                .iter()
                .take(MAX_ARENA_REPLAY_WARNINGS)
                .cloned()
                .collect(),
            truncated: replay.truncated,
            total_frames: replay.total_ticks_executed as usize,
            completed_at,
            events,
        };

        let mut replay_match_order = self.inner.replay_match_order.lock();
        let mut replay_matches = self.inner.replay_matches.write();
        if !replay_matches.contains_key(&queued_match.match_id) {
            replay_match_order.push_back(queued_match.match_id.clone());
        }
        replay_matches.insert(queued_match.match_id.clone(), replay_record);
        while replay_match_order.len() > self.inner.replay_match_history_capacity {
            if let Some(evicted_match_id) = replay_match_order.pop_front() {
                replay_matches.remove(&evicted_match_id);
            }
        }
    }

    pub(super) fn recent_replay_events(
        &self,
        limit: usize,
        after_sequence: Option<u64>,
    ) -> ArenaReplayEventFeedResponse {
        let bounded_limit = limit.clamp(1, MAX_ARENA_REPLAY_EVENTS_LIMIT);
        let after = after_sequence.unwrap_or(0);
        let replay_events = self.inner.replay_events.lock();
        let mut filtered: Vec<ArenaReplayEvent> = replay_events
            .iter()
            .filter(|event| event.sequence > after)
            .cloned()
            .collect();
        if filtered.len() > bounded_limit {
            let truncate_from = filtered.len() - bounded_limit;
            filtered.drain(0..truncate_from);
        }
        let newest_sequence = filtered.last().map(|event| event.sequence);
        ArenaReplayEventFeedResponse {
            generated_at: super::scoring::unix_now(),
            total_events: replay_events.len(),
            returned_events: filtered.len(),
            newest_sequence,
            events: filtered,
        }
    }

    pub(super) fn replay_events_for_match(
        &self,
        match_id: &str,
        limit: usize,
        after_sequence: Option<u64>,
    ) -> Result<ArenaMatchReplayResponse, ArenaError> {
        let bounded_limit = limit.clamp(1, MAX_ARENA_REPLAY_EVENTS_LIMIT);
        let after = after_sequence.unwrap_or(0);
        let record = self
            .inner
            .replay_matches
            .read()
            .get(match_id)
            .cloned()
            .ok_or_else(|| {
                ArenaError::NotFound(
                    "replay_not_found",
                    format!("replay for match '{}' was not found", match_id),
                )
            })?;

        let mut filtered: Vec<ArenaReplayEvent> = record
            .events
            .iter()
            .filter(|event| event.sequence > after)
            .cloned()
            .collect();
        if filtered.len() > bounded_limit {
            let truncate_from = filtered.len() - bounded_limit;
            filtered.drain(0..truncate_from);
        }
        let returned_events = filtered.len();

        Ok(ArenaMatchReplayResponse {
            generated_at: super::scoring::unix_now(),
            match_id: record.match_id,
            mode: record.mode,
            model_a_id: record.model_a_id,
            model_b_id: record.model_b_id,
            seed: record.seed,
            max_ticks: record.max_ticks,
            ticks_executed: record.ticks_executed,
            duration_ms: record.duration_ms,
            winner_model_id: record.winner_model_id,
            draw: record.draw,
            truncated: record.truncated,
            total_frames: record.total_frames,
            total_events: record.events.len(),
            returned_events,
            completed_at: record.completed_at,
            warnings: record.warnings,
            events: filtered,
        })
    }

    pub(super) fn replay_stream(&self, query: super::types::ReplayStreamQuery) -> impl Reply {
        let after_sequence = query.after_sequence.unwrap_or(0);
        let backlog_limit = query
            .backlog
            .unwrap_or(DEFAULT_ARENA_REPLAY_STREAM_BACKLOG)
            .clamp(0, MAX_ARENA_STREAM_BACKLOG);
        let backlog_events = if backlog_limit == 0 {
            Vec::new()
        } else {
            self.recent_replay_events(backlog_limit, Some(after_sequence))
                .events
        };
        let receiver = self.inner.replay_event_tx.subscribe();
        let backlog_stream = stream::iter(
            backlog_events
                .into_iter()
                .map(arena_replay_event_to_sse_result),
        );
        let live_stream = BroadcastStream::new(receiver).filter_map(move |result| async move {
            match result {
                Ok(event) if event.sequence > after_sequence => {
                    Some(arena_replay_event_to_sse_result(event))
                }
                Ok(_) => None,
                Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(skipped)) => {
                    let lag_event = warp::sse::Event::default()
                        .event("lagged")
                        .data(format!("lagged:{}", skipped));
                    Some(Ok(lag_event))
                }
            }
        });
        let event_stream = backlog_stream.chain(live_stream);
        warp::sse::reply(
            warp::sse::keep_alive()
                .interval(Duration::from_secs(10))
                .text("keepalive")
                .stream(event_stream),
        )
    }
}

fn arena_replay_event_to_sse_result(
    event: ArenaReplayEvent,
) -> Result<warp::sse::Event, Infallible> {
    let event_name = event.event_type.clone();
    let event_id = event.sequence.to_string();
    let sse_event = match warp::sse::Event::default()
        .id(event_id)
        .event(event_name)
        .json_data(&event)
    {
        Ok(sse_event) => sse_event,
        Err(_) => warp::sse::Event::default()
            .event("encode_error")
            .data("{\"error\":\"failed to encode replay event\"}"),
    };
    Ok(sse_event)
}
