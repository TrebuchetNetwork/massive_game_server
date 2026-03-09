#[no_mangle]
pub extern "C" fn bot_tick(self_health: i32, enemy_health: i32, self_score: i32, tick: i32) -> i32 {
    let health_advantage = self_health - enemy_health;
    let score_lead = self_score;

    let self_hp = if self_health > 100 { 100 } else { self_health };
    let enemy_hp = if enemy_health > 100 { 100 } else { enemy_health };

    let phase = tick % 60;
    let micro = tick % 7;

    let rhythm: i32 = match micro {
        1 | 4 => 1,
        3 | 6 => -1,
        _ => 0,
    };

    // Critical health: almost always retreat
    if self_hp <= 10 {
        return -1;
    }

    // Low health: retreat unless enemy is nearly dead
    if self_hp <= 20 {
        if enemy_hp <= 15 {
            return 1;
        }
        if phase % 3 == 0 {
            return 0;
        }
        return -1;
    }

    // Leading by a lot: play conservatively to preserve lead
    if score_lead > 5 {
        if self_hp >= 60 && health_advantage > 20 && phase < 20 {
            return 1;
        }
        if self_hp >= 60 {
            return if rhythm >= 0 { 0 } else { -1 };
        }
        if self_hp >= 35 && health_advantage > 0 {
            return 0;
        }
        return -1;
    }

    // Trailing badly: take calculated risks
    if score_lead < -5 {
        if self_hp >= 50 && (enemy_hp <= 30 || health_advantage > 10) {
            return 1;
        }
        if self_hp >= 50 && phase < 30 {
            return 1;
        }
        if self_hp >= 50 {
            return if rhythm > 0 { 1 } else { 0 };
        }
        if self_hp >= 30 && (health_advantage > 20 || enemy_hp <= 20) {
            return 1;
        }
        if self_hp >= 30 && phase < 15 {
            return 1;
        }
        if self_hp >= 30 {
            return 0;
        }
        if enemy_hp <= 10 {
            return 1;
        }
        return -1;
    }

    // Score is roughly even: balanced play based on health and rhythm
    if self_hp >= 70 && enemy_hp >= 70 {
        if phase < 20 {