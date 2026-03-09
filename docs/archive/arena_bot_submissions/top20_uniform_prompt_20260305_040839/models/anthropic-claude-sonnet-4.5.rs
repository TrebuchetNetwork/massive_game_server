#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    let score_diff = self_score;
    let health_diff = self_health - enemy_health;
    
    let critical_health = 25;
    let low_health = 40;
    let comfortable_health = 60;
    
    let big_lead = 15;
    let small_lead = 5;
    
    let phase = (tick / 20) % 5;
    let micro_cycle = tick % 10;
    
    if self_health <= critical_health {
        if score_diff > big_lead && enemy_health < low_health {
            if micro_cycle < 3 {
                return 0;
            } else {
                return -1;
            }
        }
        return -1;
    }
    
    if score_diff > big_lead {
        if self_health > comfortable_health {
            if enemy_health < critical_health {
                return 1;
            }
            if health_diff > 20 {
                if phase < 3 {
                    return 1;
                } else {
                    return 0;
                }
            }
            if micro_cycle < 4 {
                return 0;
            } else if micro_cycle < 7 {
                return -1;
            } else {
                return 0;
            }
        } else if self_health > low_health {
            if enemy_health < critical_health && health_diff > 0 {
                return 1;
            }
            if micro_cycle < 6 {
                return 0;
            } else {
                return -1;
            }
        } else {
            return -1;
        }
    }
    
    if score_diff > small_lead {
        if self_health > comfortable_health {
            if health_diff > 15 {
                return 1;
            } else if health_diff > 0 {
                if phase < 2 {
                    return 1;
                } else if phase < 4 {
                    return 0;
                } else {
                    return -1;
                }
            } else {
                if micro_cycle < 5 {
                    return 0;
                } else {
                    return -1;
                }
            }
        } else if self_health > low_health {
            if health_diff > 20 {
                return 1;
            } else if health_diff > 5 {
                return 0;
            } else {
                return -1;
            }
        } else {
            if enemy_health < critical_health && health_diff > -10 {
                return 1