use anyhow::Context;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::constants::{
    MAX_INPUT_SEQUENCE_GAP, PLAYER_BASE_SPEED, POSITION_VALIDATION_VIOLATION_THRESHOLD,
};
use massive_game_server_core::core::types::{
    PlayerAoIs, PlayerID, PlayerInputData, ServerWeaponType,
};
use massive_game_server_core::flatbuffers_generated::game_protocol as fb;
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientStatesMap, DataChannelsMap, MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::server::instance::MassiveGameServer;

use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock as ParkingLotRwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock as TokioRwLock;
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

fn setup_test_server() -> Arc<MassiveGameServer> {
    let config = Arc::new(ServerConfig::default());
    let thread_pool_system =
        Arc::new(ThreadPoolSystem::new(config.clone()).expect("failed to create thread pools"));
    let data_channels_map: DataChannelsMap = Arc::new(DashMap::new());
    let client_states_map: ClientStatesMap = Arc::new(ParkingLotRwLock::new(HashMap::new()));
    let chat_messages_queue: ChatMessagesQueue =
        Arc::new(TokioRwLock::new(BoundedChatQueue::new(MAX_CHAT_QUEUE_SIZE)));
    let player_aois: PlayerAoIs = Arc::new(DashMap::new());

    Arc::new(MassiveGameServer::new(
        config,
        thread_pool_system,
        data_channels_map,
        client_states_map,
        chat_messages_queue,
        player_aois,
    ))
}

fn add_player(server: &MassiveGameServer, peer_id: &str, team_id: u8, x: f32, y: f32) -> PlayerID {
    server
        .player_manager
        .add_player(peer_id.to_owned(), peer_id.to_owned(), x, y);
    let player_id = server.player_manager.id_pool.get_or_create(peer_id);
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&player_id) {
        ps.team_id = team_id;
        ps.x = x;
        ps.y = y;
        ps.alive = true;
    }
    player_id
}

fn make_input(sequence: u32) -> PlayerInputData {
    PlayerInputData {
        timestamp: 1000 + (sequence as u64 * 16),
        sequence,
        move_forward: false,
        move_backward: false,
        move_left: false,
        move_right: false,
        shooting: false,
        reload: false,
        rotation: 0.0,
        melee_attack: false,
        use_ability_slot: 0,
        change_weapon_slot: 0,
        ping_x: 0.0,
        ping_y: 0.0,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn suspicious_sequence_gap_is_rejected_before_processing() {
    let server = setup_test_server();
    let pid = add_player(&server, "seq-gap", 1, 0.0, 0.0);

    let accepted_first;
    let accepted_gap;
    let queued_len;
    let last_queued_sequence;
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        accepted_first = ps.queue_input(make_input(1));
        accepted_gap = ps.queue_input(make_input(MAX_INPUT_SEQUENCE_GAP + 2));
        queued_len = ps.input_queue.len();
        last_queued_sequence = ps.last_queued_input_sequence;
    } else {
        panic!("missing player state");
    }

    assert!(accepted_first, "initial input should be accepted");
    assert!(!accepted_gap, "suspicious sequence gap should be rejected");
    assert_eq!(queued_len, 1, "rejected input should not remain queued");
    assert_eq!(
        last_queued_sequence, 1,
        "rejected input must not advance accepted sequence"
    );

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert_eq!(
        ps.last_processed_input_sequence, 1,
        "only the accepted input should have been processed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fire_rate_abuse_does_not_create_projectile_before_cooldown() {
    let server = setup_test_server();
    let pid = add_player(&server, "cooldown-abuse", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.shooting = true;
    input.rotation = 0.0;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.weapon = ServerWeaponType::Rifle;
        ps.ammo = 30;
        ps.last_shot_time = Some(Instant::now());
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert_eq!(
        server.projectiles_to_add.len(),
        0,
        "cooldown abuse should not enqueue a projectile"
    );
    assert_eq!(ps.ammo, 30, "rejected shot should not consume ammo");
}

#[tokio::test(flavor = "multi_thread")]
async fn melee_lunge_is_not_counted_as_speed_hack() {
    let server = setup_test_server();
    let pid = add_player(&server, "melee-lunger", 1, 0.0, 0.0);

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.weapon = ServerWeaponType::Melee;
        let mut input = make_input(1);
        input.melee_attack = true;
        input.rotation = 0.0;
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;
    // Melee windup is 90ms; resolve the lunge, then run several validation ticks.
    for _ in 0..12 {
        server.run_physics_update(0.016).await;
    }

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.x > 5.0,
        "melee lunge should move the player forward, got x={} violation_count={} melee_pending={} windup={}",
        ps.x,
        ps.violation_count,
        ps.melee_pending_attack,
        ps.melee_windup_remaining
    );
    assert_eq!(
        ps.violation_count, 0,
        "server-sanctioned melee lunge must not count as a speed-hack violation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn teleported_player_is_clamped_after_violation_threshold() {
    let server = setup_test_server();
    let pid = add_player(&server, "teleporter", 1, 0.0, 0.0);

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.last_valid_position = (0.0, 0.0);
        ps.x = 220.0;
        ps.y = 0.0;
        ps.velocity_x = PLAYER_BASE_SPEED * 4.0;
        ps.velocity_y = 0.0;
        ps.violation_count = POSITION_VALIDATION_VIOLATION_THRESHOLD;
    }

    server.run_physics_update(0.016).await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.x.abs() < f32::EPSILON && ps.y.abs() < f32::EPSILON,
        "teleported player should be snapped back to last valid position, got ({}, {})",
        ps.x,
        ps.y
    );
    assert!(
        ps.velocity_x.abs() < f32::EPSILON && ps.velocity_y.abs() < f32::EPSILON,
        "teleported player should have velocity zeroed after clamp"
    );
    assert!(
        ps.violation_count > POSITION_VALIDATION_VIOLATION_THRESHOLD,
        "violation count should increase when impossible movement is detected"
    );
}

struct TransportServerProcess {
    child: Child,
    base_url: String,
    ws_url: String,
    _data_root: std::path::PathBuf,
}

impl Drop for TransportServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self._data_root);
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SignalingMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp: Option<RTCSessionDescription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ice: Option<IceCandidateJson>,
}

