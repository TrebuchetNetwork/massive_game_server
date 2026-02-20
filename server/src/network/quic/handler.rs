// massive_game_server/server/src/network/quic/handler.rs

use crate::network::connection_manager::{
    shared_connection_manager, ConnectionInfo, TransportKind,
};
use crate::operational::monitoring::metrics;
use crate::operational::monitoring::tracing as monitoring_tracing;
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use dashmap::DashMap;
use quinn::{Connecting, Endpoint, RecvStream, SendStream};
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tracing::{debug, info, warn, Instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub type QuicRequestHandler = Arc<dyn Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync + 'static>;

#[derive(Debug, Clone)]
pub struct QuicEndpointConfig {
    pub bind_addr: SocketAddr,
    pub max_concurrent_bidi_streams: u32,
    pub max_stream_payload_bytes: usize,
}

#[derive(Debug)]
pub struct QuicRuntime {
    endpoint: Endpoint,
    local_addr: SocketAddr,
}

#[derive(Clone)]
struct QuicPeerSender {
    connection_token: u64,
    outbound_tx: mpsc::UnboundedSender<Bytes>,
}

static QUIC_PEER_SENDERS: OnceLock<DashMap<String, QuicPeerSender>> = OnceLock::new();
static QUIC_CONNECTION_TOKEN: AtomicU64 = AtomicU64::new(1);

impl QuicRuntime {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    #[allow(dead_code)]
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

impl QuicEndpointConfig {
    pub fn from_env(default_bind_addr: SocketAddr) -> Self {
        let bind_addr = std::env::var("MGS_QUIC_BIND_ADDR")
            .ok()
            .and_then(|raw| raw.parse::<SocketAddr>().ok())
            .unwrap_or(default_bind_addr);
        let max_concurrent_bidi_streams = std::env::var("MGS_QUIC_MAX_BIDI")
            .ok()
            .and_then(|raw| raw.parse::<u32>().ok())
            .unwrap_or(1024);
        let max_stream_payload_bytes = std::env::var("MGS_QUIC_MAX_STREAM_PAYLOAD_BYTES")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(128 * 1024)
            .clamp(4 * 1024, 2 * 1024 * 1024);
        Self {
            bind_addr,
            max_concurrent_bidi_streams,
            max_stream_payload_bytes,
        }
    }
}

fn shared_quic_peer_senders() -> &'static DashMap<String, QuicPeerSender> {
    QUIC_PEER_SENDERS.get_or_init(DashMap::new)
}

pub fn connected_quic_peer_count() -> usize {
    shared_quic_peer_senders().len()
}

pub fn connected_quic_peer_ids() -> Vec<String> {
    shared_quic_peer_senders()
        .iter()
        .map(|entry| entry.key().clone())
        .collect()
}

pub fn send_quic_packet_batch(peer_id: &str, packets: &[Bytes]) -> usize {
    if packets.is_empty() {
        return 0;
    }

    let Some(sender) = shared_quic_peer_senders()
        .get(peer_id)
        .map(|entry| entry.value().clone())
    else {
        return 0;
    };

    let mut sent = 0usize;
    for packet in packets {
        if sender.outbound_tx.send(packet.clone()).is_ok() {
            sent += 1;
        } else {
            break;
        }
    }

    if sent == 0 {
        let _ = shared_quic_peer_senders().remove(peer_id);
    }
    sent
}

pub fn quic_enabled() -> bool {
    env_flag("MGS_QUIC_PRIMARY")
}

fn env_flag(var_name: &str) -> bool {
    std::env::var(var_name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

fn allow_self_signed_quic_identity_fallback() -> bool {
    cfg!(debug_assertions)
        || env_flag("MGS_QUIC_ALLOW_SELF_SIGNED_FALLBACK")
        || env_flag("QUIC_ALLOW_SELF_SIGNED_FALLBACK")
}

pub fn validate_quic_config(config: &QuicEndpointConfig) -> Result<()> {
    if config.max_concurrent_bidi_streams == 0 {
        return Err(anyhow!("max_concurrent_bidi_streams must be > 0"));
    }
    if config.max_stream_payload_bytes == 0 {
        return Err(anyhow!("max_stream_payload_bytes must be > 0"));
    }
    Ok(())
}

pub fn start_quic_runtime(
    config: &QuicEndpointConfig,
    request_handler: Option<QuicRequestHandler>,
) -> Result<QuicRuntime> {
    validate_quic_config(config)?;

    let (cert_chain, key) = match load_quic_identity_from_env()
        .context("failed loading QUIC certificate/key from configured paths")?
    {
        Some(identity) => identity,
        None => {
            if !allow_self_signed_quic_identity_fallback() {
                return Err(anyhow!(
                    "QUIC certificates are required in this build. Set MGS_QUIC_CERT_PATH and \
                     MGS_QUIC_KEY_PATH, or explicitly allow fallback with \
                     MGS_QUIC_ALLOW_SELF_SIGNED_FALLBACK=1 for non-production usage."
                ));
            }
            warn!(
                "QUIC identity files were not configured; falling back to a self-signed certificate. \
                 This should only be used in local/dev environments."
            );
            let certified_key = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
                .context("failed to generate self-signed QUIC certificate")?;
            let cert_der = certified_key.cert.der().to_vec();
            let key_der = certified_key.key_pair.serialize_der();
            (
                vec![rustls::Certificate(cert_der)],
                rustls::PrivateKey(key_der),
            )
        }
    };

    let mut server_config = quinn::ServerConfig::with_single_cert(cert_chain, key)
        .context("failed to create QUIC server config")?;
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(config.max_concurrent_bidi_streams.into());
    server_config.transport = Arc::new(transport);

    let endpoint = quinn::Endpoint::server(server_config, config.bind_addr)
        .context("failed to bind QUIC endpoint")?;
    let local_addr = endpoint
        .local_addr()
        .context("failed to read QUIC local address")?;

    let endpoint_for_accept = endpoint.clone();
    let max_stream_payload_bytes = config.max_stream_payload_bytes;
    tokio::spawn(async move {
        loop {
            let Some(connecting) = endpoint_for_accept.accept().await else {
                break;
            };
            let request_handler = request_handler.clone();
            tokio::spawn(async move {
                if let Err(err) =
                    handle_connecting(connecting, request_handler, max_stream_payload_bytes).await
                {
                    warn!("QUIC connection handler failed: {}", err);
                }
            });
        }
    });

    info!(
        "QUIC endpoint started on {} (max_concurrent_bidi_streams={}).",
        local_addr, config.max_concurrent_bidi_streams
    );

    Ok(QuicRuntime {
        endpoint,
        local_addr,
    })
}

fn load_quic_identity_from_env() -> Result<Option<(Vec<rustls::Certificate>, rustls::PrivateKey)>> {
    let cert_path = std::env::var("MGS_QUIC_CERT_PATH")
        .or_else(|_| std::env::var("QUIC_CERT_PATH"))
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty());
    let key_path = std::env::var("MGS_QUIC_KEY_PATH")
        .or_else(|_| std::env::var("QUIC_KEY_PATH"))
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty());

    match (cert_path, key_path) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(anyhow!(
            "both MGS_QUIC_CERT_PATH and MGS_QUIC_KEY_PATH must be set"
        )),
        (Some(cert_path), Some(key_path)) => {
            let cert_der = fs::read(&cert_path)
                .with_context(|| format!("failed reading QUIC cert path {}", cert_path))?;
            let key_der = fs::read(&key_path)
                .with_context(|| format!("failed reading QUIC key path {}", key_path))?;
            info!(
                "Loaded QUIC TLS identity from files cert='{}' key='{}'.",
                cert_path, key_path
            );
            Ok(Some((
                vec![rustls::Certificate(cert_der)],
                rustls::PrivateKey(key_der),
            )))
        }
    }
}

