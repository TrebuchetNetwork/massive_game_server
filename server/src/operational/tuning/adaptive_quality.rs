// massive_game_server/server/src/operational/tuning/adaptive_quality.rs

#[derive(Debug, Clone, Copy)]
pub struct QualitySettings {
    pub aoi_radius_scale: f32,
    pub max_projectiles_scale: f32,
    pub delta_skip_modulus: u64,
}

impl Default for QualitySettings {
    fn default() -> Self {
        Self {
            aoi_radius_scale: 1.0,
            max_projectiles_scale: 1.0,
            delta_skip_modulus: 1,
        }
    }
}

pub fn adjust_quality(
    current: QualitySettings,
    frame_time_ms: f32,
    target_ms: f32,
) -> QualitySettings {
    if frame_time_ms > target_ms * 1.20 {
        return QualitySettings {
            aoi_radius_scale: (current.aoi_radius_scale * 0.95).clamp(0.65, 1.0),
            max_projectiles_scale: (current.max_projectiles_scale * 0.9).clamp(0.5, 1.0),
            delta_skip_modulus: (current.delta_skip_modulus + 1).min(4),
        };
    }

    if frame_time_ms < target_ms * 0.80 {
        return QualitySettings {
            aoi_radius_scale: (current.aoi_radius_scale * 1.03).clamp(0.65, 1.0),
            max_projectiles_scale: (current.max_projectiles_scale * 1.05).clamp(0.5, 1.0),
            delta_skip_modulus: current.delta_skip_modulus.saturating_sub(1).max(1),
        };
    }

    current
}
