//! Champion duel ("Beat the champion") on-demand mode.
//!
//! A visitor landing on `client.html?match_type=champion_duel` is registered
//! as a duel challenger by the signaling layer. While at least one challenger
//! is connected, the instance reconfigures at the next match boundary into a
//! 1v1: every non-spectator human on team 1 against a single exhibition bot
//! running the published roster's top fighter (the champion) on team 2, with
//! 3-minute TDM rounds and no other bot auto-fill. When the last challenger
//! disconnects, the instance reverts to its configured rules (gauntlet or
//! league rotation) at the next boundary, mirroring the on-demand gauntlet.
//!
//! A dedicated instance (`MGS_MATCH_TYPE=champion_duel`) runs the same rules
//! unconditionally through `MatchType::ChampionDuel`.

use super::*;

/// Exactly one bot fields the champion's fighter; no other bots autofill.
pub(crate) const DUEL_BOT_TARGET: u64 = 1;
/// When the first challenger joins an otherwise bot-only match, wind the
/// running round down to this many seconds so the duel forms promptly
/// instead of waiting out a full round.
pub(crate) const DUEL_FORM_WINDDOWN_SECS: f32 = 8.0;
/// A challenger is registered at the WebSocket upgrade, before the WebRTC
/// data channel (and its player entity) exists. If negotiation never
/// completes, no disconnect hook fires, so entries that never materialized
/// are pruned at match boundaries once they are older than this grace
/// window (longer than the idle-connection eviction threshold).
pub(crate) const DUEL_CHALLENGER_GRACE: Duration = Duration::from_secs(150);

impl MassiveGameServer {
    /// Register a connected peer as a champion-duel challenger.
    pub fn note_duel_challenger(&self, peer_id: &str) {
        self.duel_challengers
            .insert(peer_id.to_owned(), Instant::now());
        info!("Champion duel challenger connected: {}", peer_id);
        if self.duel_rules_active() {
            return;
        }
        // Nobody else is playing: cut the running bot-only round short so the
        // duel forms at the upcoming boundary within seconds. With other
        // humans in the match we leave their round alone and the duel forms
        // at the next natural boundary instead.
        if self.non_challenger_human_count() == 0 {
            let mut match_info_guard = self.match_info.write();
            if match_info_guard.match_state == fb::MatchStateType::Active
                && match_info_guard.time_remaining > DUEL_FORM_WINDDOWN_SECS
            {
                info!(
                    "Winding down the current round ({:.0}s -> {:.0}s) to form the champion duel",
                    match_info_guard.time_remaining, DUEL_FORM_WINDDOWN_SECS
                );
                match_info_guard.time_remaining = DUEL_FORM_WINDDOWN_SECS;
            }
        }
    }

    /// Drop a challenger (disconnect / cleanup paths all funnel here).
    pub fn remove_duel_challenger(&self, peer_id: &str) {
        if self.duel_challengers.remove(peer_id).is_some() {
            info!("Champion duel challenger disconnected: {}", peer_id);
        }
    }

    /// Live challenger count, pruning entries that never materialized into a
    /// player/data channel past the join grace window (dead WebRTC attempts
    /// otherwise pin the duel on forever).
    pub fn duel_challenger_count(&self) -> usize {
        let now = Instant::now();
        let mut stale = Vec::new();
        let mut live = 0usize;
        for entry in self.duel_challengers.iter() {
            let peer_id = entry.key();
            let player_id: PlayerID = Arc::from(peer_id.as_str());
            let materialized = self.player_manager.get_player_state(&player_id).is_some()
                || self.data_channels_map.contains_key(peer_id.as_str());
            if materialized || now.duration_since(*entry.value()) < DUEL_CHALLENGER_GRACE {
                live += 1;
            } else {
                stale.push(peer_id.clone());
            }
        }
        for peer_id in stale {
            self.duel_challengers.remove(&peer_id);
        }
        live
    }

    /// On-demand duel rules are in force for the current/next match.
    pub fn duel_active(&self) -> bool {
        self.duel_active.load(AtomicOrdering::Relaxed)
    }