pub fn start_quic_runtime_from_env(default_bind_addr: SocketAddr) -> Result<Option<QuicRuntime>> {
    start_quic_runtime_from_env_with_handler(default_bind_addr, None)
}

pub fn start_quic_runtime_from_env_with_handler(
    default_bind_addr: SocketAddr,
    request_handler: Option<QuicRequestHandler>,
) -> Result<Option<QuicRuntime>> {
    if !quic_enabled() {
        return Ok(None);
    }
    let config = QuicEndpointConfig::from_env(default_bind_addr);
    let runtime = start_quic_runtime(&config, request_handler)?;
    Ok(Some(runtime))
}

#[derive(Debug, Deserialize)]
struct QuicControlEnvelope {
    op: Option<String>,
    peer_id: Option<String>,
    smoothed_rtt_ms: Option<u32>,
    traceparent: Option<String>,
    tracestate: Option<String>,
}

#[derive(Debug, Serialize)]
struct QuicControlAck {
    ok: bool,
    op: String,
    detail: String,
}

async fn handle_connecting(
    connecting: Connecting,
    request_handler: Option<QuicRequestHandler>,
    max_stream_payload_bytes: usize,
) -> Result<()> {
    let connection = connecting
        .await
        .context("failed to establish QUIC connection")?;
    let remote_addr = connection.remote_address();
    info!("QUIC client connected from {}", remote_addr);
    let connection_token = QUIC_CONNECTION_TOKEN.fetch_add(1, AtomicOrdering::Relaxed);
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<Bytes>();
    let peer_sender = QuicPeerSender {
        connection_token,
        outbound_tx,
    };
    let registered_peer_ids = Arc::new(Mutex::new(Vec::<String>::new()));

    tokio::spawn(run_connection_writer(
        connection.clone(),
        outbound_rx,
        connection_token,
        remote_addr,
    ));

    loop {
        match connection.accept_bi().await {
            Ok((send, recv)) => {
                let request_handler = request_handler.clone();
                let peer_sender = peer_sender.clone();
                let registered_peer_ids = Arc::clone(&registered_peer_ids);
                tokio::spawn(async move {
                    if let Err(err) = handle_bidi_stream(
                        send,
                        recv,
                        request_handler,
                        max_stream_payload_bytes,
                        peer_sender,
                        registered_peer_ids,
                    )
                    .await
                    {
                        debug!("QUIC stream handler ended with error: {}", err);
                    }
                });
            }
            Err(err) => {
                debug!("QUIC connection {} closed: {}", remote_addr, err);
                break;
            }
        }
    }

    cleanup_registered_quic_peers(connection_token, &registered_peer_ids);

    Ok(())
}