#[derive(Debug, Serialize, Deserialize)]
struct IceCandidateJson {
    candidate: String,
    #[serde(rename = "sdpMid")]
    sdp_mid: Option<String>,
    #[serde(rename = "sdpMLineIndex")]
    sdp_m_line_index: Option<u16>,
    #[serde(rename = "usernameFragment")]
    username_fragment: Option<String>,
}

struct WebRtcSession {
    peer_connection: Arc<webrtc::peer_connection::RTCPeerConnection>,
    data_channel: Arc<webrtc::data_channel::RTCDataChannel>,
    messages_rx: mpsc::UnboundedReceiver<fb::MessageType>,
    signaling_shutdown: Arc<AtomicBool>,
    signaling_task: tokio::task::JoinHandle<()>,
}

impl WebRtcSession {
    async fn connect(ws_url: &str) -> anyhow::Result<Self> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .with_context(|| format!("failed to connect websocket to {ws_url}"))?;
        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .context("failed to register WebRTC codecs")?;
        let api = APIBuilder::new().with_media_engine(media_engine).build();
        let peer_connection = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .context("failed to create peer connection")?,
        );

        let data_channel = peer_connection
            .create_data_channel(
                "gameDataChannel",
                Some(RTCDataChannelInit {
                    ordered: Some(false),
                    max_retransmits: Some(0),
                    ..Default::default()
                }),
            )
            .await
            .context("failed to create data channel")?;

        let dc_open = Arc::new(AtomicBool::new(false));
        let dc_open_notify = Arc::new(Notify::new());
        let (messages_tx, messages_rx) = mpsc::unbounded_channel::<fb::MessageType>();
        let (ice_tx, mut ice_rx) = mpsc::channel::<String>(64);
        let signaling_shutdown = Arc::new(AtomicBool::new(false));

        {
            let dc_open = Arc::clone(&dc_open);
            let dc_open_notify = Arc::clone(&dc_open_notify);
            data_channel.on_open(Box::new(move || {
                dc_open.store(true, AtomicOrdering::SeqCst);
                dc_open_notify.notify_waiters();
                Box::pin(async {})
            }));
        }

        {
            let messages_tx = messages_tx.clone();
            data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
                let messages_tx = messages_tx.clone();
                Box::pin(async move {
                    for payload in unpack_mgsb_batch(&msg.data) {
                        if let Ok(message) = fb::root_as_game_message(payload) {
                            let _ = messages_tx.send(message.msg_type());
                        }
                    }
                })
            }));
        }

        {
            let ice_tx = ice_tx.clone();
            peer_connection.on_ice_candidate(Box::new(
                move |candidate: Option<RTCIceCandidate>| {
                    let ice_tx = ice_tx.clone();
                    Box::pin(async move {
                        if let Some(candidate) = candidate {
                            if let Ok(init) = candidate.to_json() {
                                let msg = SignalingMessage {
                                    protocol_version: Some(
                                        massive_game_server_core::core::constants::GAME_PROTOCOL_VERSION,
                                    ),
                                    sdp: None,
                                    ice: Some(IceCandidateJson {
                                        candidate: init.candidate,
                                        sdp_mid: init.sdp_mid,
                                        sdp_m_line_index: init.sdp_mline_index,
                                        username_fragment: init.username_fragment,
                                    }),
                                };
                                if let Ok(serialized) = serde_json::to_string(&msg) {
                                    let _ = ice_tx.send(serialized).await;
                                }
                            }
                        }
                    })
                },
            ));
        }

        let offer = peer_connection
            .create_offer(None)
            .await
            .context("failed to create offer")?;
        peer_connection
            .set_local_description(offer.clone())
            .await
            .context("failed to set local description")?;
        ws_tx
            .send(WsMessage::Text(
                serde_json::to_string(&SignalingMessage {
                    protocol_version: Some(
                        massive_game_server_core::core::constants::GAME_PROTOCOL_VERSION,
                    ),
                    sdp: Some(offer),
                    ice: None,
                })
                .context("failed to serialize offer")?,
            ))
            .await
            .context("failed to send offer over websocket")?;

        let peer_connection_for_signaling = Arc::clone(&peer_connection);
        let dc_open_for_signaling = Arc::clone(&dc_open);
        let signaling_shutdown_for_task = Arc::clone(&signaling_shutdown);
        let signaling_task = tokio::spawn(async move {
            loop {
                if signaling_shutdown_for_task.load(AtomicOrdering::Relaxed) {
                    break;
                }
                tokio::select! {
                    Some(ice_json) = ice_rx.recv() => {
                        if ws_tx.send(WsMessage::Text(ice_json)).await.is_err() {
                            break;
                        }
                    }
                    msg = ws_rx.next() => {
                        match msg {
                            Some(Ok(WsMessage::Text(text))) => {
                                if let Ok(sig) = serde_json::from_str::<SignalingMessage>(&text) {
                                    if let Some(sdp) = sig.sdp {
                                        if peer_connection_for_signaling.set_remote_description(sdp).await.is_err() {
                                            break;
                                        }
                                    }
                                    if let Some(ice) = sig.ice {
                                        let init = webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
                                            candidate: ice.candidate,
                                            sdp_mid: ice.sdp_mid,
                                            sdp_mline_index: ice.sdp_m_line_index,
                                            username_fragment: ice.username_fragment,
                                        };
                                        let _ = peer_connection_for_signaling.add_ice_candidate(init).await;
                                    }
                                }
                            }
                            Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => break,
                            _ => {}
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(20)), if !dc_open_for_signaling.load(AtomicOrdering::SeqCst) => {
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                }
            }
        });

        tokio::time::timeout(Duration::from_secs(15), dc_open_notify.notified())
            .await
            .context("timed out waiting for data channel open")?;

        Ok(Self {
            peer_connection,
            data_channel,
            messages_rx,
            signaling_shutdown,
            signaling_task,
        })
    }

    async fn send_data(&self, payload: Vec<u8>) -> anyhow::Result<()> {
        self.data_channel
            .send(&bytes::Bytes::from(payload))
            .await
            .context("failed to send data-channel payload")?;
        Ok(())
    }

    async fn wait_for_message_matching<F>(
        &mut self,
        timeout: Duration,
        mut predicate: F,
    ) -> anyhow::Result<fb::MessageType>
    where
        F: FnMut(fb::MessageType) -> bool,
    {
        tokio::time::timeout(timeout, async {
            loop {
                let Some(msg_type) = self.messages_rx.recv().await else {
                    anyhow::bail!("message channel closed before expected message arrived");
                };
                if predicate(msg_type) {
                    return Ok(msg_type);
                }
            }
        })
        .await
        .context("timed out waiting for expected game message")?
    }

    async fn close(self) -> anyhow::Result<()> {
        self.signaling_shutdown.store(true, AtomicOrdering::Relaxed);
        let _ = self.peer_connection.close().await;
        self.signaling_task.abort();
        Ok(())
    }
}

