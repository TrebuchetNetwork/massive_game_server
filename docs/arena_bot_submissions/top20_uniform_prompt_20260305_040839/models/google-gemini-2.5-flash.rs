#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    const PUSH: i32 = 1;
    const HOLD: i32 = 0;
    const RETREAT: i32 = -1;

    let score_difference = self_score - enemy_health; // Assuming enemy_health is actually enemy_score for comparison

    // Preserve lead when self_score is high
    if score_difference > 100 && self_health > 50 {
        // If we have a significant lead and good health, play it safe but don't turtle completely
        if tick % 10 < 3 {
            return RETREAT; // Briefly retreat to maintain distance
        } else if tick % 10 < 7 {
            return HOLD; // Hold position
        } else {
            return PUSH; // Light push to keep pressure
        }
    }

    // Take calculated risks when trailing
    if score_difference < -50 {
        // If we are significantly behind
        if self_health > 70 {
            // High health, take a strong risk
            return PUSH;
        } else if self_health > 30 {
            // Medium health, calculated push
            if tick % 5 == 0 {
                return PUSH;
            } else {
                return HOLD;
            }
        } else {
            // Low health, don't be reckless, try to survive and recover
            return RETREAT;
        }
    }

    // Avoid reckless pushes at low health
    if self_health < 20 {
        // Very low health, prioritize survival
        return RETREAT;
    }

    // General strategy based on health and tick for dynamic behavior
    if self_health > enemy_health + 20 {
        // Significant health advantage
        if tick % 7 < 5 {
            return PUSH;
        } else {
            return HOLD;
        }
    } else if enemy_health > self_health + 20 {
        // Significant health disadvantage
        if tick % 7 < 3 {
            return RETREAT;
        } else {
            return HOLD;
        }
    } else {
        // Healths are relatively even
        if tick % 15 < 5 {
            return PUSH;
        } else if tick % 15 < 10 {
            return HOLD;
        } else {
            return RETREAT;
        }
    }
}