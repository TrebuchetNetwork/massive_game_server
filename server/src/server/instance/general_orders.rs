//! Applying a general's mass call to the live match.
//!
//! See [`crate::systems::ai::generals`] for the order model. This is the
//! server-side edge: validate, store, push the team's waypoint onto the
//! existing commander channel that the bot AI already follows, and announce
//! the call to everyone watching.

use super::*;

impl MassiveGameServer {
    /// Validate and apply one general's order. Returns the stored order so
    /// the caller can echo it back to the worker.
    pub fn apply_general_order(
        &self,
        request: GeneralOrderRequest,
    ) -> Result<GeneralOrder, GeneralOrderError> {
        let now_ms = self.get_server_timestamp_ms();
        let sequence = self
            .general_order_sequence
            .fetch_add(1, AtomicOrdering::Relaxed)
            .saturating_add(1);
        let order = validate_order(
            &request,
            now_ms,
            sequence,
            Vec2::new(WORLD_MIN_X, WORLD_MIN_Y),
            Vec2::new(WORLD_MAX_X, WORLD_MAX_Y),
        )?;

        {
            let mut board = self.general_orders.write();
            board.prune(now_ms);
            board.set(order.clone());
        }

        // `Hold` deliberately issues no waypoint: it releases the team from
        // the previous call rather than pinning it in place.
        if order.posture.moves_the_team() {
            let commander_id = self.player_manager.id_pool.get_or_create("general");
            self.register_commander_waypoint(&commander_id, order.team_id, order.target(), now_ms);
        }

        info!(
            team_id = order.team_id,
            posture = order.posture.as_str(),
            model_id = order.model_id.as_str(),
            target_x = order.target_x,
            target_y = order.target_y,
            "General issued a mass call"
        );

        if let Some(packet) = self.build_system_event_packet("general_order", Some(&order)) {
            self.enqueue_direct_packet_for_all_players(packet);
        }

        Ok(order)
    }

    /// Orders still standing, for the public scoreboard and the client HUD.
    pub fn active_general_orders(&self) -> Vec<GeneralOrder> {
        let now_ms = self.get_server_timestamp_ms();
        self.general_orders.read().active(now_ms)
    }

    /// The standing order for one team, if any. Read on the AI path, so this
    /// takes the read lock only and never mutates.
    pub(crate) fn general_order_for_team(&self, team_id: u8) -> Option<GeneralOrder> {
        let now_ms = self.get_server_timestamp_ms();
        self.general_orders
            .read()
            .active_for_team(team_id, now_ms)
            .cloned()
    }
}
