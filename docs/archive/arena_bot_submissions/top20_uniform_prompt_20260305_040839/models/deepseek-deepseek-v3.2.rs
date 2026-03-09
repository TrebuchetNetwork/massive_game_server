#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    const HEALTH_THRESHOLD: i32 = 30;
    const HEALTH_ADVANTAGE: i32 = 15;
    const SCORE_LEAD_THRESHOLD: i32 = 10;
    const AGGRESSION_CYCLE: i32 = 7;
    
    let health_difference = self_health - enemy_health;
    let is_healthy = self_health > HEALTH_THRESHOLD;
    let has_health_advantage = health_difference > HEALTH_ADVANTAGE;
    let has_score_lead = self_score > SCORE_LEAD_THRESHOLD;
    let is_critical = self_health < HEALTH_THRESHOLD / 2;
    
    let cycle_position = tick % AGGRESSION_CYCLE;
    
    if is_critical {
        return -1;
    }
    
    if has_score_lead {
        if !is_healthy {
            return -1;
        }
        
        if has_health_advantage {
            if cycle_position < 2 {
                return 1;
            }
            return 0;
        }
        
        if tick % 3 == 0 {
            return 0;
        }
        return -1;
    }
    
    if self_score < -SCORE_LEAD_THRESHOLD {
        if is_healthy {
            match tick % 5 {
                0 | 1 => return 1,
                2 => return 0,
                _ => return -1,
            }
        } else if self_health > HEALTH_THRESHOLD / 3 {
            if tick % 4 == 0 && health_difference > -HEALTH_ADVANTAGE {
                return 1;
            }
            return 0;
        } else {
            return -1;
        }
    }
    
    if is_healthy {
        if has_health_advantage {
            match (tick / 3) % 4 {
                0 => return 1,
                1 => return 0,
                2 => return 1,
                _ => return -1,
            }
        } else if health_difference < -HEALTH_ADVANTAGE {
            if tick % 6 < 2 {
                return 1;
            }
            return -1;
        } else {
            match tick % 8 {
                0 | 1 | 2 => return 0,
                3 | 4 => return 1,
                _ => return -1,
            }
        }
    } else {
        if enemy_health < self_health + 5 {
            if tick % 7 == 0 {
                return 1;
            }
            return -1;
        }
        return -1;
    }
}