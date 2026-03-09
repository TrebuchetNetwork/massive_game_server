use crate::core::types::{PlayerAoIs, PlayerID};
use crate::network::connection_manager::shared_connection_manager;
use crate::operational::auth::AuthService;
use crate::operational::monitoring::metrics;
use std::sync::Arc;
use tracing::{debug, error, info};

use super::chat::clear_chat_rate_limit;
use super::client_state::ClientStatesMap;
use super::webrtc_state::remove_webrtc_peer_state;
use super::{DataChannelsMap, PlayerManagerRef, SignalingPeers};

pub fn cleanup_connection(
    peer_id_str: &str,
    signaling_peers: &SignalingPeers,
    player_manager: &PlayerManagerRef, // This is Arc<ImprovedPlayerManager>
    data_channels_map: &DataChannelsMap,
    client_states_map: &ClientStatesMap,
    player_aois: &PlayerAoIs,
    auth_service: &AuthService,
) {
    info!("[{}]: Cleaning up resources.", peer_id_str);
    clear_chat_rate_limit(peer_id_str);
    remove_webrtc_peer_state(peer_id_str);
    let _ = shared_connection_manager().remove(peer_id_str);
    // Remove signaling sender first; duplicate cleanups are expected under concurrent callbacks.
    let removed_signaling_entry = signaling_peers.remove(peer_id_str).is_some();
    metrics::set_ws_connections_active(signaling_peers.len());
    // Maintain a single lock acquisition order for lifecycle paths:
    // client state first, then player manager operations.
    client_states_map.write().remove(peer_id_str);
    data_channels_map.remove(peer_id_str);
    player_aois.remove(peer_id_str);

    let player_state_snapshot = {
        let player_id_lookup: PlayerID = Arc::from(peer_id_str.to_owned());
        player_manager
            .get_player_state(&player_id_lookup)
            .map(|entry| entry.clone())
    };

    if removed_signaling_entry {
        if let Some(player_state) = player_state_snapshot.as_ref() {
            auth_service.record_disconnect_score_for_peer(peer_id_str, player_state);
        } else {
            auth_service.clear_peer_binding(peer_id_str);
        }
    } else {
        debug!(
            "[{}]: Signaling sender already removed; continuing idempotent cleanup.",
            peer_id_str
        );
        if player_state_snapshot.is_none() {
            auth_service.clear_peer_binding(peer_id_str);
        }
    }

    if player_state_snapshot.is_some() {
        player_manager.remove_player(peer_id_str);
    }
    info!("[{}]: Player AoI data removed.", peer_id_str);
}

pub fn handle_dc_send_error(error_string: &str, peer_id_str: &str, message_type: &str) {
    let is_stream_closed_error = error_string.contains("stream closed")
        || error_string.contains("Stream closed")
        || error_string.contains("connection reset")
        || error_string.contains("Channel closed");

    if !is_stream_closed_error {
        error!(
            "[{}]: Error sending {} on data channel: {}",
            peer_id_str, message_type, error_string
        );
    }
}
