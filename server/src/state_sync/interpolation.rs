// massive_game_server/server/src/state_sync/interpolation.rs

use crate::core::math::lerp;
use crate::core::types::Vec2;
use std::collections::VecDeque;

pub trait Interpolate: Clone {
    fn interpolate(&self, newer: &Self, alpha: f32) -> Self;
}

impl Interpolate for Vec2 {
    fn interpolate(&self, newer: &Self, alpha: f32) -> Self {
        Self {
            x: lerp(self.x, newer.x, alpha),
            y: lerp(self.y, newer.y, alpha),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimedSample<T> {
    pub timestamp_ms: u64,
    pub value: T,
}

#[derive(Debug, Clone)]
pub struct InterpolationBuffer<T> {
    samples: VecDeque<TimedSample<T>>,
    max_samples: usize,
}

impl<T: Interpolate> InterpolationBuffer<T> {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples.max(2)),
            max_samples: max_samples.max(2),
        }
    }

    pub fn push(&mut self, timestamp_ms: u64, value: T) {
        if let Some(last) = self.samples.back_mut() {
            if timestamp_ms < last.timestamp_ms {
                // Ignore out-of-order samples to preserve monotonic interpolation windows.
                return;
            }
            if timestamp_ms == last.timestamp_ms {
                // Replace duplicate timestamp with fresher value.
                last.value = value;
                return;
            }
        }
        self.samples.push_back(TimedSample {
            timestamp_ms,
            value,
        });
        while self.samples.len() > self.max_samples {
            let _ = self.samples.pop_front();
        }
    }

    pub fn sample_at(&self, timestamp_ms: u64) -> Option<T> {
        let first = self.samples.front()?;
        let last = self.samples.back()?;

        if timestamp_ms <= first.timestamp_ms {
            return Some(first.value.clone());
        }
        if timestamp_ms >= last.timestamp_ms {
            return Some(last.value.clone());
        }

        for pair in self.samples.as_slices().0.windows(2) {
            let older = &pair[0];
            let newer = &pair[1];
            if older.timestamp_ms <= timestamp_ms && timestamp_ms <= newer.timestamp_ms {
                let span = (newer.timestamp_ms - older.timestamp_ms).max(1);
                let alpha = (timestamp_ms - older.timestamp_ms) as f32 / span as f32;
                return Some(older.value.interpolate(&newer.value, alpha));
            }
        }

        // Fallback path for wrapped ring slices.
        let all = self.samples.iter().collect::<Vec<_>>();
        for pair in all.windows(2) {
            let older = pair[0];
            let newer = pair[1];
            if older.timestamp_ms <= timestamp_ms && timestamp_ms <= newer.timestamp_ms {
                let span = (newer.timestamp_ms - older.timestamp_ms).max(1);
                let alpha = (timestamp_ms - older.timestamp_ms) as f32 / span as f32;
                return Some(older.value.interpolate(&newer.value, alpha));
            }
        }

        Some(last.value.clone())
    }

    pub fn recent_samples(&self, limit: usize) -> Vec<TimedSample<T>> {
        let bounded = limit.max(1).min(self.samples.len().max(1));
        let mut result: Vec<_> = self.samples.iter().rev().take(bounded).cloned().collect();
        result.reverse();
        result
    }

    /// Clear all samples. Call this when the entity disconnects or is removed
    /// so that the buffer does not leak memory for departed players.
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Returns true if the buffer contains no samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Number of samples currently stored.
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_midpoint() {
        let mut buffer = InterpolationBuffer::new(8);
        buffer.push(100, Vec2::new(0.0, 0.0));
        buffer.push(200, Vec2::new(10.0, 10.0));
        let sample = buffer.sample_at(150).expect("sample");
        assert!((sample.x - 5.0).abs() < 0.01);
        assert!((sample.y - 5.0).abs() < 0.01);
    }

    #[test]
    fn ignores_out_of_order_samples() {
        let mut buffer = InterpolationBuffer::new(8);
        buffer.push(100, Vec2::new(1.0, 1.0));
        buffer.push(120, Vec2::new(2.0, 2.0));
        buffer.push(110, Vec2::new(99.0, 99.0));
        assert_eq!(buffer.sample_count(), 2);
    }

    #[test]
    fn replaces_duplicate_timestamp_sample() {
        let mut buffer = InterpolationBuffer::new(8);
        buffer.push(100, Vec2::new(1.0, 1.0));
        buffer.push(100, Vec2::new(5.0, 5.0));
        assert_eq!(buffer.sample_count(), 1);
        let sample = buffer.sample_at(100).expect("sample at exact ts");
        assert!((sample.x - 5.0).abs() < 0.01);
        assert!((sample.y - 5.0).abs() < 0.01);
    }
}
