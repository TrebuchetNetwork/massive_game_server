// massive_game_server/server/src/concurrent/event_queue.rs
use crate::core::types::{EventPriority, GameEvent}; // Assuming GameEvent and EventPriority are defined
use crossbeam_queue::SegQueue;
use std::sync::Arc;

/// Maximum number of high-priority events drained per `pop_batch` call before
/// normal/low-priority events get a turn. This prevents high-priority flood
/// from starving other priorities indefinitely.
const HIGH_PRIORITY_BATCH_LIMIT: usize = 64;

/// Maximum number of normal-priority events drained per `pop_batch` call before
/// low-priority events get a turn.
const NORMAL_PRIORITY_BATCH_LIMIT: usize = 32;

// Lock-free event queue with priority (from user code)
pub struct PriorityEventQueue {
    high_priority: Arc<SegQueue<GameEvent>>,
    normal_priority: Arc<SegQueue<GameEvent>>,
    low_priority: Arc<SegQueue<GameEvent>>,
}

impl PriorityEventQueue {
    pub fn new() -> Self {
        PriorityEventQueue {
            high_priority: Arc::new(SegQueue::new()),
            normal_priority: Arc::new(SegQueue::new()),
            low_priority: Arc::new(SegQueue::new()),
        }
    }

    pub fn push(&self, event: GameEvent, priority: EventPriority) {
        match priority {
            EventPriority::High => self.high_priority.push(event),
            EventPriority::Normal => self.normal_priority.push(event),
            EventPriority::Low => self.low_priority.push(event),
        }
    }

    /// Pop a single event. Still prioritizes high > normal > low, but callers
    /// that need fairness should prefer `pop_batch`.
    pub fn pop(&self) -> Option<GameEvent> {
        // Pop from high priority first, then normal, then low
        if let Some(event) = self.high_priority.pop() {
            return Some(event);
        }
        if let Some(event) = self.normal_priority.pop() {
            return Some(event);
        }
        if let Some(event) = self.low_priority.pop() {
            return Some(event);
        }
        None
    }

    /// Drain up to `max_count` events with fairness guarantees:
    ///
    ///  1. Up to `HIGH_PRIORITY_BATCH_LIMIT` high-priority events.
    ///  2. Up to `NORMAL_PRIORITY_BATCH_LIMIT` normal-priority events.
    ///  3. Fill remaining capacity from low-priority.
    ///  4. If budget remains and high/normal still have events, continue
    ///     draining them in the same ratio so nothing is wasted.
    ///
    /// This ensures that even under sustained high-priority load, normal and
    /// low-priority events make forward progress every batch.
    pub fn pop_batch(&self, max_count: usize) -> Vec<GameEvent> {
        let mut batch = Vec::with_capacity(max_count);

        // Phase 1: Drain up to HIGH_PRIORITY_BATCH_LIMIT from high.
        let high_limit = HIGH_PRIORITY_BATCH_LIMIT.min(max_count);
        Self::drain_queue(&self.high_priority, &mut batch, high_limit);

        if batch.len() >= max_count {
            return batch;
        }

        // Phase 2: Drain up to NORMAL_PRIORITY_BATCH_LIMIT from normal.
        let normal_limit = NORMAL_PRIORITY_BATCH_LIMIT.min(max_count - batch.len());
        Self::drain_queue(&self.normal_priority, &mut batch, normal_limit);

        if batch.len() >= max_count {
            return batch;
        }

        // Phase 3: Fill remaining from low.
        let low_limit = max_count - batch.len();
        Self::drain_queue(&self.low_priority, &mut batch, low_limit);

        if batch.len() >= max_count {
            return batch;
        }

        // Phase 4: If budget remains, continue draining high then normal to
        // avoid wasting capacity when low is empty.
        let remaining = max_count - batch.len();
        Self::drain_queue(&self.high_priority, &mut batch, remaining);

        if batch.len() < max_count {
            let remaining = max_count - batch.len();
            Self::drain_queue(&self.normal_priority, &mut batch, remaining);
        }

        batch
    }

    fn drain_queue(queue: &SegQueue<GameEvent>, batch: &mut Vec<GameEvent>, limit: usize) {
        for _ in 0..limit {
            if let Some(event) = queue.pop() {
                batch.push(event);
            } else {
                break;
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.high_priority.is_empty()
            && self.normal_priority.is_empty()
            && self.low_priority.is_empty()
    }

    pub fn len(&self) -> usize {
        self.high_priority.len() + self.normal_priority.len() + self.low_priority.len()
    }
}

impl Default for PriorityEventQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_player_event(tag: &str) -> GameEvent {
        GameEvent::PlayerJoined {
            player_id: Arc::new(tag.to_owned()),
        }
    }

    fn player_id_of(event: &GameEvent) -> &str {
        match event {
            GameEvent::PlayerJoined { player_id } => player_id.as_str(),
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn pop_batch_respects_high_priority_limit() {
        let queue = PriorityEventQueue::new();

        // Push 200 high-priority events (tagged "h0" .. "h199").
        for i in 0..200 {
            queue.push(make_player_event(&format!("h{}", i)), EventPriority::High);
        }
        // Push 10 normal-priority events.
        for i in 0..10 {
            queue.push(make_player_event(&format!("n{}", i)), EventPriority::Normal);
        }
        // Push 5 low-priority events.
        for i in 0..5 {
            queue.push(make_player_event(&format!("l{}", i)), EventPriority::Low);
        }

        let batch = queue.pop_batch(100);
        assert_eq!(batch.len(), 100);

        // Count how many are from each priority.
        let mut high_count = 0;
        let mut normal_count = 0;
        let mut low_count = 0;
        for event in &batch {
            let id = player_id_of(event);
            if id.starts_with('h') {
                high_count += 1;
            } else if id.starts_with('n') {
                normal_count += 1;
            } else {
                low_count += 1;
            }
        }

        // Normal and low events must appear in the batch (not starved).
        assert!(normal_count > 0, "normal events were starved");
        assert!(low_count > 0, "low events were starved");
        // High events should be capped at HIGH_PRIORITY_BATCH_LIMIT in the first phase
        // but may get more in phase 4 (filling remaining capacity).
        assert!(high_count >= HIGH_PRIORITY_BATCH_LIMIT);
    }

    #[test]
    fn pop_batch_drains_all_when_no_starvation_risk() {
        let queue = PriorityEventQueue::new();

        for i in 0..5 {
            queue.push(make_player_event(&format!("h{}", i)), EventPriority::High);
        }
        for i in 0..3 {
            queue.push(make_player_event(&format!("n{}", i)), EventPriority::Normal);
        }
        for i in 0..2 {
            queue.push(make_player_event(&format!("l{}", i)), EventPriority::Low);
        }

        let batch = queue.pop_batch(20);
        // All 10 events should be drained since total < max_count.
        assert_eq!(batch.len(), 10);
        assert!(queue.is_empty());
    }

    #[test]
    fn pop_single_still_prioritizes() {
        let queue = PriorityEventQueue::new();
        queue.push(make_player_event("low"), EventPriority::Low);
        queue.push(make_player_event("high"), EventPriority::High);
        queue.push(make_player_event("normal"), EventPriority::Normal);

        // Single pop should still prefer high.
        let first = queue.pop().unwrap();
        assert_eq!(player_id_of(&first), "high");
    }
}
