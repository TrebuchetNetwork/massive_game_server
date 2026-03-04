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
}
