// massive_game_server/server/src/network/signaling.rs
use crate::core::config::ServerConfig;
use crate::core::types::{
    PlayerState as MassivePlayerState, PlayerID, Vec2, Wall as CoreWall, Pickup as CorePickup,
    CorePickupType, PlayerInputData, ServerWeaponType, EntityId, RTCDataChannel as CoreRTCDataChannel,
    FIELD_POSITION_ROTATION, FIELD_HEALTH_ALIVE, FIELD_WEAPON_AMMO, FIELD_SCORE_STATS,
    FIELD_POWERUPS, FIELD_SHIELD, FIELD_FLAG, PlayerAoI, PlayerAoIs,
};

use crate::core::constants::*;
use crate::core::types::PlayerState;
use crate::entities::player::ImprovedPlayerManager;
use crate::flatbuffers_generated::game_protocol as fb;
use crate::world::partition::WorldPartitionManager;
use crate::server::instance::MassiveGameServer; // Added for server access for initial spawn
use parking_lot::RwLock as ParkingLotRwLock;

use bytes::Bytes;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn, debug};
use warp::ws::{Message, WebSocket};
use webrtc::{
    api::{media_engine::MediaEngine, APIBuilder},
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
};
use uuid::Uuid;
// Removed: use rand::Rng; // Not directly used here after spawn logic change

// Type Aliases
pub type SignalingPeers =
    Arc<std::sync::Mutex<HashMap<String, mpsc::UnboundedSender<Result<Message, warp::Error>>>>>;
pub type PlayerManagerRef = Arc<ImprovedPlayerManager>;
pub type DataChannelsMap = Arc<DashMap<String, Arc<CoreRTCDataChannel>>>;
pub type WorldPartitionManagerRef = Arc<WorldPartitionManager>;
pub type ServerInstanceRef = Arc<MassiveGameServer>; // Type alias for server instance

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub seq: u64,
    pub player_id: PlayerID,
    pub username: String,
    pub message: String,
    pub timestamp: u64,
}
pub type ChatMessagesQueue = Arc<RwLock<VecDeque<ChatMessage>>>;
static NEXT_CHAT_MESSAGE_SEQ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct ClientState {
    pub known_walls_sent: bool,
    pub last_update_sent_time: Instant,
    pub last_known_player_states: HashMap<PlayerID, PlayerState>,
    pub last_known_projectile_ids: HashSet<EntityId>,
    pub last_known_pickup_states: HashMap<EntityId, PickupState>,
    pub last_known_match_state: Option<fb::MatchStateType>,
    pub last_known_match_time_remaining: Option<f32>,
    pub last_known_team_scores: HashMap<u8, i32>,
    pub known_destroyed_wall_ids: HashSet<EntityId>,
    pub last_kill_feed_count_sent: usize,
    pub last_chat_message_seq_sent: u64,
    pub last_broadcast_frame: u64,
    pub last_known_players: HashSet<Arc<String>>,
    pub last_known_wall_ids: Option<HashSet<EntityId>>,
    pub last_known_wall_states: HashMap<EntityId, (i32, i32)>,  // wall_id -> (current_health, max_health)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PickupState {
    pub is_active: bool,
}

impl Default for ClientState {
    fn default() -> Self {
        ClientState {
            known_walls_sent: false,
            last_update_sent_time: Instant::now(),
            last_known_player_states: HashMap::new(),
            last_known_projectile_ids: HashSet::new(),
            last_known_pickup_states: HashMap::new(),
            last_known_match_state: None,
            last_known_match_time_remaining: None,
            last_known_team_scores: HashMap::new(),
            known_destroyed_wall_ids: HashSet::new(),
            last_kill_feed_count_sent: 0,
            last_chat_message_seq_sent: 0,
            last_broadcast_frame: 0,
            last_known_players: HashSet::new(),
            last_known_wall_ids: None,
            last_known_wall_states: HashMap::new(),
        }
    }
}
//pub type ClientStatesMap = Arc<DashMap<String, ClientState>>;
pub type ClientStatesMap = Arc<ParkingLotRwLock<HashMap<String, ClientState>>>;

