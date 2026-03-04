use super::*;

impl MassiveGameServer {
    #[inline]
    pub(super) fn advance_killstreak(&self, attacker_state: &mut PlayerState) -> u32 {
        attacker_state.current_streak += 1;
        attacker_state.peak_streak = attacker_state
            .peak_streak
            .max(attacker_state.current_streak);
        let streak = attacker_state.current_streak;
        attacker_state.apply_killstreak_reward_for_streak(streak);
        streak
    }

    #[inline]
    pub(super) fn momentum_score_multiplier_for_streak(streak: u32) -> f32 {
        use crate::core::constants::{
            MOMENTUM_SCORE_TIER_ONE_MULTIPLIER, MOMENTUM_SCORE_TIER_ONE_STREAK,
            MOMENTUM_SCORE_TIER_THREE_MULTIPLIER, MOMENTUM_SCORE_TIER_THREE_STREAK,
            MOMENTUM_SCORE_TIER_TWO_MULTIPLIER, MOMENTUM_SCORE_TIER_TWO_STREAK,
        };
        if streak >= MOMENTUM_SCORE_TIER_THREE_STREAK {
            MOMENTUM_SCORE_TIER_THREE_MULTIPLIER
        } else if streak >= MOMENTUM_SCORE_TIER_TWO_STREAK {
            MOMENTUM_SCORE_TIER_TWO_MULTIPLIER
        } else if streak >= MOMENTUM_SCORE_TIER_ONE_STREAK {
            MOMENTUM_SCORE_TIER_ONE_MULTIPLIER
        } else {
            1.0
        }
    }

    #[inline]
    pub(super) fn apply_momentum_score_bonus(base_points: i32, streak: u32) -> i32 {
        if base_points <= 0 {
            return base_points;
        }
        let multiplier = Self::momentum_score_multiplier_for_streak(streak);
        let boosted = (base_points as f32 * multiplier).round();
        if !boosted.is_finite() {
            return base_points;
        }
        boosted.clamp(i32::MIN as f32, i32::MAX as f32) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::MassiveGameServer;

    #[test]
    fn momentum_multiplier_applies_tiered_bonus() {
        assert_eq!(MassiveGameServer::apply_momentum_score_bonus(10, 0), 10);
        assert_eq!(MassiveGameServer::apply_momentum_score_bonus(10, 3), 15);
        assert_eq!(MassiveGameServer::apply_momentum_score_bonus(10, 5), 20);
        assert_eq!(MassiveGameServer::apply_momentum_score_bonus(15, 8), 45);
    }

    #[test]
    fn momentum_multiplier_keeps_non_positive_scores_unchanged() {
        assert_eq!(MassiveGameServer::apply_momentum_score_bonus(0, 8), 0);
        assert_eq!(MassiveGameServer::apply_momentum_score_bonus(-200, 8), -200);
    }
}
