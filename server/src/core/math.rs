// massive_game_server/server/src/core/math.rs

#[inline]
pub fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * clamp01(t)
}

#[inline]
pub fn squared_distance_2d(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

#[inline]
pub fn normalize_2d(x: f32, y: f32) -> (f32, f32) {
    let mag_sq = x * x + y * y;
    if mag_sq <= f32::EPSILON {
        return (0.0, 0.0);
    }
    let inv = mag_sq.sqrt().recip();
    (x * inv, y * inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_is_clamped() {
        assert_eq!(lerp(0.0, 10.0, -1.0), 0.0);
        assert_eq!(lerp(0.0, 10.0, 2.0), 10.0);
    }

    #[test]
    fn normalize_handles_zero() {
        assert_eq!(normalize_2d(0.0, 0.0), (0.0, 0.0));
    }
}
