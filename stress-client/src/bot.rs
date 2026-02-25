use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{debug, error, info, warn};
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

use massive_game_server_protocol::game_protocol as fb;

use crate::metrics::ScenarioMetrics;

// Must match server's GAME_PROTOCOL_VERSION in constants.rs
const GAME_PROTOCOL_VERSION: u32 = 1;

/// Signaling message format exchanged over the WebSocket (JSON).
/// Mirrors the server's `SignalingMessageJson` struct.
#[derive(Debug, Serialize, Deserialize)]
struct SignalingMessage {
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

/// Runs a single stress-test bot.
///
/// 1. Connects WebSocket to server `/ws`
/// 2. Creates an RTCPeerConnection + data channel "gameDataChannel"
/// 3. Performs SDP offer/answer exchange over the WebSocket
/// 4. Waits for data channel to open
/// 5. Receives WelcomeMessage + DeltaState; sends PlayerInput at ~20Hz
/// 6. Runs until `shutdown` is signalled or `run_duration` elapses
pub async fn run_bot(
    bot_id: usize,
    server_url: &str,
    metrics: Arc<ScenarioMetrics>,
    shutdown: Arc<AtomicBool>,
    run_duration: Duration,
) -> Result<()> {
    let username = format!("StressBot_{:04}", bot_id);
    metrics.register_bot(bot_id, &username).await;

    let connect_start = Instant::now();

    // --- 1. WebSocket signaling connection ---
    let ws_url = format!("{}", server_url);
    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .with_context(|| format!("bot#{}: WebSocket connect to {}", bot_id, ws_url))?;

    info!("bot#{}: WebSocket connected to {}", bot_id, ws_url);
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // --- 2. Create WebRTC peer connection + data channel ---
    let mut media_engine = MediaEngine::default();
    media_engine.register_default_codecs()?;

    let api = APIBuilder::new().with_media_engine(media_engine).build();

    let rtc_config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            ..Default::default()
        }],
        ..Default::default()
    };

    let peer_connection = Arc::new(api.new_peer_connection(rtc_config).await?);

    // Create the data channel before creating the offer (same as the JS client).
    let dc_init = webrtc::data_channel::data_channel_init::RTCDataChannelInit {
        ordered: Some(false),
        max_retransmits: Some(0),
        ..Default::default()
    };
    let data_channel = peer_connection
        .create_data_channel("gameDataChannel", Some(dc_init))
        .await?;

    info!("bot#{}: DataChannel created", bot_id);

    // Shared state between callbacks
    let dc_open = Arc::new(AtomicBool::new(false));
    let dc_open_notify = Arc::new(Notify::new());
    let welcome_received = Arc::new(AtomicBool::new(false));
    let input_sequence = Arc::new(AtomicU32::new(0));

    // --- Data channel on_open callback ---
    {
        let dc_open = dc_open.clone();
        let dc_open_notify = dc_open_notify.clone();
        let metrics = metrics.clone();
        let connect_start = connect_start;
        let bot_id = bot_id;

        data_channel.on_open(Box::new(move || {
            info!("bot#{}: DataChannel OPENED", bot_id);
            dc_open.store(true, Ordering::SeqCst);
            dc_open_notify.notify_waiters();
            let latency = connect_start.elapsed();
            let metrics = metrics.clone();
            Box::pin(async move {
                metrics.mark_dc_open(bot_id, latency).await;
            })
        }));
    }

    // --- Data channel on_message callback ---
    {
        let metrics = metrics.clone();
        let welcome_received = welcome_received.clone();
        let connect_start = connect_start;
        let bot_id = bot_id;

        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let metrics = metrics.clone();
            let welcome_received = welcome_received.clone();
            Box::pin(async move {
                if let Ok(game_msg) = fb::root_as_game_message(&msg.data) {
                    match game_msg.msg_type() {
                        fb::MessageType::Welcome => {
                            if !welcome_received.swap(true, Ordering::SeqCst) {
                                let latency = connect_start.elapsed();
                                info!(
                                    "bot#{}: Welcome received (latency={:.0}ms)",
                                    bot_id,
                                    latency.as_secs_f64() * 1000.0
                                );
                                metrics.mark_connected(bot_id, latency).await;
                            }
                        }
                        fb::MessageType::InitialState => {
                            debug!("bot#{}: InitialState received", bot_id);
                            metrics.record_delta(bot_id).await;
                        }
                        fb::MessageType::DeltaState => {
                            metrics.record_delta(bot_id).await;
                        }
                        fb::MessageType::MatchUpdate => {
                            debug!("bot#{}: MatchUpdate received", bot_id);
                        }
                        _ => {
                            debug!(
                                "bot#{}: unhandled msg_type {:?}",
                                bot_id,
                                game_msg.msg_type()
                            );
                        }
                    }
                }
            })
        }));
    }

    // --- ICE candidate trickle: send candidates to server via WS ---
    let (ice_tx, mut ice_rx) = tokio::sync::mpsc::channel::<String>(64);

    {
        let ice_tx = ice_tx.clone();
        let bot_id = bot_id;
        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let ice_tx = ice_tx.clone();
            Box::pin(async move {
                if let Some(c) = candidate {
                    match c.to_json() {
                        Ok(init) => {
                            let ice_json = IceCandidateJson {
                                candidate: init.candidate,
                                sdp_mid: init.sdp_mid,
                                sdp_m_line_index: init.sdp_mline_index,
                                username_fragment: init.username_fragment,
                            };
                            let msg = SignalingMessage {
                                sdp: None,
                                ice: Some(ice_json),
                            };
                            if let Ok(json) = serde_json::to_string(&msg) {
                                let _ = ice_tx.send(json).await;
                            }
                        }
                        Err(e) => {
                            warn!("bot#{}: ICE candidate to_json error: {}", bot_id, e);
                        }
                    }
                }
            })
        }));
    }

    // --- Peer connection state change monitoring ---
    {
        let shutdown = shutdown.clone();
        let metrics = metrics.clone();
        let bot_id = bot_id;
        peer_connection.on_peer_connection_state_change(Box::new(
            move |s: RTCPeerConnectionState| {
                info!("bot#{}: PeerConnection state: {}", bot_id, s);
                let _shutdown = shutdown.clone();
                let metrics = metrics.clone();
                Box::pin(async move {
                    if matches!(
                        s,
                        RTCPeerConnectionState::Failed
                            | RTCPeerConnectionState::Closed
                            | RTCPeerConnectionState::Disconnected
                    ) {
                        metrics
                            .mark_disconnected(bot_id, &format!("PeerConnection {}", s))
                            .await;
                    }
                })
            },
        ));
    }

    // --- 3. Create SDP offer and send to server ---
    let offer = peer_connection.create_offer(None).await?;
    peer_connection.set_local_description(offer.clone()).await?;

    let offer_msg = SignalingMessage {
        sdp: Some(offer),
        ice: None,
    };
    let offer_json = serde_json::to_string(&offer_msg)?;
    ws_tx.send(WsMessage::Text(offer_json)).await?;
    info!("bot#{}: SDP offer sent", bot_id);

    // --- 4. Process signaling messages (SDP answer + ICE candidates) ---
    // Run signaling + ICE trickle concurrently until data channel opens or timeout.
    let deadline = Instant::now() + Duration::from_secs(30);

    let signaling_task = {
        let peer_connection = peer_connection.clone();
        let dc_open = dc_open.clone();
        let dc_open_notify = dc_open_notify.clone();
        let bot_id = bot_id;

        tokio::spawn(async move {
            loop {
                if dc_open.load(Ordering::SeqCst) {
                    break;
                }

                tokio::select! {
                    // Forward ICE candidates from our peer to the server
                    Some(ice_json) = ice_rx.recv() => {
                        if let Err(e) = ws_tx.send(WsMessage::Text(ice_json)).await {
                            warn!("bot#{}: WS send ICE error: {}", bot_id, e);
                            break;
                        }
                    }
                    // Receive signaling messages from server
                    msg = ws_rx.next() => {
                        match msg {
                            Some(Ok(WsMessage::Text(text))) => {
                                // Check for error messages
                                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                                    if value.get("error").is_some() {
                                        let detail = value.get("detail")
                                            .and_then(|d| d.as_str())
                                            .unwrap_or("unknown");
                                        error!("bot#{}: Server error: {}", bot_id, detail);
                                        anyhow::bail!("Server rejected: {}", detail);
                                    }
                                    // Skip non-signaling messages like sdp_offer_queue
                                    if value.get("event").is_some() {
                                        debug!("bot#{}: signaling event: {}", bot_id, text);
                                        continue;
                                    }
                                }

                                match serde_json::from_str::<SignalingMessage>(&text) {
                                    Ok(sig) => {
                                        if let Some(sdp) = sig.sdp {
                                            debug!("bot#{}: received SDP {}", bot_id, sdp.sdp_type);
                                            peer_connection.set_remote_description(sdp).await
                                                .unwrap_or_else(|e| error!("bot#{}: set_remote_description error: {}", bot_id, e));
                                        }
                                        if let Some(ice) = sig.ice {
                                            let init = webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
                                                candidate: ice.candidate,
                                                sdp_mid: ice.sdp_mid,
                                                sdp_mline_index: ice.sdp_m_line_index,
                                                username_fragment: ice.username_fragment,
                                            };
                                            let _ = peer_connection.add_ice_candidate(init).await;
                                        }
                                    }
                                    Err(e) => {
                                        debug!("bot#{}: non-signaling WS msg (parse err: {})", bot_id, e);
                                    }
                                }
                            }
                            Some(Ok(WsMessage::Close(_))) => {
                                info!("bot#{}: WS closed by server during signaling", bot_id);
                                break;
                            }
                            Some(Err(e)) => {
                                warn!("bot#{}: WS receive error: {}", bot_id, e);
                                break;
                            }
                            None => {
                                info!("bot#{}: WS stream ended", bot_id);
                                break;
                            }
                            _ => {}
                        }
                    }
                    // Timeout
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                        warn!("bot#{}: signaling timeout", bot_id);
                        break;
                    }
                    // Data channel opened
                    _ = dc_open_notify.notified() => {
                        break;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        })
    };

    // Wait for data channel to open
    tokio::select! {
        _ = dc_open_notify.notified(), if !dc_open.load(Ordering::SeqCst) => {}
        _ = tokio::time::sleep(Duration::from_secs(30)) => {
            if !dc_open.load(Ordering::SeqCst) {
                metrics
                    .mark_disconnected(bot_id, "DataChannel open timeout")
                    .await;
                warn!("bot#{}: DataChannel open timeout after 30s", bot_id);
                let _ = peer_connection.close().await;
                return Ok(());
            }
        }
    }

    if !dc_open.load(Ordering::SeqCst) {
        metrics
            .mark_disconnected(bot_id, "DataChannel never opened")
            .await;
        let _ = peer_connection.close().await;
        return Ok(());
    }

    info!("bot#{}: DataChannel open, starting gameplay loop", bot_id);

    // --- 5. Gameplay loop: send random inputs at ~20Hz ---
    let run_end = Instant::now() + run_duration;
    let mut input_ticker = tokio::time::interval(Duration::from_millis(50)); // 20Hz
    let mut rng = StdRng::from_entropy();

    loop {
        if shutdown.load(Ordering::Relaxed) || Instant::now() >= run_end {
            break;
        }

        input_ticker.tick().await;

        let seq = input_sequence.fetch_add(1, Ordering::Relaxed);
        let input_bytes = build_player_input(seq, &mut rng);

        match data_channel.send(&bytes::Bytes::from(input_bytes)).await {
            Ok(_) => {
                metrics.record_input_sent(bot_id).await;
            }
            Err(e) => {
                warn!("bot#{}: DC send error: {}", bot_id, e);
                metrics
                    .mark_disconnected(bot_id, &format!("DC send error: {}", e))
                    .await;
                break;
            }
        }
    }

    info!("bot#{}: shutting down", bot_id);
    let _ = peer_connection.close().await;
    // The signaling task will end when the WS connection drops.
    signaling_task.abort();
    Ok(())
}