fn reserve_free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

async fn wait_until_ready(base_url: &str) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .expect("build readiness client");
    for _ in 0..120 {
        let ready = client
            .get(format!("{base_url}/readyz"))
            .send()
            .await
            .ok()
            .map(|resp| resp.status().is_success())
            .unwrap_or(false);
        if ready {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("server did not become ready at {base_url}");
}

async fn spawn_transport_server(
    admin_token: &str,
    username: &str,
) -> anyhow::Result<TransportServerProcess> {
    let port = reserve_free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let ws_url = format!("ws://127.0.0.1:{port}/ws?username={username}");
    let data_root = std::env::temp_dir().join(format!("mgs_anti_cheat_transport_{port}"));
    let arena_wasm_dir = data_root.join("arena_wasm");
    let arena_source_dir = data_root.join("arena_sources");
    fs::create_dir_all(&arena_wasm_dir).context("create arena wasm dir")?;
    fs::create_dir_all(&arena_source_dir).context("create arena source dir")?;

    let map_path = data_root.join("empty_map.json");
    fs::write(&map_path, r#"{"walls":[],"pickups":[],"zones":[]}"#).context("write empty map")?;

    let mut command = Command::new(env!("CARGO_BIN_EXE_massive_game_server_core"));
    command
        .env("MGS_HOST", "127.0.0.1")
        .env("MGS_PORT", port.to_string())
        .env("MGS_DISABLE_STUN", "1")
        .env("MGS_TARGET_BOT_COUNT", "0")
        .env("MGS_DIAGNOSTICS_ENABLED", "0")
        .env("MGS_QUIC_PRIMARY", "0")
        .env("MGS_ADMIN_BEARER_TOKEN", admin_token)
        .env("MGS_LIVE_REPLAY_ENABLED", "1")
        .env("MGS_LIVE_REPLAY_PLAYER_CAP", "16")
        .env("MGS_MAP_PATH", &map_path)
        .env("MGS_AUTH_STORE_PATH", data_root.join("auth_store.json"))
        .env(
            "MGS_FEATURE_FLAG_STORE_PATH",
            data_root.join("feature_flags_store.json"),
        )
        .env("MGS_ARENA_STORE_PATH", data_root.join("arena_store.json"))
        .env("MGS_ARENA_WASM_DIR", &arena_wasm_dir)
        .env("MGS_ARENA_SOURCE_DIR", &arena_source_dir)
        .env("MGS_REQUIRE_AUTH", "0")
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command
        .spawn()
        .context("spawn anti-cheat transport server")?;

    let process = TransportServerProcess {
        child,
        base_url,
        ws_url,
        _data_root: data_root,
    };
    wait_until_ready(&process.base_url).await;
    Ok(process)
}

fn unpack_mgsb_batch(data: &[u8]) -> Vec<&[u8]> {
    const MAGIC: &[u8; 4] = b"MGSB";
    const HEADER_LEN: usize = 7;

    if data.len() >= HEADER_LEN && &data[..4] == MAGIC {
        let count = u16::from_le_bytes([data[5], data[6]]) as usize;
        let mut offset = HEADER_LEN;
        let mut payloads = Vec::with_capacity(count);
        for _ in 0..count {
            if offset + 4 > data.len() {
                break;
            }
            let len = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            offset += 4;
            if offset + len > data.len() {
                break;
            }
            payloads.push(&data[offset..offset + len]);
            offset += len;
        }
        payloads
    } else {
        vec![data]
    }
}

fn build_transport_input(sequence: u32, rotation: f32, move_forward: bool) -> Vec<u8> {
    let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(128);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let input = fb::PlayerInput::create(
        &mut builder,
        &fb::PlayerInputArgs {
            timestamp: now_ms,
            sequence,
            move_forward,
            move_backward: false,
            move_left: false,
            move_right: false,
            shooting: false,
            reload: false,
            rotation,
            melee_attack: false,
            change_weapon_slot: 0,
            use_ability_slot: 0,
            ping_x: 0.0,
            ping_y: 0.0,
        },
    );
    let game_message = fb::GameMessage::create(
        &mut builder,
        &fb::GameMessageArgs {
            msg_type: fb::MessageType::Input,
            actual_message_type: fb::MessagePayload::PlayerInput,
            actual_message: Some(input.as_union_value()),
            protocol_version: massive_game_server_core::core::constants::GAME_PROTOCOL_VERSION,
        },
    );
    builder.finish(game_message, None);
    builder.finished_data().to_vec()
}

async fn wait_for_live_replay_position(
    base_url: &str,
    admin_token: &str,
    username: &str,
) -> anyhow::Result<(f32, f32)> {
    let client = reqwest::Client::new();
    for _ in 0..40 {
        let response = client
            .get(format!("{base_url}/api/ops/live-replay/recent?limit=12"))
            .bearer_auth(admin_token)
            .send()
            .await
            .context("request live replay frames")?;
        let body: Value = response.json().await.context("live replay json")?;
        if let Some(frames) = body["frames"].as_array() {
            for frame in frames {
                if let Some(players) = frame["sampled_players"].as_array() {
                    for player in players {
                        if player["username"].as_str() == Some(username) {
                            let x = player["x"].as_f64().unwrap_or_default() as f32;
                            let y = player["y"].as_f64().unwrap_or_default() as f32;
                            return Ok((x, y));
                        }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    anyhow::bail!("timed out waiting for live replay sample for {username}")
}

/// Poll until the newest live-replay sample for `username` shows x above
/// `threshold`. Replay sampling cadence is not synchronized with input, so a
/// fixed sleep after sending input is inherently flaky — wait for the
/// condition instead (bounded, so a genuine regression still fails fast).
async fn wait_for_live_replay_x_above(
    base_url: &str,
    admin_token: &str,
    username: &str,
    threshold: f32,
    timeout: Duration,
) -> anyhow::Result<f32> {
    let started = std::time::Instant::now();
    let mut last_x = f32::NAN;
    loop {
        if let Ok((x, _)) = wait_for_live_replay_position(base_url, admin_token, username).await {
            last_x = x;
            if x > threshold {
                return Ok(x);
            }
        }
        if started.elapsed() >= timeout {
            anyhow::bail!(
                "timed out waiting for {username} x > {threshold} (last sample {last_x})"
            );
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_sequence_over_webrtc_does_not_reverse_movement() -> anyhow::Result<()> {
    let admin_token = "integration-anti-cheat-admin-token";
    let username = "anti-cheat-dup";
    let process = spawn_transport_server(admin_token, username).await?;
    let mut session = WebRtcSession::connect(&process.ws_url).await?;
    session
        .wait_for_message_matching(Duration::from_secs(5), |msg_type| {
            matches!(msg_type, fb::MessageType::Welcome)
        })
        .await?;
    session
        .wait_for_message_matching(Duration::from_secs(5), |msg_type| {
            matches!(
                msg_type,
                fb::MessageType::InitialState | fb::MessageType::DeltaState
            )
        })
        .await?;

    let (x0, _) = wait_for_live_replay_position(&process.base_url, admin_token, username).await?;
    session
        .send_data(build_transport_input(1, 0.0, true))
        .await?;
    let x1 = wait_for_live_replay_x_above(
        &process.base_url,
        admin_token,
        username,
        x0 + 5.0,
        Duration::from_secs(8),
    )
    .await?;
    assert!(
        x1 > x0 + 5.0,
        "valid forward input should move the player forward, before={x0}, after={x1}"
    );

    session
        .send_data(build_transport_input(1, std::f32::consts::PI, true))
        .await?;
    tokio::time::sleep(Duration::from_millis(350)).await;
    let (x2, _) = wait_for_live_replay_position(&process.base_url, admin_token, username).await?;
    assert!(
        x2 >= x1 - 1.0,
        "duplicate sequence should be ignored and must not reverse movement, before={x1}, after={x2}"
    );

    session.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sequence_gap_over_webrtc_does_not_reverse_movement() -> anyhow::Result<()> {
    let admin_token = "integration-anti-cheat-gap-admin-token";
    let username = "anti-cheat-gap";
    let process = spawn_transport_server(admin_token, username).await?;
    let mut session = WebRtcSession::connect(&process.ws_url).await?;
    session
        .wait_for_message_matching(Duration::from_secs(5), |msg_type| {
            matches!(msg_type, fb::MessageType::Welcome)
        })
        .await?;
    session
        .wait_for_message_matching(Duration::from_secs(5), |msg_type| {
            matches!(
                msg_type,
                fb::MessageType::InitialState | fb::MessageType::DeltaState
            )
        })
        .await?;

    let (x0, _) = wait_for_live_replay_position(&process.base_url, admin_token, username).await?;
    session
        .send_data(build_transport_input(1, 0.0, true))
        .await?;
    let x1 = wait_for_live_replay_x_above(
        &process.base_url,
        admin_token,
        username,
        x0 + 5.0,
        Duration::from_secs(8),
    )
    .await?;

    session
        .send_data(build_transport_input(
            MAX_INPUT_SEQUENCE_GAP + 5,
            std::f32::consts::PI,
            true,
        ))
        .await?;
    tokio::time::sleep(Duration::from_millis(350)).await;
    let (x2, _) = wait_for_live_replay_position(&process.base_url, admin_token, username).await?;
    assert!(
        x2 >= x1 - 1.0,
        "suspicious sequence gap should be ignored and must not reverse movement, before={x1}, after={x2}"
    );

    session.close().await?;
    Ok(())
}
