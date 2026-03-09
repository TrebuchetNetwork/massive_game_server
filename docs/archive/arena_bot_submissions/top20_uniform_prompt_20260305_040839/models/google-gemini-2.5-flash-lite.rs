#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    // Constants for decision thresholds
    const LOW_HEALTH_THRESHOLD: i32 = 30;
    const HIGH_SCORE_THRESHOLD: i32 = 70;
    const SCORE_DIFFERENCE_THRESHOLD: i32 = 20;
    const AGGRESSION_RHYTHM_MODIFIER: i32 = 2; // Controls how much tick influences aggression

    // Basic health check: prioritize survival if health is low
    if self_health < LOW_HEALTH_THRESHOLD {
        return -1; // Retreat/defend
    }

    // Determine current aggression level based on score and health
    let mut aggression_level: i32 = 0; // Default to hold/reposition

    // High score lead: play defensively to preserve the lead
    if self_score > HIGH_SCORE_THRESHOLD && self_score > enemy_health {
        aggression_level = -1; // Preserve lead, play defensively
    }
    // Trailing significantly: take calculated risks
    else if self_score < enemy_health - SCORE_DIFFERENCE_THRESHOLD {
        // Increase aggression slightly when trailing, but not if health is moderate
        if self_health > LOW_HEALTH_THRESHOLD + 20 {
            aggression_level = 1; // Push/attack
        } else {
            aggression_level = 0; // Hold/reposition if health is not great
        }
    }
    // Close game or slight lead: balanced approach
    else if self_score > enemy_health {
        aggression_level = 1; // Push/attack if we have a slight lead
    }
    // Enemy has a slight lead or it's a tie: cautious approach
    else {
        aggression_level = 0; // Hold/reposition
    }

    // Introduce deterministic rhythm based on tick to avoid static behavior
    // This adds a slight push/attack tendency on certain ticks,
    // making the bot less predictable.
    // The modulo operation ensures the rhythm is consistent.
    if tick % AGGRESSION_RHYTHM_MODIFIER == 0 {
        // If we were already aggressive, reinforce it.
        // If we were holding, this might push us to attack.
        // If we were retreating, this might slightly reduce the retreat.
        aggression_level = aggression_level.max(0); // Ensure it doesn't become more defensive than hold
    } else if tick % AGGRESSION_RHYTHM_MODIFIER == 1 {
        // On other ticks, slightly reduce aggression if we were pushing.
        // This creates a subtle ebb and flow.
        aggression_level = aggression_level.min(0); // Ensure it doesn't become more aggressive than hold
    }

    // Final decision based on calculated aggression level
    if aggression_level > 0 {
        1 //