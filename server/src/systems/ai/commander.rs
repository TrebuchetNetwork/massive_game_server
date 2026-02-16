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
        if self.samples.len() > 16 {
            self.samples.remove(0);
        }
    }

    // Lightweight online prediction based on a short velocity+acceleration estimate.
    pub fn predict_position(&self, future_timestamp_ms: u64) -> Option<Vec2> {
        let last = self.samples.last()?;
        let prev = self.samples.iter().rev().nth(1)?;
        let dt_ms = (last.timestamp_ms.saturating_sub(prev.timestamp_ms)).max(1) as f32;

        let vx = (last.position.x - prev.position.x) / dt_ms;
        let vy = (last.position.y - prev.position.y) / dt_ms;

        let (ax, ay) = if let Some(prev2) = self.samples.iter().rev().nth(2) {
            let dt_prev = (prev.timestamp_ms.saturating_sub(prev2.timestamp_ms)).max(1) as f32;
            let vx_prev = (prev.position.x - prev2.position.x) / dt_prev;
            let vy_prev = (prev.position.y - prev2.position.y) / dt_prev;
            (
                (vx - vx_prev) / dt_ms.max(1.0),
                (vy - vy_prev) / dt_ms.max(1.0),
            )
        } else {
            (0.0, 0.0)
        };

        let future_dt = future_timestamp_ms.saturating_sub(last.timestamp_ms) as f32;
        let clamped_dt = future_dt.min(500.0);

        Some(Vec2::new(
            last.position.x + vx * clamped_dt + 0.5 * ax * clamped_dt * clamped_dt,
            last.position.y + vy * clamped_dt + 0.5 * ay * clamped_dt * clamped_dt,
        ))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ThreatSample {
    pub distance: f32,
    pub relative_speed: f32,
    pub recent_damage_taken: f32,
    pub target_visibility: f32,
}

#[derive(Debug, Clone)]
pub struct ThreatPredictor {
    // [bias, distance, relative_speed, damage, visibility]
    weights: [f32; 5],
    learning_rate: f32,
}

impl Default for ThreatPredictor {
    fn default() -> Self {
        Self {
            weights: [0.0, -0.015, 0.08, 0.2, 0.35],
            learning_rate: 0.02,
        }
    }
}

impl ThreatPredictor {
    pub fn predict_threat_score(&self, sample: ThreatSample) -> f32 {
        let x = sample.to_features();
        sigmoid(dot(self.weights, x))
    }

    // Online logistic regression update.
    pub fn train_online(&mut self, sample: ThreatSample, was_threat: bool) {
        let x = sample.to_features();
        let prediction = sigmoid(dot(self.weights, x));
        let label = if was_threat { 1.0 } else { 0.0 };
        let error = label - prediction;

        for (w, feature) in self.weights.iter_mut().zip(x.iter()) {
            *w += self.learning_rate * error * feature;
        }
    }

    pub fn weights(&self) -> [f32; 5] {
        self.weights
    }
}

impl ThreatSample {
    fn to_features(self) -> [f32; 5] {
        [
            1.0,
            self.distance.clamp(0.0, 3000.0),
            self.relative_speed.clamp(0.0, 1200.0),
            self.recent_damage_taken.clamp(0.0, 500.0),
            self.target_visibility.clamp(0.0, 1.0),
        ]
    }
}

fn dot(weights: [f32; 5], features: [f32; 5]) -> f32 {
    weights
        .iter()
        .zip(features.iter())
        .fold(0.0, |acc, (w, f)| acc + (w * f))
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_model_predicts_forward() {
        let mut model = PredictiveMotionModel::default();
        model.push_sample(MotionSample {
            timestamp_ms: 100,
            position: Vec2::new(0.0, 0.0),
        });
        model.push_sample(MotionSample {
            timestamp_ms: 200,
            position: Vec2::new(10.0, 0.0),
        });

        let predicted = model.predict_position(300).expect("prediction");
        assert!(predicted.x > 15.0);
    }

    #[test]
    fn threat_predictor_learns_online() {
        let mut predictor = ThreatPredictor::default();
        let sample = ThreatSample {
            distance: 250.0,
            relative_speed: 120.0,
            recent_damage_taken: 40.0,
            target_visibility: 1.0,
        };

        let before = predictor.predict_threat_score(sample);
        for _ in 0..32 {
            predictor.train_online(sample, true);
        }
        let after = predictor.predict_threat_score(sample);
        assert!(after >= before);
    }
}
