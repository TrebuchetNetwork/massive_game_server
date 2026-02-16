// massive_game_server/server/src/systems/ai/commander.rs

use crate::core::types::Vec2;

#[derive(Debug, Clone, Copy)]
pub struct MotionSample {
    pub timestamp_ms: u64,
    pub position: Vec2,
}

#[derive(Debug, Clone, Default)]
pub struct PredictiveMotionModel {
    samples: Vec<MotionSample>,
}

impl PredictiveMotionModel {
    pub fn push_sample(&mut self, sample: MotionSample) {
        self.samples.push(sample);
        if self.samples.len() > 8 {
            self.samples.remove(0);
        }
    }

    // Lightweight online prediction; intentionally simple to keep deterministic behavior.
    pub fn predict_position(&self, future_timestamp_ms: u64) -> Option<Vec2> {
        let last = self.samples.last()?;
        let prev = self.samples.iter().rev().nth(1)?;
        let dt_ms = (last.timestamp_ms.saturating_sub(prev.timestamp_ms)).max(1) as f32;
        let vx = (last.position.x - prev.position.x) / dt_ms;
        let vy = (last.position.y - prev.position.y) / dt_ms;
        let future_dt = future_timestamp_ms.saturating_sub(last.timestamp_ms) as f32;
        Some(Vec2::new(
            last.position.x + vx * future_dt,
            last.position.y + vy * future_dt,
        ))
    }
}
