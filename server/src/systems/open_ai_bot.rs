use rand::Rng;
use crate::core::types::{Vec2, PlayerID};
use crate::core::constants::*;
use super::BotBehavior;

/// Simple bot behavior used for OpenAI bot example.
/// Chooses targets similar to the balanced bot with a bit
/// more aggression when health is high.
pub fn decide_action(
    bot: &mut BotBehavior,
    bot_pos: Vec2,
    bot_health: i32,
    closest_enemy: Option<(PlayerID, f32, Vec2)>,
    rng: &mut impl Rng,
) {
    if let Some((enemy_id, dist, enemy_pos)) = closest_enemy {
        if dist < 350.0 || bot_health > 80 {
            bot.target_player = Some(enemy_id);
            bot.target_position = Some(enemy_pos);
        } else {
            bot.target_player = None;
            bot.target_position = Some(Vec2::new(
                rng.gen_range(-400.0..400.0),
                rng.gen_range(-300.0..300.0),
            ));
        }
    } else {
        bot.target_player = None;
        bot.target_position = Some(Vec2::new(
            rng.gen_range((WORLD_MIN_X + 200.0)..(WORLD_MAX_X - 200.0)),
            rng.gen_range((WORLD_MIN_Y + 200.0)..(WORLD_MAX_Y - 200.0)),
        ));
    }
}
