#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    let health_advantage = self_health - enemy_health;
    let score_lead = self_score;

    let phase = tick % 16;
    let rhythm_offset: i32 = if phase < 4 {
        1
    } else if phase < 8 {
        0
    } else if phase < 12 {
        1
    } else {
        -1
    };

    if self_health <= 15 {
        return -1;
    }

    if self_health <= 30 {
        if health_advantage > 20 {
            return 0;
        }
        return -1;
    }

    if score_lead >= 50 {
        if self_health < 40 {
            return -1;
        }
        if health_advantage >= 0 {
            return 0;
        }
        return -1;
    }

    if score_lead >= 25 {
        if self_health < 50 {
            if health_advantage > 10 {
                return 0;
            }
            return -1;
        }
        if health_advantage > 15 {
            return 1;
        }
        if health_advantage >= -10 {
            return 0;
        }
        return -1;
    }

    if score_lead <= -30 {
        if self_health >= 60 {
            return 1;
        }
        if self_health >= 40 && health_advantage >= -5 {
            return 1;
        }
        if self_health >= 35 {
            return 0;
        }
        return -1;
    }

    if score_lead <= -10 {
        if self_health >= 70 {
            return 1;
        }
        if self_health >= 50 && health_advantage >= 0 {
            return 1;
        }
        if self_health >= 40 {
            return 0;
        }
        return -1;
    }

    if health_advantage > 30 {
        return 1;
    }

    if health_advantage > 15 {
        if self_health >= 50 {
            return 1;
        }
        return 0;
    }

    if health_advantage >= 0 {
        if self_health >= 60 {
            if rhythm_offset > 0 {
                return 1;
            }
            return 0;
        }
        if self_health >= 40 {
            return 0;
        }
        return -1;
    }

    if health_advantage >= -15 {
        if self_health >=