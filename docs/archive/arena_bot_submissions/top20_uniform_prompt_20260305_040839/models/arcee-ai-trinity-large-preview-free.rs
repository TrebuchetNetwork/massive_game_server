#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    // Define thresholds
    let health_threshold = 30;
    let score_lead_threshold = 20;
    let score_trail_threshold = -20;

    // Calculate score difference
    let score_diff = self_score;

    // Determine action based on health and score
    if self_health <= health_threshold {
        // Low health: prioritize survival
        -1
    } else if score_diff >= score_lead_threshold {
        // Leading significantly: play conservatively
        0
    } else if score_diff <= score_trail_threshold {
        // Trailing significantly: take calculated risks
        1
    } else {
        // Moderate situation: use tick for rhythm-based decisions
        if tick % 4 == 0 {
            1
        } else if tick % 4 == 2 {
            -1
        } else {
            0
        }
    }
}