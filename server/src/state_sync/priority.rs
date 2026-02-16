// massive_game_server/server/src/state_sync/priority.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityClass {
    Player,
    Projectile,
    Pickup,
    Objective,
    Wall,
}

pub fn priority_score(
    distance_sq: f32,
    recently_changed: bool,
    age_frames: u32,
    class: EntityClass,
) -> f32 {
    let class_weight = match class {
        EntityClass::Player => 1.0,
        EntityClass::Projectile => 0.9,
        EntityClass::Objective => 0.8,
        EntityClass::Pickup => 0.5,
        EntityClass::Wall => 0.3,
    };
    let distance_factor = 1.0 / (1.0 + distance_sq.sqrt() * 0.01);
    let freshness_factor = if recently_changed { 1.2 } else { 1.0 };
    let age_penalty = 1.0 / (1.0 + (age_frames as f32 * 0.02));
    class_weight * distance_factor * freshness_factor * age_penalty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearby_player_scores_higher_than_far_wall() {
        let near_player = priority_score(25.0, true, 0, EntityClass::Player);
        let far_wall = priority_score(10_000.0, false, 30, EntityClass::Wall);
        assert!(near_player > far_wall);
    }
}
