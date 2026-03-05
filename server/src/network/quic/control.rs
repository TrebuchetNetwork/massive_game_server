use crate::core::types::PlayerInputData;
use crate::network::quic::{quic_outbound_mode_name, QuicRequestHandler};
use crate::operational::auth::AuthService;
use crate::operational::monitoring::tracing as monitoring_tracing;
use crate::server::instance::{LiveReplayDisputeRequest, MassiveGameServer};

use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone, Default, Deserialize)]
struct QuicControlRequest {
    op: Option<String>,
    peer_id: Option<String>,
    auth_token: Option<String>,
    username: Option<String>,
    team_id: Option<u8>,
    replay_limit: Option<usize>,
    from_frame: Option<u64>,
    to_frame: Option<u64>,
    player_id: Option<String>,
    input: Option<QuicInputEnvelope>,
    inputs: Option<Vec<QuicInputEnvelope>>,
}

#[derive(Clone, Default, Deserialize)]
struct QuicInputEnvelope {
    timestamp: Option<u64>,
    sequence: Option<u32>,
    move_forward: Option<bool>,
    move_backward: Option<bool>,
    move_left: Option<bool>,
    move_right: Option<bool>,
    shooting: Option<bool>,
    reload: Option<bool>,
    rotation: Option<f32>,
    melee_attack: Option<bool>,
    change_weapon_slot: Option<u8>,
    use_ability_slot: Option<u8>,
    ping_x: Option<f32>,
    ping_y: Option<f32>,
}

impl QuicInputEnvelope {
    fn into_player_input(self) -> PlayerInputData {
        PlayerInputData {
            timestamp: self.timestamp.unwrap_or_default(),
            sequence: self.sequence.unwrap_or_default(),
            move_forward: self.move_forward.unwrap_or(false),
            move_backward: self.move_backward.unwrap_or(false),
            move_left: self.move_left.unwrap_or(false),
            move_right: self.move_right.unwrap_or(false),
            shooting: self.shooting.unwrap_or(false),
            reload: self.reload.unwrap_or(false),
            rotation: self.rotation.unwrap_or(0.0),
            melee_attack: self.melee_attack.unwrap_or(false),
            change_weapon_slot: self.change_weapon_slot.unwrap_or(0),
            use_ability_slot: self.use_ability_slot.unwrap_or(0),
            ping_x: self.ping_x.unwrap_or(0.0),
            ping_y: self.ping_y.unwrap_or(0.0),
        }
    }
}

fn serialize_quic_response<T: Serialize>(payload: &T, op_name: &str) -> Option<Vec<u8>> {
    match serde_json::to_vec(payload) {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            error!(
                "Failed to serialize QUIC response for op '{}': {}",
                op_name, err
            );
            Some(br#"{"ok":false,"op":"internal_error","error":"serialization_failed"}"#.to_vec())
        }
    }
}