/// Build a FlatBuffers PlayerInput message with random movement/shooting.
fn build_player_input(sequence: u32, rng: &mut impl Rng) -> Vec<u8> {
    let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(128);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let rotation: f32 = rng.gen_range(0.0..std::f32::consts::TAU);
    let shooting = rng.gen_bool(0.3);
    let move_forward = rng.gen_bool(0.6);
    let move_backward = !move_forward && rng.gen_bool(0.2);
    let move_left = rng.gen_bool(0.3);
    let move_right = !move_left && rng.gen_bool(0.3);
    let melee_attack = rng.gen_bool(0.05);
    let change_weapon_slot: i8 = if rng.gen_bool(0.02) {
        rng.gen_range(0..5)
    } else {
        0
    };

    let input_args = fb::PlayerInputArgs {
        timestamp: now_ms,
        sequence,
        move_forward,
        move_backward,
        move_left,
        move_right,
        shooting,
        reload: rng.gen_bool(0.05),
        rotation,
        melee_attack,
        change_weapon_slot,
        use_ability_slot: 0,
        ping_x: 0.0,
        ping_y: 0.0,
    };
    let input = fb::PlayerInput::create(&mut builder, &input_args);

    let game_msg_args = fb::GameMessageArgs {
        msg_type: fb::MessageType::Input,
        actual_message_type: fb::MessagePayload::PlayerInput,
        actual_message: Some(input.as_union_value()),
        protocol_version: GAME_PROTOCOL_VERSION,
    };
    let game_msg = fb::GameMessage::create(&mut builder, &game_msg_args);
    builder.finish(game_msg, None);

    builder.finished_data().to_vec()
}
