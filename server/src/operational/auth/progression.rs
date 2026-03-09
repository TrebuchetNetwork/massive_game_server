use super::phone_utils::masked_phone_for_user;
use super::types::{AuthProfileView, UserRecord};
use crate::core::types::PlayerState;

use super::{PROGRESSION_BASE_CREDITS_PER_MATCH, PROGRESSION_BASE_XP_PER_MATCH};
use super::{PROGRESSION_CREDITS_PER_KILL, PROGRESSION_XP_PER_KILL};

pub(super) fn to_profile_view(user: &UserRecord) -> AuthProfileView {
    let level = level_from_experience(user.experience_points);
    let mmr = compute_mmr(
        user.total_kills,
        user.total_deaths,
        user.cumulative_score,
        user.matches_played,
    );
    let favorite_weapon = favorite_weapon_from_kills(&user.kills_per_weapon).to_owned();
    let lifetime_kd = user.total_kills as f32 / user.total_deaths.max(1) as f32;
    AuthProfileView {
        user_id: user.user_id.clone(),
        display_name: user.display_name.clone(),
        phone_masked: masked_phone_for_user(user),
        created_at: user.created_at,
        last_seen_at: user.last_seen_at,
        matches_played: user.matches_played,
        cumulative_score: user.cumulative_score,
        best_score: user.best_score,
        total_kills: user.total_kills,
        total_deaths: user.total_deaths,
        total_flag_captures: user.total_flag_captures,
        top_streak: user.top_streak,
        favorite_weapon,
        lifetime_kd,
        last_game_username: user.last_game_username.clone(),
        experience_points: user.experience_points,
        credits: user.credits,
        level,
        next_level_experience: experience_for_level(level.saturating_add(1)),
        mmr,
        mmr_band: classify_mmr_band(mmr).to_string(),
    }
}

pub(super) fn favorite_weapon_from_kills(kills_per_weapon: &[u64; 5]) -> &'static str {
    let mut best_idx: Option<usize> = None;
    let mut best_kills = 0u64;
    for (idx, kills) in kills_per_weapon.iter().enumerate() {
        if *kills > best_kills {
            best_kills = *kills;
            best_idx = Some(idx);
        }
    }
    match best_idx {
        Some(0) => "Pistol",
        Some(1) => "Shotgun",
        Some(2) => "Rifle",
        Some(3) => "Sniper",
        Some(4) => "Melee",
        _ => "None",
    }
}

pub(super) fn compute_mmr(
    total_kills: u64,
    total_deaths: u64,
    cumulative_score: i64,
    matches_played: u64,
) -> f32 {
    let kd = total_kills as f32 / total_deaths.max(1) as f32;
    let avg_score = cumulative_score.max(0) as f32 / matches_played.max(1) as f32;
    kd * 100.0 + avg_score * 0.5
}

fn classify_mmr_band(mmr: f32) -> &'static str {
    crate::scaling::router::classify_mmr_band(mmr)
}

pub(super) fn progression_reward_from_match(player_state: &PlayerState) -> (u64, u64) {
    let score = player_state.score.max(0) as u64;
    let kills = player_state.kills.max(0) as u64;
    let deaths = player_state.deaths.max(0) as u64;
    let score_xp = score / 2;
    let score_credits = score / 10;
    let performance_bonus_xp = if kills >= deaths && kills > 0 { 20 } else { 0 };
    let performance_bonus_credits = if kills >= deaths && kills > 0 { 10 } else { 0 };
    let xp_gain = PROGRESSION_BASE_XP_PER_MATCH
        .saturating_add(score_xp)
        .saturating_add(kills.saturating_mul(PROGRESSION_XP_PER_KILL))
        .saturating_add(performance_bonus_xp);
    let credits_gain = PROGRESSION_BASE_CREDITS_PER_MATCH
        .saturating_add(score_credits)
        .saturating_add(kills.saturating_mul(PROGRESSION_CREDITS_PER_KILL))
        .saturating_add(performance_bonus_credits);
    (xp_gain, credits_gain)
}

pub(super) fn experience_for_level(level: u32) -> u64 {
    if level <= 1 {
        return 0;
    }
    // Smoothly rising curve: sum_{i=1..level-1} (100 + 25*(i-1))
    let n = (level - 1) as u64;
    n.saturating_mul(100)
        .saturating_add(25u64.saturating_mul(n.saturating_sub(1)).saturating_mul(n) / 2)
}

pub(super) fn level_from_experience(experience_points: u64) -> u32 {
    let mut level = 1u32;
    loop {
        let next_level = level.saturating_add(1);
        let required = experience_for_level(next_level);
        if experience_points < required || next_level == u32::MAX {
            return level;
        }
        level = next_level;
    }
}
