// massive_game_server/server/src/operational/tuning/auto_tuner.rs

use super::adaptive_quality::{adjust_quality, QualitySettings};

#[derive(Debug, Clone, Copy)]
pub struct TuningSample {
    pub frame_time_ms: f32,
    pub connected_players: usize,
}

#[derive(Debug, Clone)]
pub struct AutoTuner {
    target_frame_ms: f32,
    quality: QualitySettings,
}

impl AutoTuner {
    pub fn new(target_frame_ms: f32) -> Self {
        Self {
            target_frame_ms,
            quality: QualitySettings::default(),
        }
    }

    pub fn ingest_sample(&mut self, sample: TuningSample) -> QualitySettings {
        let pressure_adjusted_target = if sample.connected_players > 180 {
            self.target_frame_ms * 0.95
        } else {
            self.target_frame_ms
        };
        self.quality = adjust_quality(self.quality, sample.frame_time_ms, pressure_adjusted_target);
        self.quality
    }

    pub fn quality(&self) -> QualitySettings {
        self.quality
    }
}
