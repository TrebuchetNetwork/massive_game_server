// massive_game_server/server/src/systems/objectives/scoring.rs

use dashmap::DashMap;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct ScoreBoard {
    team_scores: DashMap<u8, i32>,
}

impl ScoreBoard {
    pub fn add_points(&self, team_id: u8, points: i32) {
        self.team_scores
            .entry(team_id)
            .and_modify(|score| *score = score.saturating_add(points))
            .or_insert(points);
    }

    pub fn score_for(&self, team_id: u8) -> i32 {
        self.team_scores
            .get(&team_id)
            .map(|score| *score)
            .unwrap_or(0)
    }

    pub fn snapshot(&self) -> HashMap<u8, i32> {
        self.team_scores
            .iter()
            .map(|entry| (*entry.key(), *entry.value()))
            .collect()
    }
}