fn build_control_ack(op: impl Into<String>, detail: impl Into<String>) -> Vec<u8> {
    serde_json::to_vec(&QuicControlAck {
        ok: true,
        op: op.into(),
        detail: detail.into(),
    })
    .unwrap_or_else(|_| br#"{"ok":true,"op":"ack","detail":"ok"}"#.to_vec())
}

fn maybe_register_quic_peer(
    payload: &[u8],
    sender: &QuicPeerSender,
    registered_peer_ids: &Arc<Mutex<Vec<String>>>,
) {
    let Ok(envelope) = serde_json::from_slice::<QuicControlEnvelope>(payload) else {
        return;
    };
    let Some(peer_id) = envelope
        .peer_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return;
    };

    shared_quic_peer_senders().insert(peer_id.to_string(), sender.clone());
    if let Ok(mut peers) = registered_peer_ids.lock() {
        if !peers.iter().any(|existing| existing == peer_id) {
            peers.push(peer_id.to_string());
        }
    }

    shared_connection_manager().upsert(ConnectionInfo::new(
        peer_id.to_string(),
        TransportKind::Quic,
    ));
    shared_connection_manager().touch(peer_id);
    if let Some(rtt) = envelope.smoothed_rtt_ms {
        shared_connection_manager().set_rtt_ms(peer_id, rtt);
        metrics::record_connection_rtt_ms("quic", rtt as f64);
    }
}