#[derive(Serialize, Deserialize, Debug)]
struct SignalingMessageJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp: Option<RTCSessionDescription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ice: Option<RTCIceCandidateInitSerde>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RTCIceCandidateInitSerde {
    candidate: String,
    #[serde(rename = "sdpMid")]
    sdp_mid: Option<String>,
    #[serde(rename = "sdpMLineIndex")]
    sdp_m_line_index: Option<u16>,
    #[serde(rename = "usernameFragment")]
    username_fragment: Option<String>,
}

fn map_server_weapon_to_fb(server_weapon: ServerWeaponType) -> fb::WeaponType {
    match server_weapon {
        ServerWeaponType::Pistol => fb::WeaponType::Pistol,
        ServerWeaponType::Shotgun => fb::WeaponType::Shotgun,
        ServerWeaponType::Rifle => fb::WeaponType::Rifle,
        ServerWeaponType::Sniper => fb::WeaponType::Sniper,
        ServerWeaponType::Melee => fb::WeaponType::Melee,
    }
}

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
) {
    info!("[{}]: New WebSocket connection for signaling.", peer_id_str);

    let (mut ws_tx, mut ws_rx) = ws.split();
    let (client_signaling_tx, mut client_signaling_rx) = mpsc::unbounded_channel();

    signaling_peers
        .lock()
        .unwrap()
        .insert(peer_id_str.clone(), client_signaling_tx.clone());

    let peer_id_fwd = peer_id_str.clone();
    tokio::spawn(async move {
        while let Some(message_result) = client_signaling_rx.recv().await {
            match message_result {
                Ok(msg) => {
                    if ws_tx.send(msg).await.is_err() {
                        warn!("[{}]: WebSocket send error, terminating forwarder.", peer_id_fwd);
                        break;
                    }
                }
                Err(e) => {
                    error!("[{}]: Error in message to send via WebSocket: {:?}", peer_id_fwd, e);
                    break;
                }
            }
        }
        info!("[{}]: Signaling forwarder task ended.", peer_id_fwd);
    });

    let mut m = MediaEngine::default();
    if let Err(e) = m.register_default_codecs() {
        error!("[{}]: Failed to register default codecs: {}", peer_id_str, e);
        cleanup_connection(&peer_id_str, &signaling_peers, &player_manager, &data_channels_map, &client_states_map, &player_aois);
        return;
    }

    let api = APIBuilder::new().with_media_engine(m).build();
    let rtc_config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    let peer_connection = match api.new_peer_connection(rtc_config).await {
        Ok(pc) => Arc::new(pc),
        Err(e) => {
            error!("[{}]: Failed to create PeerConnection: {}", peer_id_str, e);
            cleanup_connection(&peer_id_str, &signaling_peers, &player_manager, &data_channels_map, &client_states_map, &player_aois);
            return;
        }
    };

    let pc_for_ice = Arc::clone(&peer_connection);
    let ice_sender_clone = client_signaling_tx.clone();
    let peer_id_for_ice = peer_id_str.clone();
    pc_for_ice.on_ice_candidate(Box::new(
        move |candidate: Option<RTCIceCandidate>| {
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
                                sdp: None,
                                ice: Some(ice_serde),
                            };
                            match serde_json::to_string(&sig_msg) {
                                Ok(json_msg) => {
                                    if ice_sender.send(Ok(Message::text(json_msg))).is_err() {
                                        warn!("[{}]: Failed to send ICE candidate via channel.", pid_ice);
                                    }
                                }
                                Err(e) => error!("[{}]: Error serializing ICE candidate: {}", pid_ice, e),
                            }
                        }
                        Err(e) => error!("[{}]: Error converting ICE candidate to JSON: {}", pid_ice, e),
                    }
                }
            })
        },
    ));

    let pc_for_state_change = Arc::clone(&peer_connection);
    let peer_id_for_state_change = peer_id_str.clone();
    let sp_clone_sc = signaling_peers.clone();
    let pm_clone_sc = player_manager.clone();
    let dc_map_clone_sc = data_channels_map.clone();
    let cs_map_clone_sc = client_states_map.clone();
    let pa_map_clone_sc = player_aois.clone();


    pc_for_state_change.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
        let current_peer_id = peer_id_for_state_change.clone();
        info!("[{}]: Peer Connection State changed: {}", current_peer_id, s);
        if matches!(
            s,
            RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Closed
                | RTCPeerConnectionState::Disconnected
        ) {
            info!("[{}]: Peer disconnected/closed. Initiating cleanup.", current_peer_id);
            cleanup_connection(&current_peer_id, &sp_clone_sc, &pm_clone_sc, &dc_map_clone_sc, &cs_map_clone_sc, &pa_map_clone_sc);
        }
        Box::pin(async {})
    }));

    let pc_for_datachannel_event = Arc::clone(&peer_connection);
    let peer_id_for_dc_event = peer_id_str.clone();
    let player_manager_for_dc_event = player_manager.clone();
    let data_channels_map_for_dc_event = data_channels_map.clone();
    let client_states_map_for_dc_event = client_states_map.clone();
    let chat_messages_queue_for_dc_event = chat_messages_queue.clone();
    let config_for_dc_event = config.clone();
    let server_instance_for_dc_event = server_instance.clone(); // Clone server instance for DC event


    pc_for_datachannel_event.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let dc_label_owned = dc.label().to_owned();
        let current_peer_id_on_dc = peer_id_for_dc_event.clone();
        info!("[{}]: DataChannel '{}' received from client.", current_peer_id_on_dc, dc_label_owned);

        let dc_on_open_arc = Arc::clone(&dc);
        let dc_for_closure = Arc::clone(&dc);
        let peer_id_on_open = current_peer_id_on_dc.clone();
        let player_manager_on_open = player_manager_for_dc_event.clone();
        let data_channels_map_on_open = data_channels_map_for_dc_event.clone();
        let client_states_map_on_open = client_states_map_for_dc_event.clone();
        let config_on_open = config_for_dc_event.clone();
        let dc_label_for_on_open = dc_label_owned.clone();
        let server_instance_on_open = server_instance_for_dc_event.clone(); // Clone server instance for on_open


        dc_on_open_arc.on_open(Box::new(move || {
            let current_peer_id_on_open_cb = peer_id_on_open.clone();
            let current_dc_label_on_open_cb = dc_label_for_on_open.clone();
            info!("[{}]: DataChannel '{}' OPENED (on_open callback).", current_peer_id_on_open_cb, current_dc_label_on_open_cb);
            let dc_for_async_block = Arc::clone(&dc_for_closure);

            let core_dc = Arc::new(crate::core::types::RTCDataChannel::new(Arc::clone(&dc_for_async_block)));
            data_channels_map_on_open.insert(current_peer_id_on_open_cb.clone(), core_dc.clone());
            info!("[{}]: Added data channel to map. Map size: {}, Map ptr: {:p}", 
                current_peer_id_on_open_cb, 
                data_channels_map_on_open.len(),
                Arc::as_ptr(&data_channels_map_on_open)
            );

            let initial_client_state = ClientState {
                known_walls_sent: false,
                last_update_sent_time: Instant::now(),
                ..Default::default()
            };
            client_states_map_on_open.write().insert(current_peer_id_on_open_cb.clone(), initial_client_state);
            info!("[{}]: Added client state. Client states map size: {}", current_peer_id_on_open_cb, client_states_map_on_open.read().len());

            let username = format!("Player_{}", &current_peer_id_on_open_cb[..4.min(current_peer_id_on_open_cb.len())]);
            

            
            // Fix 2.2: Use RespawnManager for initial spawn
            let player_id_arc_for_spawn = player_manager_on_open.id_pool.get_or_create(&current_peer_id_on_open_cb);
            let team_to_assign = player_manager_on_open.assign_team_to_new_player();
            let initial_spawn_pos = server_instance_on_open.respawn_manager.get_respawn_position(
                &server_instance_on_open, // Pass the server instance
                &player_id_arc_for_spawn,
                Some(team_to_assign),
                &[] // No specific enemy positions for initial spawn balancing here
            );

            info!("[{}] Player spawned at ({}, {})", current_peer_id_on_open_cb, initial_spawn_pos.x, initial_spawn_pos.y);


            let _player_id_arc = player_manager_on_open.add_player(
                current_peer_id_on_open_cb.clone(),
                username.clone(),
                initial_spawn_pos.x, // Use determined spawn position
                initial_spawn_pos.y  // Use determined spawn position
            ).unwrap_or_else(|| {
                warn!("[{}]: add_player returned None, attempting to get existing PlayerID Arc.", current_peer_id_on_open_cb);
                player_manager_on_open.id_pool.get_or_create(&current_peer_id_on_open_cb)
            });

            let new_player_id_arc_for_team = player_manager_on_open.id_pool.get_or_create(&current_peer_id_on_open_cb);
            // let team_to_assign = player_manager_on_open.assign_team_to_new_player(); // Moved up

            if let Some(mut p_state_entry) = player_manager_on_open.get_player_state_mut(&new_player_id_arc_for_team) {
                let p_state: &mut PlayerState = &mut *p_state_entry;
                p_state.team_id = team_to_assign;
                p_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
                info!("[{}] assigned to team {}. Player state marked as changed.", current_peer_id_on_open_cb, team_to_assign);
            }

            if let Some(player_state) = player_manager_on_open.get_player_state(&new_player_id_arc_for_team) {
                // Update spatial index with player's position
                server_instance_on_open.spatial_index.update_player_position(
                    new_player_id_arc_for_team.clone(), 
                    player_state.x, 
                    player_state.y
                );
                
                // Update player's AoI
                server_instance_on_open.update_player_aoi(
                    &new_player_id_arc_for_team, 
                    player_state.x, 
                    player_state.y
                );
                
                info!("[{}] Player AoI initialized at position ({}, {})", 
                    current_peer_id_on_open_cb, player_state.x, player_state.y);
            }

            let config_for_welcome = config_on_open.clone();

            Box::pin(async move {
                let mut builder_welcome = flatbuffers::FlatBufferBuilder::with_capacity(256);
                let player_id_fb_welcome = builder_welcome.create_string(&current_peer_id_on_open_cb);
                let welcome_text_fb = builder_welcome.create_string("Welcome to MassiveGameServer!");
                let welcome_msg_args = fb::WelcomeMessageArgs {
                    player_id: Some(player_id_fb_welcome),
                    message: Some(welcome_text_fb),
                    server_tick_rate: config_for_welcome.tick_rate as u16,
                };
                let welcome_msg = fb::WelcomeMessage::create(&mut builder_welcome, &welcome_msg_args);
                let game_msg_welcome_args = fb::GameMessageArgs {
                    msg_type: fb::MessageType::Welcome,
                    actual_message_type: fb::MessagePayload::WelcomeMessage,
                    actual_message: Some(welcome_msg.as_union_value()),
                };
                let game_msg_welcome = fb::GameMessage::create(&mut builder_welcome, &game_msg_welcome_args);
                builder_welcome.finish(game_msg_welcome, None);

                if let Err(e) = dc_for_async_block.send(&Bytes::from(builder_welcome.finished_data().to_vec())).await {
                    handle_dc_send_error(&e.to_string(), &current_peer_id_on_open_cb, "welcome message");
                } else {
                    info!("[{}]: Sent WelcomeMessage. Initial state will be sent by game loop.", current_peer_id_on_open_cb);
                }
            })
        
        
        }));


        let dc_on_message_arc = Arc::clone(&dc);
        let peer_id_on_message = current_peer_id_on_dc.clone();
        let player_manager_on_message = player_manager_for_dc_event.clone();
        let chat_q_on_message = chat_messages_queue_for_dc_event.clone();

        dc_on_message_arc.on_message(Box::new(move |msg: DataChannelMessage| {
            let pid_msg_inner_str = peer_id_on_message.clone();
            let players_map_on_msg = player_manager_on_message.clone();
            let chat_q_on_msg = chat_q_on_message.clone();

            Box::pin(async move {
                if let Ok(game_msg_root) = fb::root_as_game_message(&msg.data) {
                    match game_msg_root.msg_type() {
                        fb::MessageType::Input => {
                            if game_msg_root.actual_message_type() == fb::MessagePayload::PlayerInput {
                                if let Some(input_fb) = game_msg_root.actual_message_as_player_input() {
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
                                    };

                                    let player_id_arc: PlayerID = players_map_on_msg.id_pool.get_or_create(&pid_msg_inner_str);
                                    if let Some(mut player_entry) = players_map_on_msg.get_player_state_mut(&player_id_arc) {
                                        debug!("[{}]: Received player input (seq: {})", pid_msg_inner_str, p_input_data.sequence);
                                        player_entry.queue_input(p_input_data);
                                    } else {
                                         warn!("[{}]: Player state not found for input processing.", pid_msg_inner_str);
                                    }
                                }
                            }
                        }
                        fb::MessageType::Chat => {
                            if game_msg_root.actual_message_type() == fb::MessagePayload::ChatMessage {
                                if let Some(chat_fb) = game_msg_root.actual_message_as_chat_message() {
                                    if let (Some(message_text_fb), Some(username_text_fb)) = (chat_fb.message(), chat_fb.username()) {
                                        let player_id_from_connection = pid_msg_inner_str.clone();
                                        let player_id_arc_for_chat = players_map_on_msg.id_pool.get_or_create(&player_id_from_connection);

                                        let trimmed_msg: String = message_text_fb.chars().take(100).collect();
                                        let current_seq = NEXT_CHAT_MESSAGE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                        let chat_entry = ChatMessage {
                                            seq: current_seq,
                                            player_id: player_id_arc_for_chat,
                                            username: username_text_fb.to_string(),
                                            message: trimmed_msg,
                                            timestamp: chat_fb.timestamp(),
                                        };
                                        info!("[CHAT] {} ({}): {}", chat_entry.username, *chat_entry.player_id, chat_entry.message);
                                        let mut chat_q_guard = chat_q_on_msg.write().await;
                                        chat_q_guard.push_back(chat_entry);
                                        if chat_q_guard.len() > 50 {
                                            chat_q_guard.pop_front();
                                        }
                                    }
                                }
                            }
                        }
                        _ => warn!("[{}]: Received unhandled FB message type: {:?}", pid_msg_inner_str, game_msg_root.msg_type()),
                    }
                } else {
                    error!("[{}]: Failed to parse FlatBuffer message from client.", pid_msg_inner_str);
                }
            })
        }));

        let dc_on_close_arc = Arc::clone(&dc);
        let peer_id_on_close = current_peer_id_on_dc.clone();
        let dc_label_for_on_close = dc_label_owned.clone();

        dc_on_close_arc.on_close(Box::new(move || {
            info!("[{}]: DataChannel '{}' CLOSED.", peer_id_on_close, dc_label_for_on_close);
            Box::pin(async {})
        }));

        let dc_on_error_arc = Arc::clone(&dc);
        let peer_id_on_error = current_peer_id_on_dc.clone();
        let dc_label_for_on_error = dc_label_owned.clone();

        dc_on_error_arc.on_error(Box::new(move |err| {
            error!("[{}]: DataChannel '{}' ERROR: {}", peer_id_on_error, dc_label_for_on_error, err);
            Box::pin(async {})
        }));

        Box::pin(async move {})
    }));

    let pc_signal_receiver = Arc::clone(&peer_connection);
    let ws_signal_sender_clone = client_signaling_tx.clone();
    let current_peer_id_ws = peer_id_str.clone();

    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(msg) => {
                if msg.is_text() {
                    if let Ok(text_content) = msg.to_str() {
                        match serde_json::from_str::<SignalingMessageJson>(text_content) {
                            Ok(sig_data) => {
                                if let Some(sdp) = sig_data.sdp {
                                    if let Err(e) = pc_signal_receiver.set_remote_description(sdp.clone()).await {
                                        error!("[{}]: Error setting remote description: {}", current_peer_id_ws, e);
                                        continue;
                                    }
                                    if pc_signal_receiver.remote_description().await.map_or(false, |rd| rd.sdp_type == webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Offer) {
                                        match pc_signal_receiver.create_answer(None).await {
                                            Ok(answer) => {
                                                if pc_signal_receiver.set_local_description(answer.clone()).await.is_ok() {
                                                    let resp_msg = SignalingMessageJson { sdp: Some(answer), ice: None };
                                                    if let Ok(json_resp) = serde_json::to_string(&resp_msg) {
                                                        if ws_signal_sender_clone.send(Ok(Message::text(json_resp))).is_err() {
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
                                    let ice_init = RTCIceCandidateInit {
                                        candidate: ice.candidate,
                                        sdp_mid: ice.sdp_mid,
                                        sdp_mline_index: ice.sdp_m_line_index,
                                        username_fragment: ice.username_fragment,
                                    };
                                    if let Err(e) = pc_signal_receiver.add_ice_candidate(ice_init).await {
                                        warn!("[{}]: Error adding ICE candidate: {}", current_peer_id_ws, e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("[{}]: Failed to parse signaling message: {}. Content: '{}'", current_peer_id_ws, e, text_content);
                            }
                        }
                    }
                } else if msg.is_close() {
                    info!("[{}]: WebSocket closed by client.", current_peer_id_ws);
                    break;
                }
            }
            Err(e) => {
                warn!("[{}]: WebSocket error: {}", current_peer_id_ws, e);
                break;
            }
        }
    }

    info!("[{}]: WebSocket connection handler for signaling ending.", peer_id_str);
    cleanup_connection(&peer_id_str, &signaling_peers, &player_manager, &data_channels_map, &client_states_map, &player_aois);
    if let Err(e) = peer_connection.close().await {
        error!("[{}]: Error closing PeerConnection: {}", peer_id_str, e);
    }
}

pub fn cleanup_connection(
    peer_id_str: &str,
    signaling_peers: &SignalingPeers,
    player_manager: &PlayerManagerRef, // This is Arc<ImprovedPlayerManager>
    data_channels_map: &DataChannelsMap,
    client_states_map: &ClientStatesMap,
    player_aois: &PlayerAoIs,
) {
    info!("[{}]: Cleaning up resources.", peer_id_str);
    // Check if peer_id was already removed from signaling_peers to prevent double cleanup issues.
    if signaling_peers.lock().unwrap().remove(peer_id_str).is_some() {
        // Only proceed with other removals if this was the first successful removal from signaling_peers
        player_manager.remove_player(peer_id_str); // This is where the warn originates
        data_channels_map.remove(peer_id_str);
        client_states_map.write().remove(peer_id_str); // Assuming client_states_map is Arc<ParkingLotRwLock<HashMap<...>>>
        player_aois.remove(peer_id_str);
        info!("[{}]: Player AoI data removed.", peer_id_str);
    } else {
        debug!("[{}]: Resources already cleaned up or peer not in signaling_peers.", peer_id_str);
    }
}

pub fn handle_dc_send_error(error_string: &str, peer_id_str: &str, message_type: &str) {
    let is_stream_closed_error = error_string.contains("stream closed")
        || error_string.contains("Stream closed")
        || error_string.contains("connection reset")
        || error_string.contains("Channel closed");

    if !is_stream_closed_error {
        error!("[{}]: Error sending {} on data channel: {}", peer_id_str, message_type, error_string);
    }
}
