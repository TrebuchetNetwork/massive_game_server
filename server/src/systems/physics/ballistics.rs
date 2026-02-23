use crate::core::types::Vec2;

#[inline]
pub fn projectile_step(position: Vec2, velocity: Vec2, delta_time: f32) -> Vec2 {
    Vec2::new(
        position.x + velocity.x * delta_time,
        position.y + velocity.y * delta_time,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projectile_advances() {
        let p0 = Vec2::new(0.0, 0.0);
        let v = Vec2::new(100.0, -40.0);
        let p1 = projectile_step(p0, v, 0.1);
        assert!((p1.x - 10.0).abs() < 0.001);
        assert!((p1.y - -4.0).abs() < 0.001);
    }
}
