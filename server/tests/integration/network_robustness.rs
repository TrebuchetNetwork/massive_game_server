use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use massive_game_server_core::core::constants::GAME_PROTOCOL_VERSION;
use massive_game_server_core::flatbuffers_generated::game_protocol as fb;
use massive_game_server_core::network::quic::{start_quic_runtime, QuicEndpointConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

struct ServerProcess {
    child: Child,
    base_url: String,
    ws_url: String,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

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

struct WebRtcSession {
    peer_connection: Arc<webrtc::peer_connection::RTCPeerConnection>,
    data_channel: Arc<webrtc::data_channel::RTCDataChannel>,
    messages_rx: mpsc::UnboundedReceiver<fb::MessageType>,
    signaling_shutdown: Arc<AtomicBool>,
    signaling_task: tokio::task::JoinHandle<()>,
}

fn quic_test_mutex() -> &'static tokio::sync::Mutex<()> {
    static QUIC_TEST_MUTEX: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    QUIC_TEST_MUTEX.get_or_init(|| tokio::sync::Mutex::new(()))
}

impl WebRtcSession {
    async fn connect(ws_url: &str) -> Result<Self> {
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
                dc_open.store(true, Ordering::SeqCst);
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
                if signaling_shutdown_for_task.load(Ordering::Relaxed) {
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
                                        if peer_connection_for_signaling
                                            .set_remote_description(sdp)
                                            .await
                                            .is_err()
                                        {
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
                    _ = tokio::time::sleep(Duration::from_secs(20)), if !dc_open_for_signaling.load(Ordering::SeqCst) => {
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

    async fn send_data(&self, payload: Vec<u8>) -> Result<()> {
        self.data_channel
            .send(&Bytes::from(payload))
            .await
            .context("failed to send data-channel payload")?;
        Ok(())
    }

    fn drain_messages(&mut self) {
        while self.messages_rx.try_recv().is_ok() {}
    }

    async fn wait_for_message_matching<F>(
        &mut self,
        timeout: Duration,
        mut predicate: F,
    ) -> Result<fb::MessageType>
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

    async fn close(self) -> Result<()> {
        self.signaling_shutdown.store(true, Ordering::Relaxed);
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

async fn spawn_server() -> ServerProcess {
    let port = reserve_free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let ws_url = format!("ws://127.0.0.1:{port}/ws?username=robustness");
    let data_root = std::env::temp_dir().join(format!("mgs_network_robustness_{port}"));
    let arena_wasm_dir = data_root.join("arena_wasm");
    let arena_source_dir = data_root.join("arena_sources");
    let _ = fs::create_dir_all(&arena_wasm_dir);
    let _ = fs::create_dir_all(&arena_source_dir);

    let mut command = Command::new(env!("CARGO_BIN_EXE_massive_game_server_core"));
    command
        .env("MGS_HOST", "127.0.0.1")
        .env("MGS_PORT", port.to_string())
        .env("MGS_DISABLE_STUN", "1")
        .env("MGS_TARGET_BOT_COUNT", "0")
        .env("MGS_DIAGNOSTICS_ENABLED", "0")
        .env("MGS_QUIC_PRIMARY", "0")
        .env("MGS_AUTH_STORE_PATH", data_root.join("auth_store.json"))
        .env(
            "MGS_FEATURE_FLAG_STORE_PATH",
            data_root.join("feature_flags_store.json"),
        )
        .env("MGS_ARENA_STORE_PATH", data_root.join("arena_store.json"))
        .env("MGS_ARENA_WASM_DIR", &arena_wasm_dir)
        .env("MGS_ARENA_SOURCE_DIR", &arena_source_dir)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command.spawn().expect("spawn server binary");

    let process = ServerProcess {
        child,
        base_url,
        ws_url,
    };
    wait_until_ready(&process.base_url).await;
    process
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

fn build_player_input(sequence: u32, protocol_version: u32) -> Vec<u8> {
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
            move_forward: true,
            move_backward: false,
            move_left: false,
            move_right: false,
            shooting: false,
            reload: false,
            rotation: 0.0,
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
            protocol_version,
        },
    );
    builder.finish(game_message, None);
    builder.finished_data().to_vec()
}

fn write_test_quic_identity() -> Result<(PathBuf, PathBuf, PathBuf, Vec<u8>)> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
        .context("failed to generate self-signed test certificate")?;
    let cert_der = cert.cert.der().to_vec();
    let key_der = cert.key_pair.serialize_der();

    let suffix = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let temp_dir = std::env::temp_dir().join(format!("mgs-network-robustness-{suffix}"));
    fs::create_dir_all(&temp_dir).context("failed to create temp identity directory")?;

    let cert_path = temp_dir.join("cert.der");
    let key_path = temp_dir.join("key.der");
    fs::write(&cert_path, &cert_der).context("failed to write temp cert")?;
    fs::write(&key_path, &key_der).context("failed to write temp key")?;

    Ok((temp_dir, cert_path, key_path, cert_der))
}

async fn connect_quic_client(
    local_addr: SocketAddr,
    cert_der: Vec<u8>,
) -> Result<(quinn::Endpoint, quinn::Connection)> {
    let mut roots = quinn::rustls::RootCertStore::empty();
    roots
        .add(quinn::rustls::pki_types::CertificateDer::from(cert_der))
        .context("failed to trust generated QUIC certificate")?;
    let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots))
        .context("failed to build QUIC client config")?;
    let mut endpoint = quinn::Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .context("failed to create QUIC client endpoint")?;
    endpoint.set_default_client_config(client_config);
    let connection = endpoint
        .connect(local_addr, "localhost")
        .context("failed to begin QUIC connection")?
        .await
        .context("failed to establish QUIC connection")?;
    Ok((endpoint, connection))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_flatbuffer_payload_does_not_close_webrtc_session() -> Result<()> {
    let process = spawn_server().await;
    let mut session = WebRtcSession::connect(&process.ws_url).await?;
    session
        .wait_for_message_matching(Duration::from_secs(5), |msg_type| {
            matches!(msg_type, fb::MessageType::Welcome)
        })
        .await?;
    session.drain_messages();

    session.send_data(vec![0x13, 0x37, 0x42, 0x99]).await?;
    session
        .send_data(build_player_input(1, GAME_PROTOCOL_VERSION))
        .await?;

    session
        .wait_for_message_matching(Duration::from_secs(5), |msg_type| {
            matches!(
                msg_type,
                fb::MessageType::InitialState | fb::MessageType::DeltaState
            )
        })
        .await?;
    session.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_version_mismatch_does_not_close_webrtc_session() -> Result<()> {
    let process = spawn_server().await;
    let mut session = WebRtcSession::connect(&process.ws_url).await?;
    session
        .wait_for_message_matching(Duration::from_secs(5), |msg_type| {
            matches!(msg_type, fb::MessageType::Welcome)
        })
        .await?;
    session.drain_messages();

    session
        .send_data(build_player_input(1, GAME_PROTOCOL_VERSION + 1))
        .await?;
    session
        .send_data(build_player_input(2, GAME_PROTOCOL_VERSION))
        .await?;

    session
        .wait_for_message_matching(Duration::from_secs(5), |msg_type| {
            matches!(
                msg_type,
                fb::MessageType::InitialState | fb::MessageType::DeltaState
            )
        })
        .await?;
    session.close().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn quic_oversized_stream_payload_is_rejected() -> Result<()> {
    let _guard = quic_test_mutex().lock().await;
    let (temp_dir, cert_path, key_path, cert_der) = write_test_quic_identity()?;
    let cert_path_raw = cert_path
        .to_str()
        .context("cert path is not valid UTF-8")?
        .to_owned();
    let key_path_raw = key_path
        .to_str()
        .context("key path is not valid UTF-8")?
        .to_owned();

    let test_result = temp_env::async_with_vars(
        [
            ("MGS_QUIC_CERT_PATH", Some(cert_path_raw.as_str())),
            ("MGS_QUIC_KEY_PATH", Some(key_path_raw.as_str())),
        ],
        async move {
            let runtime = start_quic_runtime(
                &QuicEndpointConfig {
                    bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    max_concurrent_bidi_streams: 8,
                    max_stream_payload_bytes: 4096,
                },
                None,
            )
            .context("failed to start QUIC runtime")?;

            let (endpoint, connection) =
                connect_quic_client(runtime.local_addr(), cert_der).await?;
            let (mut send, mut recv) = connection
                .open_bi()
                .await
                .context("failed to open client bidi stream")?;
            send.write_all(&vec![b'x'; 8192])
                .await
                .context("failed to write oversized payload")?;
            send.finish().context("failed finishing client stream")?;

            match recv.read_to_end(16 * 1024).await {
                Err(_) => {}
                Ok(response) => assert!(
                    response.is_empty(),
                    "oversized QUIC stream should not yield a non-empty response"
                ),
            }

            connection.close(0u32.into(), b"done");
            endpoint.wait_idle().await;
            drop(runtime);
            Ok(())
        },
    )
    .await;

    let _ = fs::remove_dir_all(temp_dir);
    test_result
}

#[tokio::test(flavor = "multi_thread")]
async fn quic_connection_rate_limit_rejects_immediate_burst() -> Result<()> {
    let _guard = quic_test_mutex().lock().await;
    let (temp_dir, cert_path, key_path, cert_der) = write_test_quic_identity()?;
    let cert_path_raw = cert_path
        .to_str()
        .context("cert path is not valid UTF-8")?
        .to_owned();
    let key_path_raw = key_path
        .to_str()
        .context("key path is not valid UTF-8")?
        .to_owned();

    let test_result = temp_env::async_with_vars(
        [
            ("MGS_QUIC_CERT_PATH", Some(cert_path_raw.as_str())),
            ("MGS_QUIC_KEY_PATH", Some(key_path_raw.as_str())),
            ("MGS_QUIC_CONN_RATE_PER_SEC", Some("1")),
            ("MGS_QUIC_CONN_RATE_BURST", Some("1")),
        ],
        async move {
            let runtime = start_quic_runtime(
                &QuicEndpointConfig {
                    bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    max_concurrent_bidi_streams: 8,
                    max_stream_payload_bytes: 4096,
                },
                None,
            )
            .context("failed to start QUIC runtime")?;

            let (endpoint, connection) =
                connect_quic_client(runtime.local_addr(), cert_der.clone()).await?;

            let second_attempt = tokio::time::timeout(Duration::from_secs(2), async {
                let connect = endpoint
                    .connect(runtime.local_addr(), "localhost")
                    .context("failed to begin second QUIC connection")?;
                connect
                    .await
                    .context("second QUIC connection unexpectedly succeeded")
            })
            .await;

            match second_attempt {
                Ok(Ok(_)) => anyhow::bail!("second QUIC connection unexpectedly succeeded"),
                Ok(Err(_)) | Err(_) => {}
            }

            connection.close(0u32.into(), b"done");
            endpoint.wait_idle().await;
            drop(runtime);
            Ok(())
        },
    )
    .await;

    let _ = fs::remove_dir_all(temp_dir);
    test_result
}
