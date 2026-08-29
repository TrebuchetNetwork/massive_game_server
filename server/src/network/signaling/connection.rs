use crate::core::config::ServerConfig;
use crate::core::constants::*;
use crate::core::types::{
    PlayerAoIs, PlayerID, PlayerInputData, FIELD_FLAG, FIELD_MISC, FIELD_SCORE_STATS,
};
use crate::flatbuffers_generated::game_protocol as fb;
use crate::network::connection_manager::{
    shared_connection_manager, ConnectionInfo, TransportKind,
};
use crate::operational::auth::AuthService;
use crate::operational::monitoring::metrics;
use futures_util::{SinkExt, StreamExt};
use std::{
    net::IpAddr,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio::time::MissedTickBehavior;
use tracing::{debug, error, info, trace, warn};
use warp::ws::{Message, WebSocket};
use webrtc::{
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    ice_transport::{ice_candidate::RTCIceCandidate, ice_candidate::RTCIceCandidateInit},
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
    },
};

use super::chat::{
    next_chat_message_seq, try_consume_chat_rate_limit, ChatMessage, MAX_CHAT_MESSAGE_CHARS,
    MAX_CHAT_USERNAME_CHARS,
};
use super::cleanup::{cleanup_connection, handle_dc_send_error};
use super::client_state::{ClientState, ClientStatesMap};
use super::ice_config::{build_client_ice_config, build_ice_servers};
use super::rate_limiting::{
    acquire_sdp_admission_permit, ice_candidate_rate_limit_config, input_rate_limit_config,
    signaling_error_json, try_acquire_ip_rate_limit_token, try_acquire_join_rate_limit_token,
    try_queue_signaling_message, validate_signaling_payload, InputRateLimiter, IpConnectionGuard,
    RTCIceCandidateInitSerde, SignalingMessageJson, DISCONNECTED_CLEANUP_GRACE_SECS,
    JOIN_RATE_LIMIT_THROTTLED_MESSAGE, MAX_DATACHANNEL_MESSAGE_BYTES, MAX_SIGNALING_TEXT_BYTES,
    SIGNALING_OUTBOX_CAPACITY,
};
use super::sanitization::{
    build_welcome_message_bytes, now_millis, sanitize_chat_field, sanitize_username_field,
    signaling_protocol_version,
};
use super::webrtc_state::{
    begin_cleanup_once, record_webrtc_peer_state, shared_webrtc_api, PeerConnectionDropGuard,
};
use super::{
    ChatMessagesQueue, DataChannelsMap, PlayerManagerRef, ServerInstanceRef, SignalingPeers,
    WorldPartitionManagerRef,
};

use super::rate_limiting::ws_keepalive_interval;

