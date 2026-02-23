use crate::core::types::Vec2;

#[inline]
pub fn integrate_velocity(position: Vec2, velocity: Vec2, delta_time: f32) -> Vec2 {
    Vec2::new(
        position.x + velocity.x * delta_time,
        position.y + velocity.y * delta_time,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrates_position() {
        let p0 = Vec2::new(10.0, -2.0);
        let v = Vec2::new(5.0, 1.0);
        let p1 = integrate_velocity(p0, v, 0.5);
        assert!((p1.x - 12.5).abs() < 0.001);
        assert!((p1.y - -1.5).abs() < 0.001);
    }
}