    /// Duel rules either because this is a dedicated duel instance or because
    /// the on-demand duel is active.
    pub fn duel_rules_active(&self) -> bool {
        self.match_type == MatchType::ChampionDuel || self.duel_active()
    }

    /// Round length for the match about to start: short blitz rounds under
    /// duel rules, the configured duration otherwise.
    pub fn effective_round_duration_secs(&self) -> f32 {
        if self.duel_rules_active() {
            CHAMPION_DUEL_DURATION_SECS
        } else {
            self.match_duration_secs
        }
    }

    /// Connected non-spectator humans who did not ask for the duel.
    fn non_challenger_human_count(&self) -> usize {
        let mut count = 0usize;
        self.player_manager.for_each_player(|player_id, state| {
            if state.is_spectator || self.bot_players.contains_key(player_id) {
                return;
            }
            if !self.duel_challengers.contains_key(player_id.as_ref()) {
                count += 1;
            }
        });
        count
    }

    /// Move every non-spectator human onto team 1 so the champion (team 2)
    /// always opposes them. Used when the duel forms and when a duel round
    /// starts on a dedicated instance.
    pub(super) fn move_humans_to_duel_side(&self) {
        let mut humans_to_move: Vec<PlayerID> = Vec::new();
        self.player_manager.for_each_player(|player_id, state| {
            if !state.is_spectator
                && !self.bot_players.contains_key(player_id)
                && state.team_id != 1
            {
                humans_to_move.push(player_id.clone());
            }
        });
        for player_id in humans_to_move {
            let spawn = self
                .respawn_manager
                .get_respawn_position(self, &player_id, Some(1), &[]);
            if let Some(mut state) = self.player_manager.get_player_state_mut(&player_id) {
                state.team_id = 1;
                state.is_carrying_flag_team_id = 0;
                state.respawn(spawn.x, spawn.y);
                state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG | FIELD_MISC);
            }
        }
    }

    /// Decide at a match boundary whether the next match runs under duel
    /// rules (a challenger is connected) and rebuild the bot population to
    /// match. The champion bot is spawned synchronously here, while the round
    /// is still Waiting, so `attach_champion_bot` commits against the
    /// committed exhibition generation instead of queuing behind a live
    /// round. Must be called without the match info lock held, before
    /// `sync_gauntlet_activation` (the duel takes precedence).
    pub(super) fn sync_duel_activation(&self) {
        // A dedicated duel instance never stands down.
        if self.match_type == MatchType::ChampionDuel {
            return;
        }
        let want_active = self.duel_challenger_count() > 0;
        let was_active = self.duel_active.swap(want_active, AtomicOrdering::Relaxed);
        if was_active == want_active {
            return;
        }

        if want_active {
            info!(
                "Champion duel forming: {} challenger(s) present -> 1 exhibition champion vs the humans",
                self.duel_challenger_count()
            );
            self.duel_saved_bot_target.store(
                self.target_bot_count.load(AtomicOrdering::Relaxed),
                AtomicOrdering::Relaxed,
            );
            self.target_bot_count
                .store(DUEL_BOT_TARGET, AtomicOrdering::Relaxed);
            self.move_humans_to_duel_side();
            let bot_count = self.bot_players.len();
            self.remove_bots(bot_count);
            // Spawn the champion immediately: still between rounds, so the
            // exhibition runtime attaches now rather than after a full round.
            self.spawn_additional_bots(DUEL_BOT_TARGET as usize);
        } else {
            info!("Champion duel standing down: no challengers connected");
            let saved = self.duel_saved_bot_target.load(AtomicOrdering::Relaxed);
            self.target_bot_count.store(saved, AtomicOrdering::Relaxed);
            let bot_count = self.bot_players.len();
            self.remove_bots(bot_count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duel_bot_target_is_exactly_one() {
        assert_eq!(DUEL_BOT_TARGET, 1);
    }

    #[test]
    fn winddown_is_shorter_than_a_blitz_round() {
        assert!(DUEL_FORM_WINDDOWN_SECS < CHAMPION_DUEL_DURATION_SECS);
    }
}