pub fn build_quic_control_handler(
    server: Arc<MassiveGameServer>,
    auth_service: AuthService,
) -> QuicRequestHandler {
    Arc::new(move |payload: &[u8], bound_peer_id: Option<&str>| {
        let request = match serde_json::from_slice::<QuicControlRequest>(payload) {
            Ok(request) => request,
            Err(err) => {
                warn!("QUIC control request parse failed: {}", err);
                return serialize_quic_response(
                    &serde_json::json!({
                        "ok": false,
                        "op": "invalid",
                        "error": "invalid_json",
                    }),
                    "invalid",
                );
            }
        };
        let op = request.op.unwrap_or_else(|| "echo".to_string());

        let response = match op.as_str() {
            "healthz" => serde_json::json!({
                "ok": true,
                "op": "healthz",
                "frame": server.frame_counter.load(Ordering::Relaxed),
                "players": server.player_manager.player_count(),
                "projectiles": server.projectiles.read().len(),
                "pickups": server.pickups.read().len(),
                "ts_ms": server.get_server_timestamp_ms(),
            }),
            "live_replay_recent" => {
                if bound_peer_id.is_none() {
                    serde_json::json!({ "ok": false, "op": "live_replay_recent", "error": "unauthorized" })
                } else {
                    let limit = request.replay_limit.unwrap_or(128).clamp(1, 4096);
                    serde_json::json!({
                        "ok": true,
                        "op": "live_replay_recent",
                        "frames": server.recent_live_replay_frames(limit),
                        "limit": limit,
                    })
                }
            }
            "live_replay_disputes_recent" => {
                if bound_peer_id.is_none() {
                    serde_json::json!({ "ok": false, "op": "live_replay_disputes_recent", "error": "unauthorized" })
                } else {
                    let limit = request.replay_limit.unwrap_or(128).clamp(1, 2048);
                    serde_json::json!({
                        "ok": true,
                        "op": "live_replay_disputes_recent",
                        "audits": server.recent_live_replay_dispute_audits(limit),
                        "limit": limit,
                    })
                }
            }
            "live_replay_dispute" => {
                if bound_peer_id.is_none() {
                    return serialize_quic_response(
                        &serde_json::json!({
                            "ok": false,
                            "op": "live_replay_dispute",
                            "error": "unauthorized"
                        }),
                        "live_replay_dispute",
                    );
                }
                let report = server.build_live_replay_dispute_report(LiveReplayDisputeRequest {
                    from_frame: request.from_frame,
                    to_frame: request.to_frame,
                    limit: request.replay_limit,
                    player_id: request.player_id,
                });
                return serialize_quic_response(&report, "live_replay_dispute");
            }
            "join" => {
                if bound_peer_id.is_some() {
                    return serialize_quic_response(
                        &serde_json::json!({
                            "ok": false,
                            "op": "join",
                            "error": "already_joined",
                            "peer_id": bound_peer_id,
                        }),
                        "join",
                    );
                }

                let auth_token = request
                    .auth_token
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty());

                let Some(token) = auth_token else {
                    warn!("QUIC join rejected: missing auth_token");
                    return serialize_quic_response(
                        &serde_json::json!({
                            "ok": false,
                            "op": "join",
                            "error": "auth_required",
                        }),
                        "join",
                    );
                };

                let Some(user_id) = auth_service.resolve_user_id_from_token(token) else {
                    warn!("QUIC join rejected: invalid or expired auth_token");
                    return serialize_quic_response(
                        &serde_json::json!({
                            "ok": false,
                            "op": "join",
                            "error": "auth_invalid",
                        }),
                        "join",
                    );
                };

                let mut peer_id = None;
                if let Some(client_peer) = request
                    .peer_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                {
                    if let Some(bound_user) = auth_service.resolve_user_id_from_peer(client_peer) {
                        if bound_user == user_id {
                            peer_id = Some(client_peer.to_string());
                        } else {
                            warn!(
                                    "QUIC join rejected client peer_id '{}' because it is bound to a different user",
                                    client_peer
                                );
                        }
                    }
                }
                let peer_id = peer_id.unwrap_or_else(|| Uuid::new_v4().to_string());

                auth_service.bind_peer_to_user(&peer_id, &user_id);

                let joined = server.register_quic_player(
                    &peer_id,
                    request.username.as_deref(),
                    request.team_id,
                );

                info!(
                    "QUIC join: user_id='{}' peer_id='{}' success={}",
                    user_id,
                    peer_id,
                    joined.is_some()
                );

                serde_json::json!({
                    "ok": joined.is_some(),
                    "op": "join",
                    "player": joined,
                    "peer_id": peer_id,
                    "quic_outbound_mode": quic_outbound_mode_name(),
                    "_bound_peer_id": peer_id,
                })
            }
            "input" => {
                let Some(peer_id) = bound_peer_id else {
                    return serialize_quic_response(
                        &serde_json::json!({
                            "ok": false,
                            "op": "input",
                            "error": "not_authenticated",
                        }),
                        "input",
                    );
                };

                let mut inputs = Vec::new();
                if let Some(single_input) = request.input {
                    inputs.push(single_input.into_player_input());
                }
                if let Some(batch_inputs) = request.inputs {
                    for input in batch_inputs.into_iter().take(128) {
                        inputs.push(input.into_player_input());
                    }
                }

                let mut accepted = 0usize;
                for input in inputs {
                    if server.enqueue_quic_input(peer_id, input) {
                        accepted += 1;
                    } else {
                        break;
                    }
                }

                serde_json::json!({
                    "ok": accepted > 0,
                    "op": "input",
                    "accepted": accepted,
                    "peer_id": peer_id,
                })
            }
            "leave" | "disconnect" => {
                if let Some(peer_id) = bound_peer_id {
                    server.remove_quic_player(peer_id);
                    auth_service.clear_peer_binding(peer_id);
                    serde_json::json!({
                        "ok": true,
                        "op": "leave",
                        "peer_id": peer_id,
                    })
                } else {
                    serde_json::json!({
                        "ok": false,
                        "op": "leave",
                        "error": "not_authenticated",
                    })
                }
            }
            _ => serde_json::json!({
                "ok": true,
                "op": "echo",
                "bytes": payload.len(),
                "trace_headers": monitoring_tracing::inject_current_context_headers(),
            }),
        };

        serialize_quic_response(&response, op.as_str())
    })
}
