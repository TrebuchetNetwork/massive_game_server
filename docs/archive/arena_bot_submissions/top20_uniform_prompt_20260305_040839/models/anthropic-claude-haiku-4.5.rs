#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    let health_ratio = if enemy_health > 0 {
        (self_health as f32) / (enemy_health as f32)
    } else {
        2.0
    };
    
    let score_diff = self_score - (100 - self_score);
    let is_leading = score_diff > 0;
    let lead_margin = score_diff.abs();
    
    let tick_phase = tick % 16;
    let rhythm_pattern = tick_phase % 4;
    
    if self_health <= 15 {
        return -1;
    }
    
    if self_health <= 30 && health_ratio < 0.8 {
        return -1;
    }
    
    if is_leading && lead_margin > 15 {
        if self_health >= 60 {
            if rhythm_pattern == 0 || rhythm_pattern == 2 {
                return 1;
            } else {
                return 0;
            }
        } else {
            return 0;
        }
    }
    
    if !is_leading && lead_margin > 20 {
        if self_health >= 40 {
            if health_ratio >= 0.7 {
                return 1;
            } else if health_ratio >= 0.5 {
                if rhythm_pattern < 2 {
                    return 1;
                } else {
                    return 0;
                }
            } else {
                return 0;
            }
        } else {
            return 0;
        }
    }
    
    if health_ratio > 1.3 {
        if self_health >= 50 {
            if rhythm_pattern == 0 || rhythm_pattern == 1 {
                return 1;
            } else {
                return 0;
            }
        } else {
            return 0;
        }
    }
    
    if health_ratio < 0.7 && self_health > 25 {
        return -1;
    }
    
    if health_ratio >= 0.9 && health_ratio <= 1.1 {
        if self_health >= 45 {
            if rhythm_pattern < 2 {
                return 1;
            } else {
                return 0;
            }
        } else {
            return 0;
        }
    }
    
    if self_health >= 70 && enemy_health <= 40 {
        return 1;
    }
    
    if self_health >= 55 && self_health <= 75 {
        if rhythm_pattern == 0 {