#[no_mangle]
pub extern "C" fn bot_tick(
    self_health: i32,
    enemy_health: i32,
    self_score: i32,
    tick: i32,
) -> i32 {
    // Configuration thresholds
    const LOW_HEALTH: i32 = 20;
    const HIGH_SCORE_LEAD: i32 = 800;
    const LOW_SCORE_TRAIL: i32 = 200;

    // Difference in health gives a quick sense of advantage
    let health_advantage = self_health - enemy_health;

    // 1. Preserve a large lead – play conservatively
    if self_score >= HIGH_SCORE_LEAD {
        // If we are dangerously low on health, retreat immediately
        if self_health <= LOW_HEALTH {
            return -1; // retreat/defend
        }
        // Push only on a regular rhythm and when we have a health edge
        if health_advantage > 0 && tick % 5 == 0 {
            return 1; // push/attack
        }
        // Otherwise hold position
        return 0; // hold/reposition
    }

    // 2. When trailing far behind – take calculated risks
    if self_score <= LOW_SCORE_TRAIL {
        // Aggressive push if we have health advantage or the enemy is weak,
        // using a faster rhythm to break out of the deficit
        if (health_advantage > 0 || enemy_health <= LOW_HEALTH) && tick % 3 == 0 {
            return 1; // push/attack
        }
        // If our health is low, fall back to survive
        if self_health <= LOW_HEALTH {
            return -1; // retreat/defend
        }
        // Otherwise stay steady and look for an opening
        return 0; // hold/reposition
    }

    // 3. Neutral situation – balance pressure and safety
    // If health is critically low, retreat regardless of score
    if self_health <= LOW_HEALTH {
        return -1; // retreat/defend
    }
    // When we have a health advantage, apply periodic pressure
    if health_advantage > 0 && tick % 4 == 0 {
        return 1; // push/attack
    }
    // If the enemy is weak, consider a push on a different rhythm
    if enemy_health <= LOW_HEALTH && tick % 6 == 0 {
        return 1; // push/attack
    }
    // Default to holding position
    0 // hold/reposition
}