use super::types::{ArenaModelRecord, ArenaModelView, QueuedMatch, QueuedMatchView};
use crate::operational::bot_sandbox::ArenaMatchMode;
use std::sync::atomic::{AtomicU64, Ordering};

// ── Match result ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MatchResult {
    Win,
    Loss,
    Draw,
}

impl MatchResult {
    pub(super) fn score(self) -> f64 {
        match self {
            MatchResult::Win => 1.0,
            MatchResult::Loss => 0.0,
            MatchResult::Draw => 0.5,
        }
    }

    pub(super) fn inverse(self) -> MatchResult {
        match self {
            MatchResult::Win => MatchResult::Loss,
            MatchResult::Loss => MatchResult::Win,
            MatchResult::Draw => MatchResult::Draw,
        }
    }
}

// ── Elo calculation ─────────────────────────────────────────────────────────

pub(super) fn update_elo_pair(
    elo_a: f64,
    elo_b: f64,
    result_a: MatchResult,
    result_b: MatchResult,
) -> (f64, f64) {
    let expected_a = 1.0 / (1.0 + 10.0f64.powf((elo_b - elo_a) / 400.0));
    let expected_b = 1.0 / (1.0 + 10.0f64.powf((elo_a - elo_b) / 400.0));
    let k = 32.0;
    let updated_a = (elo_a + k * (result_a.score() - expected_a)).clamp(100.0, 4000.0);
    let updated_b = (elo_b + k * (result_b.score() - expected_b)).clamp(100.0, 4000.0);
    (updated_a, updated_b)
}

pub(super) fn apply_match_result(
    model: &mut ArenaModelRecord,
    result: MatchResult,
    score: i32,
    completed_at: u64,
) {
    model.matches_played = model.matches_played.saturating_add(1);
    match result {
        MatchResult::Win => model.wins = model.wins.saturating_add(1),
        MatchResult::Loss => model.losses = model.losses.saturating_add(1),
        MatchResult::Draw => model.draws = model.draws.saturating_add(1),
    }
    model.cumulative_score = model.cumulative_score.saturating_add(score as i64);
    model.updated_at = completed_at;
    model.last_seen_at = completed_at;
}

// ── View mapping ────────────────────────────────────────────────────────────

pub(super) fn to_model_view(model: &ArenaModelRecord) -> ArenaModelView {
    let win_rate = if model.matches_played == 0 {
        0.0
    } else {
        model.wins as f64 / model.matches_played as f64
    };

    ArenaModelView {
        model_id: model.model_id.clone(),
        model_name: model.model_name.clone(),
        provider: model.provider.clone(),
        version: model.version.clone(),
        active: model.active,
        elo_rating: model.elo_rating,
        matches_played: model.matches_played,
        wins: model.wins,
        losses: model.losses,
        draws: model.draws,
        cumulative_score: model.cumulative_score,
        win_rate,
        created_at: model.created_at,
        updated_at: model.updated_at,
        last_seen_at: model.last_seen_at,
    }
}

pub(super) fn to_queued_match_view(entry: &QueuedMatch) -> QueuedMatchView {
    QueuedMatchView {
        match_id: entry.match_id.clone(),
        model_a_id: entry.model_a_id.clone(),
        model_b_id: entry.model_b_id.clone(),
        mode: entry.mode.clone(),
        queued_at: entry.queued_at,
    }
}

pub(super) fn normalize_match_mode(
    raw_mode: Option<&str>,
) -> Result<String, super::types::ArenaError> {
    use super::types::ArenaError;
    let value = raw_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("arena");
    let Some(mode) = ArenaMatchMode::parse(value) else {
        return Err(ArenaError::InvalidInput(
            "invalid_mode",
            format!(
                "unsupported mode '{}'; expected one of: arena, ctf, koth, tdm",
                value
            ),
        ));
    };
    Ok(mode.as_str().to_owned())
}

// ── Utility helpers ─────────────────────────────────────────────────────────

pub(super) fn safe_average(total: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

pub(super) fn atomic_update_max(counter: &AtomicU64, value: u64) {
    loop {
        let current = counter.load(Ordering::Relaxed);
        if value <= current {
            return;
        }
        if counter
            .compare_exchange(current, value, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }
    }
}

pub(super) fn atomic_update_min_nonzero(counter: &AtomicU64, value: u64) {
    if value == 0 {
        return;
    }
    loop {
        let current = counter.load(Ordering::Relaxed);
        if current != 0 && value >= current {
            return;
        }
        if counter
            .compare_exchange(current, value, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }
    }
}

pub(super) fn unix_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
