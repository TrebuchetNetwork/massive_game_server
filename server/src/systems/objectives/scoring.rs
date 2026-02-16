// massive_game_server/server/src/systems/objectives/scoring.rs

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct ScoreBoard {
    team_scores: HashMap<u8, i32>,
}

impl ScoreBoard {
    pub fn add_points(&mut self, team_id: u8, points: i32) {
        let entry = self.team_scores.entry(team_id).or_insert(0);
        *entry += points;
    }

    pub fn score_for(&self, team_id: u8) -> i32 {
        self.team_scores.get(&team_id).copied().unwrap_or(0)
    }

    pub fn snapshot(&self) -> HashMap<u8, i32> {
        self.team_scores.clone()
    }
}
