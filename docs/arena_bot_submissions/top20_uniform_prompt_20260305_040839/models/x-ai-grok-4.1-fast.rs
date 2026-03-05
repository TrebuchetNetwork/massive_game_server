use std::f64::consts::PI;

#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    let sh = self_health.clamp(0, 100);
    let eh = enemy_health.clamp(0, 100);
    let score = self_score.clamp(0, 1000);

    // Opportunistic kill low health enemy if safe
    if eh < 20 && sh > 25 {
        return 1;
    }

    // Emergency retreat
    if sh < 15 {
        return -1;
    }

    // Preserve high score lead safely
    if score > 75 && sh > 60 {
        return 0;
    }

    // Take calculated risks when trailing
    if score < 15 {
        if eh < 50 || sh > 70 {
            return 1;
        } else {
            return 0;
        }
    }

    // Computed aggression
    let health_diff = sh - eh;
    let mut agg = health_diff / 10;

    // Score influence
    if score > 50 {
        agg -= 2;
    } else if score < 25 {
        agg += 3;
    }

    // Deterministic rhythm to avoid static patterns
    let phase = (tick % 72) as f64 / 72.0 * 2.0 * PI;
    let rhythm = (phase.sin() * 3.0) as i32;
    agg += rhythm;

    if agg > 1 {
        1
    } else if agg < -1 {
        -1
    } else {
        0
    }
}