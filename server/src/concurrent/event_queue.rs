// massive_game_server/server/src/concurrent/event_queue.rs
use crate::core::types::{GameEvent, EventPriority}; // Assuming GameEvent and EventPriority are defined
use crossbeam_queue::SegQueue;
use std::sync::Arc;

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

    pub fn pop_batch(&self, max_count: usize) -> Vec<GameEvent> {
        let mut batch = Vec::with_capacity(max_count);
        let mut count = 0;

        while count < max_count {
            if let Some(event) = self.high_priority.pop() {
                batch.push(event);
                count += 1;
                if count >= max_count { break; }
            } else {
                break; // No more high priority
            }
        }
        while count < max_count {
            if let Some(event) = self.normal_priority.pop() {
                batch.push(event);
                count += 1;
                if count >= max_count { break; }
            } else {
                break; // No more normal priority
            }
        }
        while count < max_count {
            if let Some(event) = self.low_priority.pop() {
                batch.push(event);
                count += 1;
                if count >= max_count { break; }
            } else {
                break; // No more low priority
            }
        }
        batch
    }

    pub fn is_empty(&self) -> bool {
        self.high_priority.is_empty() && self.normal_priority.is_empty() && self.low_priority.is_empty()
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