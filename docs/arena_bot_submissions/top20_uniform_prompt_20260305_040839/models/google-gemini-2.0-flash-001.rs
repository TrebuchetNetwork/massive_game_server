#![allow(dead_code)]
#![allow(unused_variables)]

#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    let health_ratio = self_health as f32 / (self_health + enemy_health) as f32;
    let score_difference = self_score - enemy_health;

    let tick_mod = tick % 100;

    if self_health <= 10 {
        return -1; // Retreat when low health
    }

    if self_score > 100 && score_difference > 50 {
        if tick_mod < 20 {
            return 0; // Hold position to preserve lead
        } else {
            return -1; // Defend to preserve lead
        }
    }

    if score_difference < -50 {
        if tick_mod < 70 {
            return 1; // Aggressively push when trailing significantly
        } else {
            return 0; // Reposition to prepare for another push
        }
    }

    if health_ratio > 0.7 {
        if tick_mod < 60 {
            return 1; // Push when healthy
        } else {
            return 0; // Reposition
        }
    }

    if health_ratio < 0.3 {
        if tick_mod < 30 {
            return -1; // Retreat when low health relative to enemy
        } else {
            return 0; // Reposition
        }
    }

    if tick_mod < 50 {
        return 0; // Hold position as default
    } else if tick_mod < 75 {
        return 1; // Push moderately
    } else {
        return -1; // Defend moderately
    }
}