async fn handle_bidi_stream(
    mut send: SendStream,
    mut recv: RecvStream,
    request_handler: Option<QuicRequestHandler>,
    max_stream_payload_bytes: usize,
    peer_sender: QuicPeerSender,
    registered_peer_ids: Arc<Mutex<Vec<String>>>,
) -> Result<()> {
    let payload = recv
        .read_to_end(max_stream_payload_bytes)
        .await
        .context("failed reading QUIC stream payload")?;

    let parsed_envelope = serde_json::from_slice::<QuicControlEnvelope>(&payload).ok();
    let remote_context = monitoring_tracing::extract_remote_context(
        parsed_envelope
            .as_ref()
            .and_then(|envelope| envelope.traceparent.as_deref()),
        parsed_envelope
            .as_ref()
            .and_then(|envelope| envelope.tracestate.as_deref()),
    );
    let stream_span = tracing::info_span!(
        "quic_bidi_stream",
        op = parsed_envelope
            .as_ref()
            .and_then(|envelope| envelope.op.as_deref())
            .unwrap_or("control"),
        peer_id = parsed_envelope
            .as_ref()
            .and_then(|envelope| envelope.peer_id.as_deref())
            .unwrap_or("unknown")
    );
    stream_span.set_parent(remote_context);

    async move {
        maybe_register_quic_peer(&payload, &peer_sender, &registered_peer_ids);

        let response = if let Some(handler) = request_handler {
            handler(payload.as_slice()).unwrap_or_else(|| build_control_ack("quic", "ok"))
        } else if let Some(envelope) = parsed_envelope {
            let op = envelope.op.unwrap_or_else(|| "control".to_string());
            build_control_ack(op, "accepted")
        } else {
            // Fallback compatibility path: echo payload for reachability tests.
            payload
        };

        send.write_all(&response)
            .await
            .context("failed writing QUIC stream payload")?;
        send.flush().await.context("failed flushing QUIC stream")?;
        send.finish()
            .await
            .context("failed finishing QUIC stream")?;
        Ok(())
    }
    .instrument(stream_span)
    .await
}

async fn run_connection_writer(
    connection: quinn::Connection,
    mut outbound_rx: mpsc::UnboundedReceiver<Bytes>,
    connection_token: u64,
    remote_addr: SocketAddr,
) {
    while let Some(payload) = outbound_rx.recv().await {
        if payload.is_empty() {
            continue;
        }

        let mut send_stream = match connection.open_uni().await {
            Ok(stream) => stream,
            Err(err) => {
                debug!(
                    "QUIC writer token={} could not open uni stream for {}: {}",
                    connection_token, remote_addr, err
                );
                break;
            }
        };

        if let Err(err) = send_stream.write_all(payload.as_ref()).await {
            debug!(
                "QUIC writer token={} failed writing payload to {}: {}",
                connection_token, remote_addr, err
            );
            break;
        }
        if let Err(err) = send_stream.finish().await {
            debug!(
                "QUIC writer token={} failed finishing payload to {}: {}",
                connection_token, remote_addr, err
            );
            break;
        }
    }
}

fn cleanup_registered_quic_peers(connection_token: u64, registered_peer_ids: &Arc<Mutex<Vec<String>>>) {
    let peers = registered_peer_ids
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();

    for peer_id in peers {
        let should_remove = shared_quic_peer_senders()
            .get(&peer_id)
            .map(|entry| entry.connection_token == connection_token)
            .unwrap_or(false);
        if should_remove {
            let _ = shared_quic_peer_senders().remove(&peer_id);
            let _ = shared_connection_manager().remove(&peer_id);
        }
    }
}