#[allow(clippy::too_many_arguments)]
pub async fn handle_signaling_connection(
    ws: WebSocket,
    peer_id_str: String,
    signaling_peers: SignalingPeers,
    player_manager: PlayerManagerRef,
    _world_partition_manager: WorldPartitionManagerRef, // Marked as unused if not directly used in this function
    data_channels_map: DataChannelsMap,
    client_states_map: ClientStatesMap,
    chat_messages_queue: ChatMessagesQueue,
    config: Arc<ServerConfig>,
    player_aois: PlayerAoIs,
    server_instance: ServerInstanceRef, // Added server instance for initial spawn
    auth_service: AuthService,
    auth_user_id: Option<String>,
    requested_team_id: Option<u8>,
    requested_username: Option<String>,
    remote_ip: Option<IpAddr>,
    // Per-IP concurrent-connection slot, acquired pre-upgrade in the /ws route
    // (`check_ws_ip_connection_cap`) so over-cap clients are rejected at the
    // HTTP handshake. Held for the rest of this function's lifetime; released
    // automatically on drop (any return path, including the many early returns
    // below) via IpConnectionGuard.
    ip_connection_guard: Option<IpConnectionGuard>,
    is_mobile: bool,
    _ws_connection_permit: crate::routes::ws_signaling::WsConnectionPermit,
) {
    let _ip_connection_guard = ip_connection_guard;
    shared_connection_manager().upsert(ConnectionInfo::new(
        peer_id_str.clone(),
        TransportKind::WebRtc,
    ));

    if !try_acquire_join_rate_limit_token() {
        warn!(
            "[{}]: Join attempt throttled by join rate limiter.",
            peer_id_str
        );
        let (mut throttled_ws_tx, _) = ws.split();
        let throttled_payload =
            signaling_error_json("join_rate_limited", JOIN_RATE_LIMIT_THROTTLED_MESSAGE);
        let _ = throttled_ws_tx.send(Message::text(throttled_payload)).await;
        let _ = throttled_ws_tx.send(Message::close()).await;
        let _ = shared_connection_manager().remove(&peer_id_str);
        return;
    }
    if let Some(client_ip) = remote_ip {
        if !try_acquire_ip_rate_limit_token(&client_ip) {
            warn!(
                "[{}]: Join attempt throttled by IP rate limiter (ip={}).",
                peer_id_str, client_ip
            );
            let (mut throttled_ws_tx, _) = ws.split();
            let throttled_payload = serde_json::json!({
                "error": "ip_rate_limited",
                "detail": "Too many signaling connections from this IP. Retry shortly.",
            })
            .to_string();
            let _ = throttled_ws_tx.send(Message::text(throttled_payload)).await;
            let _ = throttled_ws_tx.send(Message::close()).await;
            let _ = shared_connection_manager().remove(&peer_id_str);
            return;
        }
    }

    info!("[{}]: New WebSocket connection for signaling.", peer_id_str);
    server_instance.note_join_enqueued(&peer_id_str);

    let (mut ws_tx, mut ws_rx) = ws.split();
    let (client_signaling_tx, mut client_signaling_rx) = mpsc::channel(SIGNALING_OUTBOX_CAPACITY);

    signaling_peers.insert(peer_id_str.clone(), client_signaling_tx.clone());
    metrics::set_ws_connections_active(signaling_peers.len());

    if let Some(keepalive_interval) = ws_keepalive_interval() {
        let keepalive_sender = client_signaling_tx.clone();
        let keepalive_peer_id = peer_id_str.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(keepalive_interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            // The first tick is immediate for tokio intervals; discard it.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                match keepalive_sender.try_send(Ok(Message::ping(Vec::new()))) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        debug!(
                            "[{}]: Signaling keepalive skipped because outbox is full.",
                            keepalive_peer_id
                        );
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        debug!(
                            "[{}]: Stopping signaling keepalive loop (outbox closed).",
                            keepalive_peer_id
                        );
                        break;
                    }
                }
            }
        });
    }

    let api = match shared_webrtc_api() {
        Ok(api) => api,
        Err(e) => {
            error!(
                "[{}]: Failed to initialize shared WebRTC API: {}",
                peer_id_str, e
            );
            cleanup_connection(
                &peer_id_str,
                &signaling_peers,
                &player_manager,
                &data_channels_map,
                &client_states_map,
                &player_aois,
                &auth_service,
            );
            return;
        }
    };
    let ice_servers = build_ice_servers();
    info!(
        "[{}]: Using {} ICE server(s) for WebRTC negotiation.",
        peer_id_str,
        ice_servers.len()
    );

    // Send ICE server configuration (including TURN with credentials) to the
    // client so it can configure its RTCPeerConnection before creating an offer.
    let client_ice_config = build_client_ice_config(&peer_id_str);
    if !client_ice_config.is_empty() {
        let turn_count = client_ice_config
            .iter()
            .filter(|s| s.urls.iter().any(|u| u.starts_with("turn:")))
            .count();
        let ice_config_msg = serde_json::json!({
            "event": "ice_servers",
            "ice_servers": client_ice_config,
            "server_protocol_version": signaling_protocol_version(),
        })
        .to_string();
        if !try_queue_signaling_message(
            &client_signaling_tx,
            Ok(Message::text(ice_config_msg)),
            &peer_id_str,
            "ice_servers",
        ) {
            warn!(
                "[{}]: Failed to send ICE server config to client.",
                peer_id_str
            );
        } else {
            info!(
                "[{}]: Sent {} ICE server(s) to client ({} TURN).",
                peer_id_str,
                client_ice_config.len(),
                turn_count,
            );
        }
    }

    let rtc_config = RTCConfiguration {
        ice_servers,
        ..Default::default()
    };

    let peer_connection = match api.new_peer_connection(rtc_config).await {
        Ok(pc) => Arc::new(pc),
        Err(e) => {
            error!("[{}]: Failed to create PeerConnection: {}", peer_id_str, e);
            cleanup_connection(
                &peer_id_str,
                &signaling_peers,
                &player_manager,
                &data_channels_map,
                &client_states_map,
                &player_aois,
                &auth_service,
            );
            return;
        }
    };

    // Safety net: if this task is cancelled/aborted, the drop guard ensures
    // the peer connection is closed to avoid resource leaks.
    let mut pc_drop_guard =
        PeerConnectionDropGuard::new(Arc::clone(&peer_connection), peer_id_str.clone());
    let cleanup_once = Arc::new(AtomicBool::new(false));

    let peer_id_fwd = peer_id_str.clone();
    let signaling_peers_for_forwarder = signaling_peers.clone();
    let player_manager_for_forwarder = player_manager.clone();
    let data_channels_for_forwarder = data_channels_map.clone();
    let client_states_for_forwarder = client_states_map.clone();
    let player_aois_for_forwarder = player_aois.clone();
    let auth_service_for_forwarder = auth_service.clone();
    let cleanup_once_for_forwarder = Arc::clone(&cleanup_once);
    tokio::spawn(async move {
        while let Some(message_result) = client_signaling_rx.recv().await {
            match message_result {
                Ok(msg) => {
                    metrics::record_network_bytes("egress_ws", msg.as_bytes().len());
                    if ws_tx.send(msg).await.is_err() {
                        warn!(
                            "[{}]: WebSocket send error, terminating forwarder.",
                            peer_id_fwd
                        );
                        if begin_cleanup_once(cleanup_once_for_forwarder.as_ref()) {
                            cleanup_connection(
                                &peer_id_fwd,
                                &signaling_peers_for_forwarder,
                                &player_manager_for_forwarder,
                                &data_channels_for_forwarder,
                                &client_states_for_forwarder,
                                &player_aois_for_forwarder,
                                &auth_service_for_forwarder,
                            );
                        }
                        break;
                    }
                }
                Err(e) => {
                    error!(
                        "[{}]: Error in message to send via WebSocket: {:?}",
                        peer_id_fwd, e
                    );
                    if begin_cleanup_once(cleanup_once_for_forwarder.as_ref()) {
                        cleanup_connection(
                            &peer_id_fwd,
                            &signaling_peers_for_forwarder,
                            &player_manager_for_forwarder,
                            &data_channels_for_forwarder,
                            &client_states_for_forwarder,
                            &player_aois_for_forwarder,
                            &auth_service_for_forwarder,
                        );
                    }
                    break;
                }
            }
        }
        info!("[{}]: Signaling forwarder task ended.", peer_id_fwd);
    });

    let pc_for_ice = Arc::clone(&peer_connection);
    let ice_sender_clone = client_signaling_tx.clone();
    let peer_id_for_ice = peer_id_str.clone();
    pc_for_ice.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
        let ice_sender = ice_sender_clone.clone();
        let pid_ice = peer_id_for_ice.clone();
        Box::pin(async move {
            if let Some(c) = candidate {
                match c.to_json() {
                    Ok(ice_init_struct) => {
                        let ice_serde = RTCIceCandidateInitSerde {
                            candidate: ice_init_struct.candidate,
                            sdp_mid: ice_init_struct.sdp_mid,
                            sdp_m_line_index: ice_init_struct.sdp_mline_index,
                            username_fragment: ice_init_struct.username_fragment,
                        };
                        let sig_msg = SignalingMessageJson {
                            protocol_version: Some(signaling_protocol_version()),
                            sdp: None,
                            ice: Some(ice_serde),
                        };
                        match serde_json::to_string(&sig_msg) {
                            Ok(json_msg) => {
                                if !try_queue_signaling_message(
                                    &ice_sender,
                                    Ok(Message::text(json_msg)),
                                    &pid_ice,
                                    "ice_candidate",
                                ) {
                                    warn!(
                                        "[{}]: Failed to send ICE candidate via channel.",
                                        pid_ice
                                    );
                                }
                            }
                            Err(e) => {
                                error!("[{}]: Error serializing ICE candidate: {}", pid_ice, e)
                            }
                        }
                    }
                    Err(e) => error!(
                        "[{}]: Error converting ICE candidate to JSON: {}",
                        pid_ice, e
                    ),
                }
            }
        })
    }));

    let pc_for_state_change = Arc::clone(&peer_connection);
    let peer_id_for_state_change = peer_id_str.clone();
    let sp_clone_sc = signaling_peers.clone();
    let pm_clone_sc = player_manager.clone();
    let dc_map_clone_sc = data_channels_map.clone();
    let cs_map_clone_sc = client_states_map.clone();
    let pa_map_clone_sc = player_aois.clone();
    let auth_service_clone_sc = auth_service.clone();
    let cleanup_once_sc = Arc::clone(&cleanup_once);
    let pc_for_state_change_cb = Arc::clone(&pc_for_state_change);

    pc_for_state_change.on_peer_connection_state_change(Box::new(
        move |s: RTCPeerConnectionState| {
            let current_peer_id = peer_id_for_state_change.clone();
            record_webrtc_peer_state(&current_peer_id, s);
            info!(
                "[{}]: Peer Connection State changed: {}",
                current_peer_id, s
            );
            if matches!(s, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
                info!(
                    "[{}]: Peer disconnected/closed. Initiating cleanup.",
                    current_peer_id
                );
                if begin_cleanup_once(cleanup_once_sc.as_ref()) {
                    cleanup_connection(
                        &current_peer_id,
                        &sp_clone_sc,
                        &pm_clone_sc,
                        &dc_map_clone_sc,
                        &cs_map_clone_sc,
                        &pa_map_clone_sc,
                        &auth_service_clone_sc,
                    );
                } else {
                    debug!(
                        "[{}]: Cleanup already performed by another disconnect path.",
                        current_peer_id
                    );
                }
            } else if matches!(s, RTCPeerConnectionState::Disconnected) {
                info!(
                    "[{}]: Peer disconnected. Waiting {}s before cleanup to allow ICE restart.",
                    current_peer_id, DISCONNECTED_CLEANUP_GRACE_SECS
                );
                let delayed_peer_id = current_peer_id.clone();
                let pc_for_delay = Arc::clone(&pc_for_state_change_cb);
                let sp_clone_delay = sp_clone_sc.clone();
                let pm_clone_delay = pm_clone_sc.clone();
                let dc_map_clone_delay = dc_map_clone_sc.clone();
                let cs_map_clone_delay = cs_map_clone_sc.clone();
                let pa_map_clone_delay = pa_map_clone_sc.clone();
                let auth_service_clone_delay = auth_service_clone_sc.clone();
                let cleanup_once_delay = Arc::clone(&cleanup_once_sc);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(DISCONNECTED_CLEANUP_GRACE_SECS))
                        .await;
                    let latest_state = pc_for_delay.connection_state();
                    if matches!(
                        latest_state,
                        RTCPeerConnectionState::Disconnected
                            | RTCPeerConnectionState::Failed
                            | RTCPeerConnectionState::Closed
                    ) {
                        if begin_cleanup_once(cleanup_once_delay.as_ref()) {
                            cleanup_connection(
                                &delayed_peer_id,
                                &sp_clone_delay,
                                &pm_clone_delay,
                                &dc_map_clone_delay,
                                &cs_map_clone_delay,
                                &pa_map_clone_delay,
                                &auth_service_clone_delay,
                            );
                        }
                    } else {
                        debug!(
                            "[{}]: Peer recovered to state {:?}; skipping delayed disconnect cleanup.",
                            delayed_peer_id, latest_state
                        );
                    }
                });
            }
            Box::pin(async {})
        },
    ));

    let pc_for_datachannel_event = Arc::clone(&peer_connection);
    let peer_id_for_dc_event = peer_id_str.clone();
    let player_manager_for_dc_event = player_manager.clone();
    let data_channels_map_for_dc_event = data_channels_map.clone();
    let client_states_map_for_dc_event = client_states_map.clone();
    let signaling_peers_for_dc_event = signaling_peers.clone();
    let player_aois_for_dc_event = player_aois.clone();
    let chat_messages_queue_for_dc_event = chat_messages_queue.clone();
    let config_for_dc_event = config.clone();
    let server_instance_for_dc_event = server_instance.clone(); // Clone server instance for DC event
    let auth_service_for_dc_event = auth_service.clone();
    let auth_user_id_for_dc_event = auth_user_id.clone();
    let cleanup_once_for_dc_event = Arc::clone(&cleanup_once);

    pc_for_datachannel_event.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let dc_label_owned = dc.label().to_owned();
        let current_peer_id_on_dc = peer_id_for_dc_event.clone();
        info!(
            "[{}]: DataChannel '{}' received from client.",
            current_peer_id_on_dc, dc_label_owned
        );

        let dc_on_open_arc = Arc::clone(&dc);
        let dc_for_closure = Arc::clone(&dc);
        let peer_id_on_open = current_peer_id_on_dc.clone();
        let player_manager_on_open = player_manager_for_dc_event.clone();
        let data_channels_map_on_open = data_channels_map_for_dc_event.clone();
        let client_states_map_on_open = client_states_map_for_dc_event.clone();
        let config_on_open = config_for_dc_event.clone();
        let dc_label_for_on_open = dc_label_owned.clone();
        let server_instance_on_open = server_instance_for_dc_event.clone(); // Clone server instance for on_open
        let auth_service_on_open = auth_service_for_dc_event.clone();
        let auth_user_id_on_open = auth_user_id_for_dc_event.clone();
        let requested_team_on_open = requested_team_id;
        let requested_username_on_open = requested_username.clone();

        dc_on_open_arc.on_open(Box::new(move || {
            let current_peer_id_on_open_cb = peer_id_on_open.clone();
            let current_dc_label_on_open_cb = dc_label_for_on_open.clone();
            info!(
                "[{}]: DataChannel '{}' OPENED (on_open callback).",
                current_peer_id_on_open_cb, current_dc_label_on_open_cb
            );
            let dc_for_async_block = Arc::clone(&dc_for_closure);
            server_instance_on_open.note_join_channel_open(&current_peer_id_on_open_cb);

            let core_dc = Arc::new(crate::core::types::RTCDataChannel::new(Arc::clone(
                &dc_for_async_block,
            )));
            data_channels_map_on_open.insert(current_peer_id_on_open_cb.clone(), core_dc.clone());
            info!(
                "[{}]: Added data channel to map. Map size: {}, Map ptr: {:p}",
                current_peer_id_on_open_cb,
                data_channels_map_on_open.len(),
                Arc::as_ptr(&data_channels_map_on_open)
            );

            let initial_client_state = ClientState {
                known_walls_sent: false,
                last_update_sent_time: Instant::now(),
                is_mobile,
                mobile_delta_skip_modulus: if is_mobile { crate::core::constants::MOBILE_DELTA_SKIP_MODULUS } else { 1 },
                ..Default::default()
            };
            client_states_map_on_open
                .write()
                .insert(current_peer_id_on_open_cb.clone(), initial_client_state);
            info!(
                "[{}]: Added client state. Client states map size: {}",
                current_peer_id_on_open_cb,
                client_states_map_on_open.read().len()
            );

            let mut username = format!(
                "Player_{}",
                &current_peer_id_on_open_cb[..4.min(current_peer_id_on_open_cb.len())]
            );

            if let Some(bound_user_id) = auth_user_id_on_open.as_deref() {
                if let Some(profile) = auth_service_on_open.profile_by_user_id(bound_user_id) {
                    username = sanitize_username_field(&profile.display_name, MAX_CHAT_USERNAME_CHARS)
                        .unwrap_or_else(|| username.clone());
                    auth_service_on_open.bind_peer_to_user(&current_peer_id_on_open_cb, bound_user_id);
                    info!(
                        "[{}]: Bound authenticated user '{}' to peer.",
                        current_peer_id_on_open_cb, bound_user_id
                    );
                }
            } else if let Some(requested_name) = requested_username_on_open.as_deref() {
                if let Some(sanitized) =
                    sanitize_username_field(requested_name, MAX_CHAT_USERNAME_CHARS)
                {
                    username = sanitized;
                }
            }

            let requested_spectator = requested_team_on_open == Some(0);
            if requested_spectator && !server_instance_on_open.can_accept_spectator_join() {
                warn!(
                    "[{}]: spectator join rejected due to spectator cap. Closing data channel.",
                    current_peer_id_on_open_cb
                );
                let dc_reject = Arc::clone(&dc_for_async_block);
                return Box::pin(async move {
                    let _ = dc_reject.close().await;
                });
            }
            let requested_balanced_team = requested_team_on_open.filter(|team| *team == 1 || *team == 2);
            if !requested_spectator
                && !server_instance_on_open.ensure_human_join_capacity_for_team(
                    &current_peer_id_on_open_cb,
                    requested_balanced_team,
                )
            {
                warn!(
                    "[{}]: server is full and no bot slot could be reclaimed for human priority join.",
                    current_peer_id_on_open_cb
                );
            }
            let fallback_player_id = player_manager_on_open
                .id_pool
                .get_or_create(&current_peer_id_on_open_cb);
            let (new_player_id_arc_for_team, team_to_assign, initial_spawn_pos) =
                if let Some((player_id, team_id, spawn)) = player_manager_on_open.add_player_for_join(
                    current_peer_id_on_open_cb.clone(),
                    username.clone(),
                    requested_balanced_team,
                    requested_spectator,
                    |resolved_player_id, assigned_team| {
                        if requested_spectator {
                            crate::core::types::Vec2::new(0.0, 0.0)
                        } else {
                            server_instance_on_open.respawn_manager.get_respawn_position(
                                &server_instance_on_open,
                                resolved_player_id,
                                Some(assigned_team),
                                &[],
                            )
                        }
                    },
                ) {
                    (player_id, team_id, spawn)
                } else if let Some(existing_state) =
                    player_manager_on_open.get_player_state(&fallback_player_id)
                {
                    (
                        fallback_player_id.clone(),
                        existing_state.team_id,
                        crate::core::types::Vec2::new(existing_state.x, existing_state.y),
                    )
                } else {
                    warn!(
                        "[{}]: failed to create or recover player state after join attempt.",
                        current_peer_id_on_open_cb
                    );
                    let dc_reject = Arc::clone(&dc_for_async_block);
                    return Box::pin(async move {
                        let _ = dc_reject.close().await;
                    });
                };

            info!(
                "[{}] Player spawned at ({}, {})",
                current_peer_id_on_open_cb, initial_spawn_pos.x, initial_spawn_pos.y
            );

            if let Some(mut p_state_entry) =
                player_manager_on_open.get_player_state_mut(&new_player_id_arc_for_team)
            {
                let p_state: &mut crate::core::types::PlayerState = &mut p_state_entry;
                p_state.team_id = team_to_assign;
                p_state.is_spectator = requested_spectator;
                if requested_spectator {
                    p_state.health = p_state.max_health;
                    p_state.respawn_timer = None;
                    p_state.reload_progress = None;
                }
                if let Some(bound_user_id) = auth_user_id_on_open.as_deref() {
                    if let Some(career_kills) =
                        auth_service_on_open.weapon_kills_by_user_id(bound_user_id)
                    {
                        p_state.career_kills_per_weapon = career_kills;
                    }
                }
                p_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG | FIELD_MISC);
                info!(
                    "[{}] assigned to team {}. Player state marked as changed.",
                    current_peer_id_on_open_cb, team_to_assign
                );
            }

            if let Some(player_state) =
                player_manager_on_open.get_player_state(&new_player_id_arc_for_team)
            {
                // add_player already updated spatial index; only AoI needs explicit initialization.
                server_instance_on_open.update_player_aoi(
                    &new_player_id_arc_for_team,
                    player_state.x,
                    player_state.y,
                );

                info!(
                    "[{}] Player AoI initialized at position ({}, {})",
                    current_peer_id_on_open_cb, player_state.x, player_state.y
                );
            }

            // Send welcome immediately; initial world snapshot is sent by broadcast pipeline.
            let config_for_welcome = config_on_open.clone();
            let server_for_join_packets = server_instance_on_open.clone();
            let core_dc_for_join_packets = core_dc.clone();

            Box::pin(async move {
                let welcome_bytes = build_welcome_message_bytes(
                    &current_peer_id_on_open_cb,
                    config_for_welcome.tick_rate as u16,
                );
                let match_info_bytes = server_for_join_packets.build_match_info_only_bytes();
                let outbound_packets = [welcome_bytes, match_info_bytes];
                let sent_packets = server_for_join_packets
                    .send_packet_batch_optimized(&core_dc_for_join_packets, &outbound_packets, 100)
                    .await;

                if sent_packets < outbound_packets.len() {
                    warn!(
                        "[{}]: Initial join packet batch partial send ({}/{} packets).",
                        current_peer_id_on_open_cb,
                        sent_packets,
                        outbound_packets.len()
                    );
                    // Fallback: re-attempt welcome using the same batched transport path.
                    let fallback_sent = server_for_join_packets
                        .send_packet_batch_optimized(
                            &core_dc_for_join_packets,
                            &outbound_packets[..1],
                            100,
                        )
                        .await;
                    if fallback_sent == 0 {
                        handle_dc_send_error(
                            "send timeout/failure",
                            &current_peer_id_on_open_cb,
                            "welcome message fallback",
                        );
                    }
                    server_for_join_packets
                        .send_match_info_only(
                            &current_peer_id_on_open_cb,
                            &core_dc_for_join_packets,
                        )
                        .await;
                } else {
                    info!(
                        "[{}]: Sent welcome + match-info batch. Initial state will be sent by broadcast pipeline.",
                        current_peer_id_on_open_cb
                    );
                }
            })
        }));

        let dc_on_message_arc = Arc::clone(&dc);
        let peer_id_on_message = current_peer_id_on_dc.clone();
        let player_manager_on_message = player_manager_for_dc_event.clone();
        let chat_q_on_message = chat_messages_queue_for_dc_event.clone();
        let input_rate_limiter = input_rate_limit_config()
            .map(|cfg| Arc::new(AsyncMutex::new(InputRateLimiter::new(cfg.per_sec, cfg.burst))));

        dc_on_message_arc.on_message(Box::new(move |msg: DataChannelMessage| {
            let pid_msg_inner_str = peer_id_on_message.clone();
            let players_map_on_msg = player_manager_on_message.clone();
            let chat_q_on_msg = chat_q_on_message.clone();
            let input_rate_limiter_on_msg = input_rate_limiter.clone();

            Box::pin(async move {
                metrics::record_network_bytes("ingress_data_channel", msg.data.len());
                if msg.data.len() > MAX_DATACHANNEL_MESSAGE_BYTES {
                    metrics::record_input_validation_failed("oversized_data_channel_message");
                    warn!(
                        "[{}]: Dropping oversized data-channel message ({} bytes, limit={}).",
                        pid_msg_inner_str,
                        msg.data.len(),
                        MAX_DATACHANNEL_MESSAGE_BYTES
                    );
                    return;
                }
                // Generic per-connection budget applied before the verifying
                // parse, so it covers every message type (including failed
                // parses and unhandled types), not just Input — those
                // previously cost a full FlatBuffers verify pass plus a
                // synchronous log line with no rate limit at all.
                if let Some(rate_limiter) = input_rate_limiter_on_msg.as_ref() {
                    let mut limiter_guard = rate_limiter.lock().await;
                    if !limiter_guard.try_acquire() {
                        if limiter_guard.should_log_throttle() {
                            warn!(
                                "[{}]: Dropping data-channel message due to per-connection rate limit.",
                                pid_msg_inner_str
                            );
                        }
                        return;
                    }
                }
                if let Ok(game_msg_root) = fb::root_as_game_message(&msg.data) {
                    let protocol_version = game_msg_root.protocol_version();
                    if protocol_version != GAME_PROTOCOL_VERSION {
                        metrics::record_input_validation_failed("protocol_version_mismatch");
                        warn!(
                            "[{}]: Dropping message with protocol_version={} (server expects {}).",
                            pid_msg_inner_str, protocol_version, GAME_PROTOCOL_VERSION
                        );
                        return;
                    }

                    match game_msg_root.msg_type() {
                        fb::MessageType::Input => {
                            // Rate limit already applied generically above, before the parse.
                            if game_msg_root.actual_message_type()
                                == fb::MessagePayload::PlayerInput
                            {
                                if let Some(input_fb) =
                                    game_msg_root.actual_message_as_player_input()
                                {
                                    let p_input_data = PlayerInputData {
                                        timestamp: input_fb.timestamp(),
                                        sequence: input_fb.sequence(),
                                        move_forward: input_fb.move_forward(),
                                        move_backward: input_fb.move_backward(),
                                        move_left: input_fb.move_left(),
                                        move_right: input_fb.move_right(),
                                        shooting: input_fb.shooting(),
                                        reload: input_fb.reload(),
                                        rotation: input_fb.rotation(),
                                        melee_attack: input_fb.melee_attack(),
                                        change_weapon_slot: input_fb.change_weapon_slot() as u8,
                                        use_ability_slot: input_fb.use_ability_slot() as u8,
                                        ping_x: input_fb.ping_x(),
                                        ping_y: input_fb.ping_y(),
                                    };

                                    let player_id_arc: PlayerID = players_map_on_msg
                                        .id_pool
                                        .get_or_create(&pid_msg_inner_str);
                                    if let Some(mut player_entry) =
                                        players_map_on_msg.get_player_state_mut(&player_id_arc)
                                    {
                                        let seq = p_input_data.sequence;
                                        if player_entry.queue_input(p_input_data) {
                                            debug!(
                                                "[{}]: Accepted player input (seq: {})",
                                                pid_msg_inner_str, seq
                                            );
                                        } else {
                                            metrics::record_input_validation_failed("sequence_gap_or_replay");
                                            debug!(
                                                "[{}]: Rejected player input (seq: {}, last_accepted: {}) – replay or sequence gap",
                                                pid_msg_inner_str, seq, player_entry.last_queued_input_sequence
                                            );
                                        }
                                    } else {
                                        warn!(
                                            "[{}]: Player state not found for input processing.",
                                            pid_msg_inner_str
                                        );
                                    }
                                }
                            }
                        }
                        fb::MessageType::Chat => {
                            if game_msg_root.actual_message_type()
                                == fb::MessagePayload::ChatMessage
                            {
                                if let Some(chat_fb) =
                                    game_msg_root.actual_message_as_chat_message()
                                {
                                    if let Some(message_text_fb) = chat_fb.message() {
                                        let chat_timestamp = now_millis();
                                        if !try_consume_chat_rate_limit(
                                            &pid_msg_inner_str,
                                            chat_timestamp,
                                        ) {
                                            trace!(
                                                "[{}]: Dropping chat message due to per-player rate limit.",
                                                pid_msg_inner_str
                                            );
                                            return;
                                        }
                                        let player_id_from_connection = pid_msg_inner_str.clone();
                                        let player_id_arc_for_chat = players_map_on_msg
                                            .id_pool
                                            .get_or_create(&player_id_from_connection);
                                        let Some(sanitized_message) = sanitize_chat_field(
                                            message_text_fb,
                                            MAX_CHAT_MESSAGE_CHARS,
                                        ) else {
                                            warn!(
                                                "[{}]: Dropping empty/invalid chat payload.",
                                                pid_msg_inner_str
                                            );
                                            return;
                                        };
                                        let authoritative_username = players_map_on_msg
                                            .get_player_state(&player_id_arc_for_chat)
                                            .map(|state| state.username.clone());
                                        let sanitized_username = sanitize_username_field(
                                            authoritative_username
                                                .as_deref()
                                                .unwrap_or("Player"),
                                            MAX_CHAT_USERNAME_CHARS,
                                        )
                                        .unwrap_or_else(|| "Player".to_owned());
                                        let current_seq = next_chat_message_seq();
                                        let chat_entry = ChatMessage {
                                            seq: current_seq,
                                            player_id: player_id_arc_for_chat,
                                            username: sanitized_username,
                                            message: sanitized_message,
                                            // Use server authoritative wall-clock timestamp to
                                            // prevent client-side future timestamp spoofing.
                                            timestamp: chat_timestamp,
                                        };
                                        info!(
                                            "[CHAT] {} ({}): {}",
                                            chat_entry.username,
                                            chat_entry.player_id.as_ref(),
                                            chat_entry.message
                                        );
                                        let mut chat_q_guard = chat_q_on_msg.write().await;
                                        chat_q_guard.push_back(chat_entry);
                                    }
                                }
                            }
                        }
                        _ => warn!(
                            "[{}]: Received unhandled FB message type: {:?}",
                            pid_msg_inner_str,
                            game_msg_root.msg_type()
                        ),
                    }
                } else {
                    error!(
                        "[{}]: Failed to parse FlatBuffer message from client.",
                        pid_msg_inner_str
                    );
                }
            })
        }));

        let dc_on_close_arc = Arc::clone(&dc);
        let peer_id_on_close = current_peer_id_on_dc.clone();
        let dc_label_for_on_close = dc_label_owned.clone();
        let signaling_peers_on_close = signaling_peers_for_dc_event.clone();
        let player_manager_on_close = player_manager_for_dc_event.clone();
        let data_channels_map_on_close = data_channels_map_for_dc_event.clone();
        let client_states_map_on_close = client_states_map_for_dc_event.clone();
        let player_aois_on_close = player_aois_for_dc_event.clone();
        let auth_service_on_close = auth_service_for_dc_event.clone();
        let server_instance_on_close = server_instance_for_dc_event.clone();
        let cleanup_once_on_close = Arc::clone(&cleanup_once_for_dc_event);

        dc_on_close_arc.on_close(Box::new(move || {
            info!(
                "[{}]: DataChannel '{}' CLOSED.",
                peer_id_on_close, dc_label_for_on_close
            );
            if begin_cleanup_once(cleanup_once_on_close.as_ref()) {
                cleanup_connection(
                    &peer_id_on_close,
                    &signaling_peers_on_close,
                    &player_manager_on_close,
                    &data_channels_map_on_close,
                    &client_states_map_on_close,
                    &player_aois_on_close,
                    &auth_service_on_close,
                );
                server_instance_on_close.cleanup_player_tracking_state(&peer_id_on_close);
            } else {
                debug!(
                    "[{}]: Cleanup already performed before data channel close callback.",
                    peer_id_on_close
                );
            }
            Box::pin(async {})
        }));

        let dc_on_error_arc = Arc::clone(&dc);
        let peer_id_on_error = current_peer_id_on_dc.clone();
        let dc_label_for_on_error = dc_label_owned.clone();
        let signaling_peers_on_error = signaling_peers_for_dc_event.clone();
        let player_manager_on_error = player_manager_for_dc_event.clone();
        let data_channels_map_on_error = data_channels_map_for_dc_event.clone();
        let client_states_map_on_error = client_states_map_for_dc_event.clone();
        let player_aois_on_error = player_aois_for_dc_event.clone();
        let auth_service_on_error = auth_service_for_dc_event.clone();
        let server_instance_on_error = server_instance_for_dc_event.clone();
        let cleanup_once_on_error = Arc::clone(&cleanup_once_for_dc_event);

        dc_on_error_arc.on_error(Box::new(move |err| {
            error!(
                "[{}]: DataChannel '{}' ERROR: {}",
                peer_id_on_error, dc_label_for_on_error, err
            );
            if begin_cleanup_once(cleanup_once_on_error.as_ref()) {
                cleanup_connection(
                    &peer_id_on_error,
                    &signaling_peers_on_error,
                    &player_manager_on_error,
                    &data_channels_map_on_error,
                    &client_states_map_on_error,
                    &player_aois_on_error,
                    &auth_service_on_error,
                );
                server_instance_on_error.cleanup_player_tracking_state(&peer_id_on_error);
            } else {
                debug!(
                    "[{}]: Cleanup already performed before data channel error callback.",
                    peer_id_on_error
                );
            }
            Box::pin(async {})
        }));

        Box::pin(async move {})
    }));

    let pc_signal_receiver = Arc::clone(&peer_connection);
    let ws_signal_sender_clone = client_signaling_tx.clone();
    let current_peer_id_ws = peer_id_str.clone();
    let ice_rate_limiter = ice_candidate_rate_limit_config().map(|cfg| {
        Arc::new(AsyncMutex::new(InputRateLimiter::new(
            cfg.per_sec,
            cfg.burst,
        )))
    });

    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(msg) => {
                if msg.is_text() {
                    shared_connection_manager().touch(&current_peer_id_ws);
                    metrics::record_network_bytes("ingress_ws", msg.as_bytes().len());
                    if msg.as_bytes().len() > MAX_SIGNALING_TEXT_BYTES {
                        warn!(
                            "[{}]: Signaling message exceeds {} bytes; closing connection.",
                            current_peer_id_ws, MAX_SIGNALING_TEXT_BYTES
                        );
                        break;
                    }
                    if let Ok(text_content) = msg.to_str() {
                        match serde_json::from_str::<SignalingMessageJson>(text_content) {
                            Ok(sig_data) => {
                                if let Err(reason) = validate_signaling_payload(&sig_data) {
                                    metrics::record_input_validation_failed(
                                        if reason.contains("protocol_version") {
                                            "signaling_protocol_version_mismatch"
                                        } else {
                                            "invalid_signaling_payload"
                                        },
                                    );
                                    let detail = match reason {
                                        "protocol_version mismatch" => format!(
                                            "Client signaling protocol version is incompatible with server version {}.",
                                            signaling_protocol_version()
                                        ),
                                        "missing protocol_version" => format!(
                                            "Client signaling payload is missing protocol_version; server requires version {}.",
                                            signaling_protocol_version()
                                        ),
                                        _ => reason.to_owned(),
                                    };
                                    let code = if reason.contains("protocol_version") {
                                        "protocol_version_mismatch"
                                    } else {
                                        "invalid_signaling_payload"
                                    };
                                    let _ = try_queue_signaling_message(
                                        &ws_signal_sender_clone,
                                        Ok(Message::text(signaling_error_json(code, detail))),
                                        &current_peer_id_ws,
                                        code,
                                    );
                                    warn!(
                                        "[{}]: Invalid signaling payload: {}. Closing connection.",
                                        current_peer_id_ws, reason
                                    );
                                    break;
                                }
                                if let Some(sdp) = sig_data.sdp {
                                    let _sdp_permit = match acquire_sdp_admission_permit(
                                        &current_peer_id_ws,
                                        &ws_signal_sender_clone,
                                    )
                                    .await
                                    {
                                        Ok(permit) => permit,
                                        Err(()) => {
                                            continue;
                                        }
                                    };
                                    if let Err(e) =
                                        pc_signal_receiver.set_remote_description(sdp.clone()).await
                                    {
                                        error!(
                                            "[{}]: Error setting remote description: {}",
                                            current_peer_id_ws, e
                                        );
                                        continue;
                                    }
                                    if pc_signal_receiver.remote_description().await.is_some_and(|rd| rd.sdp_type == webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Offer) {
                                        match pc_signal_receiver.create_answer(None).await {
                                            Ok(answer) => {
                                                if pc_signal_receiver.set_local_description(answer.clone()).await.is_ok() {
                                                    let resp_msg = SignalingMessageJson {
                                                        protocol_version: Some(signaling_protocol_version()),
                                                        sdp: Some(answer),
                                                        ice: None,
                                                    };
                                                    if let Ok(json_resp) = serde_json::to_string(&resp_msg) {
                                                        if !try_queue_signaling_message(
                                                            &ws_signal_sender_clone,
                                                            Ok(Message::text(json_resp)),
                                                            &current_peer_id_ws,
                                                            "sdp_answer",
                                                        ) {
                                                            warn!("[{}]: Failed to send SDP answer via channel.", current_peer_id_ws);
                                                        }
                                                    } else {
                                                        error!("[{}]: Error serializing SDP answer.", current_peer_id_ws);
                                                    }
                                                } else {
                                                    error!("[{}]: Error setting local description for answer.", current_peer_id_ws);
                                                }
                                            }
                                            Err(e) => error!("[{}]: Error creating SDP answer: {}", current_peer_id_ws, e),
                                        }
                                    }
                                } else if let Some(ice) = sig_data.ice {
                                    if let Some(rate_limiter) = ice_rate_limiter.as_ref() {
                                        let mut limiter_guard = rate_limiter.lock().await;
                                        if !limiter_guard.try_acquire() {
                                            if limiter_guard.should_log_throttle() {
                                                warn!(
                                                    "[{}]: Dropping ICE candidate due to per-connection signaling rate limit.",
                                                    current_peer_id_ws
                                                );
                                            }
                                            continue;
                                        }
                                    }
                                    let ice_init = RTCIceCandidateInit {
                                        candidate: ice.candidate,
                                        sdp_mid: ice.sdp_mid,
                                        sdp_mline_index: ice.sdp_m_line_index,
                                        username_fragment: ice.username_fragment,
                                    };
                                    if let Err(e) =
                                        pc_signal_receiver.add_ice_candidate(ice_init).await
                                    {
                                        warn!(
                                            "[{}]: Error adding ICE candidate: {}",
                                            current_peer_id_ws, e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                metrics::record_input_validation_failed("invalid_signaling_json");
                                let _ = try_queue_signaling_message(
                                    &ws_signal_sender_clone,
                                    Ok(Message::text(signaling_error_json(
                                        "invalid_signaling_payload",
                                        "Malformed signaling JSON.",
                                    ))),
                                    &current_peer_id_ws,
                                    "invalid_signaling_payload",
                                );
                                error!(
                                    "[{}]: Failed to parse signaling message: {} (len={}).",
                                    current_peer_id_ws,
                                    e,
                                    text_content.len()
                                );
                            }
                        }
                    }
                } else if msg.is_close() {
                    info!("[{}]: WebSocket closed by client.", current_peer_id_ws);
                    break;
                } else if msg.is_ping() {
                    let payload = msg.as_bytes().to_vec();
                    if !try_queue_signaling_message(
                        &ws_signal_sender_clone,
                        Ok(Message::pong(payload)),
                        &current_peer_id_ws,
                        "ws_pong",
                    ) {
                        break;
                    }
                } else if msg.is_pong() {
                    trace!("[{}]: WebSocket pong received.", current_peer_id_ws);
                }
            }
            Err(e) => {
                warn!("[{}]: WebSocket error: {}", current_peer_id_ws, e);
                break;
            }
        }
    }

    info!(
        "[{}]: WebSocket connection handler for signaling ending.",
        peer_id_str
    );
    if begin_cleanup_once(cleanup_once.as_ref()) {
        cleanup_connection(
            &peer_id_str,
            &signaling_peers,
            &player_manager,
            &data_channels_map,
            &client_states_map,
            &player_aois,
            &auth_service,
        );
    } else {
        debug!(
            "[{}]: Cleanup already performed by peer-state callback.",
            peer_id_str
        );
    }
    // Defuse the drop guard since we are closing the connection explicitly.
    pc_drop_guard.defuse();
    // Clean up interpolation history to prevent memory leak after disconnect.
    server_instance.cleanup_player_tracking_state(&peer_id_str);
    if let Err(e) = peer_connection.close().await {
        error!("[{}]: Error closing PeerConnection: {}", peer_id_str, e);
    }
}

// Need Instant for ClientState construction in on_open callback
use std::time::Instant;
