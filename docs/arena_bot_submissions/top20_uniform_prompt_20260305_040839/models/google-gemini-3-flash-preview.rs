#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    let health_advantage = self_health - enemy_health;
    let is_low_health = self_health < 30;
    let is_critical = self_health < 15;
    
    let score_lead = self_score > 50;
    let trailing_badly = self_score < -20;

    let rhythm = tick % 10;
    let is_pulse_active = rhythm < 4;

    if is_critical {
        return -1;
    }

    if score_lead {
        if is_low_health {
            return -1;
        }
        if health_advantage > 20 {
            return if is_pulse_active { 1 } else { 0 };
        }
        return if is_pulse_active { 0 } else { -1 };
    }

    if trailing_badly {
        if self_health > 40 {
            return 1;
        }
        return if is_pulse_active { 1 } else { 0 };
    }

    if health_advantage > 10 {
        return 1;
    }

    if is_low_health {
        return -1;
    }

    if rhythm < 3 {
        1
    } else if rhythm < 7 {
        0
    } else {
        -1
    }
}