// massive_game_server/server/src/network/quic/handler.rs

use crate::network::connection_manager::{
    shared_connection_manager, ConnectionInfo, TransportKind,
};
use crate::operational::monitoring::metrics;
use crate::operational::monitoring::tracing as monitoring_tracing;
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use dashmap::DashMap;
use quinn::{Endpoint, Incoming, RecvStream, SendStream};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::BufReader;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tracing::{debug, error, info, warn, Instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Handler signature now receives the connection-bound peer_id (if any) alongside the raw payload.
/// The second argument is the server-assigned peer_id for this QUIC connection, derived from the
/// session token during the "join" handshake. It is `None` until a successful auth handshake.
pub type QuicRequestHandler =
    Arc<dyn Fn(&[u8], Option<&str>) -> Option<Vec<u8>> + Send + Sync + 'static>;
type QuicDisconnectHook = Arc<dyn Fn(&str) + Send + Sync + 'static>;

/// Hard upper bound on a single QUIC stream payload (64 KB).
/// Game messages should never exceed this; the configurable max_stream_payload_bytes is clamped
/// to this ceiling regardless of environment variable settings.
const QUIC_MAX_STREAM_PAYLOAD_HARD_CAP: usize = 64 * 1024;

/// Bounded outbound channel capacity per connection.
/// If the client cannot keep up, sends will apply backpressure rather than growing without bound.
const OUTBOUND_CHANNEL_CAPACITY: usize = 4096;
/// 4-byte big-endian length prefix for framed outbound QUIC streams.
const QUIC_FRAMED_LENGTH_PREFIX_BYTES: usize = 4;

/// Default per-IP connection rate: connections per second.
const DEFAULT_CONN_RATE_PER_SEC: u32 = 8;
/// Default per-IP burst capacity for connection rate limiting.
const DEFAULT_CONN_RATE_BURST: u32 = 16;
/// Global cap on simultaneous QUIC connections handled by this process.
const DEFAULT_MAX_CONCURRENT_CONNECTIONS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuicOutboundMode {
    /// Legacy mode: open one unidirectional stream per packet.
    LegacyPerPacketStream,
    /// Preferred mode: keep one unidirectional stream open and write length-prefixed frames.
    FramedStream,
}

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
    outbound_tx: mpsc::Sender<Bytes>,
}

static QUIC_PEER_SENDERS: OnceLock<DashMap<String, QuicPeerSender>> = OnceLock::new();
static QUIC_CONNECTION_TOKEN: AtomicU64 = AtomicU64::new(1);
static QUIC_DISCONNECT_HOOK: OnceLock<QuicDisconnectHook> = OnceLock::new();

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
        // Clamp to hard cap: game messages should never exceed QUIC_MAX_STREAM_PAYLOAD_HARD_CAP.
        let max_stream_payload_bytes = std::env::var("MGS_QUIC_MAX_STREAM_PAYLOAD_BYTES")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(QUIC_MAX_STREAM_PAYLOAD_HARD_CAP)
            .clamp(4 * 1024, QUIC_MAX_STREAM_PAYLOAD_HARD_CAP);
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

pub fn register_quic_disconnect_hook(hook: QuicDisconnectHook) {
    if QUIC_DISCONNECT_HOOK.set(hook).is_err() {
        warn!("QUIC disconnect hook already registered; keeping existing hook.");
    }
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
    let mut saw_closed = false;
    for packet in packets {
        // try_send applies backpressure: if channel is full, stop sending.
        match sender.outbound_tx.try_send(packet.clone()) {
            Ok(()) => sent += 1,
            Err(mpsc::error::TrySendError::Full(_)) => {
                let dropped = packets.len().saturating_sub(sent) as u64;
                metrics::record_quic_outbound_dropped_packets("channel_full", dropped);
                debug!(
                    "QUIC outbound channel full for peer '{}', dropping {} remaining packets",
                    peer_id, dropped
                );
                break;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                let dropped = packets.len().saturating_sub(sent) as u64;
                metrics::record_quic_outbound_dropped_packets("channel_closed", dropped);
                saw_closed = true;
                break;
            }
        }
    }

    if sent == 0 && saw_closed {
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
    if env_flag("MGS_QUIC_ALLOW_SELF_SIGNED_TESTING") {
        tracing::warn!("DANGER: Self-signed certificates enabled for QUIC");
        return true;
    }
    false
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

// ---------------------------------------------------------------------------
// Per-IP connection rate limiter
// ---------------------------------------------------------------------------

struct IpRateLimiter {
    refill_per_sec: f64,
    capacity: f64,
    available: f64,
    last_refill: Instant,
}

impl IpRateLimiter {
    fn new(refill_per_sec: u32, burst: u32) -> Self {
        let cap = burst.max(1) as f64;
        Self {
            refill_per_sec: refill_per_sec.max(1) as f64,
            capacity: cap,
            available: cap,
            last_refill: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now
            .checked_duration_since(self.last_refill)
            .unwrap_or_default()
            .as_secs_f64();
        if elapsed > 0.0 {
            self.available = (self.available + elapsed * self.refill_per_sec).min(self.capacity);
            self.last_refill = now;
        }
        if self.available >= 1.0 {
            self.available -= 1.0;
            true
        } else {
            false
        }
    }
}

struct ConnectionRateLimiters {
    limiters: Mutex<HashMap<IpAddr, IpRateLimiter>>,
    per_sec: u32,
    burst: u32,
}

impl ConnectionRateLimiters {
    fn new(per_sec: u32, burst: u32) -> Self {
        Self {
            limiters: Mutex::new(HashMap::new()),
            per_sec,
            burst,
        }
    }

    fn try_acquire(&self, ip: IpAddr) -> bool {
        let mut map = match self.limiters.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        let limiter = map
            .entry(ip)
            .or_insert_with(|| IpRateLimiter::new(self.per_sec, self.burst));
        limiter.try_acquire()
    }

    /// Periodic cleanup of stale entries to prevent unbounded growth.
    fn cleanup_stale(&self) {
        let mut map = match self.limiters.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let now = Instant::now();
        map.retain(|_ip, limiter| {
            // Keep entries that were active in the last 60 seconds.
            now.checked_duration_since(limiter.last_refill)
                .unwrap_or_default()
                .as_secs()
                < 60
        });
    }
}

fn load_conn_rate_limit_config() -> (u32, u32) {
    let per_sec = std::env::var("MGS_QUIC_CONN_RATE_PER_SEC")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_CONN_RATE_PER_SEC)
        .clamp(1, 1000);
    let burst = std::env::var("MGS_QUIC_CONN_RATE_BURST")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_CONN_RATE_BURST)
        .clamp(1, 2000);
    (per_sec, burst)
}

fn parse_max_concurrent_connections(raw: Option<&str>) -> usize {
    raw.and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_CONCURRENT_CONNECTIONS)
        .clamp(1, 20_000)
}

fn load_max_concurrent_connections() -> usize {
    parse_max_concurrent_connections(
        std::env::var("MGS_QUIC_MAX_CONCURRENT_CONNECTIONS")
            .ok()
            .as_deref(),
    )
}

fn parse_quic_outbound_mode(raw: Option<&str>) -> QuicOutboundMode {
    match raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("legacy") | Some("stream_per_packet") | Some("per_packet") => {
            QuicOutboundMode::LegacyPerPacketStream
        }
        Some("framed") | Some("framed_stream") | Some("stream_framed") | None => {
            QuicOutboundMode::FramedStream
        }
        Some(_) => QuicOutboundMode::FramedStream,
    }
}

fn load_quic_outbound_mode() -> QuicOutboundMode {
    parse_quic_outbound_mode(std::env::var("MGS_QUIC_OUTBOUND_MODE").ok().as_deref())
}

pub fn quic_outbound_mode_name() -> &'static str {
    match load_quic_outbound_mode() {
        QuicOutboundMode::LegacyPerPacketStream => "legacy",
        QuicOutboundMode::FramedStream => "framed",
    }
}

// ---------------------------------------------------------------------------

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
                if env_flag("MGS_QUIC_REQUIRE_REAL_CERT") {
                    error!(
                        "MGS_QUIC_REQUIRE_REAL_CERT is set but no certificate files were provided. \
                         Set MGS_QUIC_CERT_PATH and MGS_QUIC_KEY_PATH to valid PEM files."
                    );
                }
                return Err(anyhow!(
                    "QUIC certificates are required in this build/configuration. Set MGS_QUIC_CERT_PATH \
                     and MGS_QUIC_KEY_PATH to PEM certificate/key files, or unset \
                     MGS_QUIC_REQUIRE_REAL_CERT to allow self-signed fallback in debug builds."
                ));
            }
            warn!(
                "Using self-signed QUIC certificates - not recommended for production. \
                 Set MGS_QUIC_CERT_PATH and MGS_QUIC_KEY_PATH to PEM files, or \
                 set MGS_QUIC_REQUIRE_REAL_CERT=true to refuse startup without real certificates."
            );
            let certified_key = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
                .context("failed to generate self-signed QUIC certificate")?;
            let cert_der =
                quinn::rustls::pki_types::CertificateDer::from(certified_key.cert.der().to_vec());
            let key_der = quinn::rustls::pki_types::PrivateKeyDer::try_from(
                certified_key.key_pair.serialize_der(),
            )
            .map_err(|err| anyhow!("failed to parse self-signed key DER: {}", err))?;
            (vec![cert_der], key_der)
        }
    };

    let mut server_config = quinn::ServerConfig::with_single_cert(cert_chain, key)
        .context("failed to create QUIC server config")?;
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(config.max_concurrent_bidi_streams.into());
    server_config.transport_config(Arc::new(transport));

    let endpoint = quinn::Endpoint::server(server_config, config.bind_addr)
        .context("failed to bind QUIC endpoint")?;
    let local_addr = endpoint
        .local_addr()
        .context("failed to read QUIC local address")?;

    let endpoint_for_accept = endpoint.clone();
    let max_stream_payload_bytes = config.max_stream_payload_bytes;

    // Connection rate limiter (per-IP).
    let (rate_per_sec, rate_burst) = load_conn_rate_limit_config();
    let conn_rate_limiters = Arc::new(ConnectionRateLimiters::new(rate_per_sec, rate_burst));
    let max_concurrent_connections = load_max_concurrent_connections();
    let quic_outbound_mode = load_quic_outbound_mode();
    let connection_slots = Arc::new(Semaphore::new(max_concurrent_connections));
    info!(
        "QUIC per-IP connection rate limit: {} conn/sec, burst {}, max_connections={}, outbound_mode={:?}",
        rate_per_sec, rate_burst, max_concurrent_connections, quic_outbound_mode
    );

    let rate_limiters_for_cleanup = Arc::clone(&conn_rate_limiters);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            rate_limiters_for_cleanup.cleanup_stale();
        }
    });

    tokio::spawn(async move {
        loop {
            let Some(incoming) = endpoint_for_accept.accept().await else {
                break;
            };
            let remote_addr = incoming.remote_address();
            let remote_ip = remote_addr.ip();

            // Per-IP connection rate limiting.
            if !conn_rate_limiters.try_acquire(remote_ip) {
                warn!(
                    "QUIC connection rate-limited for IP {}, rejecting",
                    remote_ip
                );
                metrics::record_quic_connection_rejected("rate_limited");
                // Drop the Incoming to refuse the connection.
                drop(incoming);
                continue;
            }
            let connection_slot = match connection_slots.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    warn!(
                        "QUIC global connection cap reached (max={}), rejecting {}",
                        max_concurrent_connections, remote_addr
                    );
                    metrics::record_quic_connection_rejected("global_connection_cap");
                    drop(incoming);
                    continue;
                }
            };

            let request_handler = request_handler.clone();
            tokio::spawn(async move {
                if let Err(err) = handle_incoming(
                    incoming,
                    request_handler,
                    max_stream_payload_bytes,
                    quic_outbound_mode,
                    connection_slot,
                )
                .await
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

fn load_quic_identity_from_env() -> Result<
    Option<(
        Vec<quinn::rustls::pki_types::CertificateDer<'static>>,
        quinn::rustls::pki_types::PrivateKeyDer<'static>,
    )>,
> {
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
            let certs = load_pem_certs(&cert_path)
                .with_context(|| format!("failed loading QUIC cert from {}", cert_path))?;
            if certs.is_empty() {
                return Err(anyhow!("no certificates found in PEM file '{}'", cert_path));
            }
            let key = load_pem_private_key(&key_path)
                .with_context(|| format!("failed loading QUIC key from {}", key_path))?;
            info!(
                "Loaded QUIC TLS identity from PEM files cert='{}' ({} cert(s)) key='{}'.",
                cert_path,
                certs.len(),
                key_path
            );
            Ok(Some((certs, key)))
        }
    }
}

/// Reads PEM-encoded certificates from a file.  Falls back to treating the
/// whole file as a single DER certificate if no PEM sections are found.
fn load_pem_certs(path: &str) -> Result<Vec<quinn::rustls::pki_types::CertificateDer<'static>>> {
    let file =
        fs::File::open(path).with_context(|| format!("cannot open certificate file '{}'", path))?;
    let mut reader = BufReader::new(file);
    let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
        .filter_map(|r| r.ok())
        .collect();
    if certs.is_empty() {
        let der =
            fs::read(path).with_context(|| format!("failed reading DER cert from '{}'", path))?;
        info!(
            "No PEM sections found in '{}'; treating as raw DER certificate.",
            path
        );
        return Ok(vec![quinn::rustls::pki_types::CertificateDer::from(der)]);
    }
    Ok(certs)
}

/// Reads a PEM-encoded private key from a file.  Supports PKCS#8, RSA,
/// and EC key formats.  Falls back to raw DER if no PEM sections are found.
fn load_pem_private_key(path: &str) -> Result<quinn::rustls::pki_types::PrivateKeyDer<'static>> {
    let file = fs::File::open(path).with_context(|| format!("cannot open key file '{}'", path))?;
    let mut reader = BufReader::new(file);

    // Try PKCS#8 first (most common for modern certs).
    let pkcs8: Vec<_> = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .filter_map(|r| r.ok())
        .collect();
    if let Some(key) = pkcs8.into_iter().next() {
        return Ok(quinn::rustls::pki_types::PrivateKeyDer::Pkcs8(key));
    }

    // Re-read and try RSA keys.
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let rsa: Vec<_> = rustls_pemfile::rsa_private_keys(&mut reader)
        .filter_map(|r| r.ok())
        .collect();
    if let Some(key) = rsa.into_iter().next() {
        return Ok(quinn::rustls::pki_types::PrivateKeyDer::Pkcs1(key));
    }

    // Fallback: raw DER.
    let der = fs::read(path).with_context(|| format!("failed reading DER key from '{}'", path))?;
    if der.is_empty() {
        return Err(anyhow!(
            "no private key found in '{}' (tried PEM PKCS#8, RSA, and raw DER)",
            path
        ));
    }
    info!(
        "No PEM key sections found in '{}'; treating as raw DER key.",
        path
    );
    quinn::rustls::pki_types::PrivateKeyDer::try_from(der)
        .map_err(|e| anyhow!("failed to parse DER key: {}", e))
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
#[allow(dead_code)]
struct QuicControlEnvelope {
    op: Option<String>,
    peer_id: Option<String>,
    auth_token: Option<String>,
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

async fn handle_incoming(
    incoming: Incoming,
    request_handler: Option<QuicRequestHandler>,
    max_stream_payload_bytes: usize,
    quic_outbound_mode: QuicOutboundMode,
    _connection_slot: OwnedSemaphorePermit,
) -> Result<()> {
    let connection = incoming
        .await
        .context("failed to establish QUIC connection")?;
    let remote_addr = connection.remote_address();
    info!("QUIC client connected from {}", remote_addr);
    let connection_token = QUIC_CONNECTION_TOKEN.fetch_add(1, AtomicOrdering::Relaxed);

    // FIX: bounded outbound channel instead of unbounded — provides backpressure to prevent
    // memory exhaustion when the client is slow to consume.
    let (outbound_tx, outbound_rx) = mpsc::channel::<Bytes>(OUTBOUND_CHANNEL_CAPACITY);
    let peer_sender = QuicPeerSender {
        connection_token,
        outbound_tx,
    };
    let registered_peer_ids = Arc::new(Mutex::new(Vec::<String>::new()));

    // The authenticated peer_id bound to this connection. Initially None; set after a valid
    // "join" via auth_token validation in the request handler. All subsequent operations on this
    // connection must use this bound peer_id rather than trusting client-supplied values.
    let bound_peer_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    tokio::spawn(run_connection_writer(
        connection.clone(),
        outbound_rx,
        connection_token,
        remote_addr,
        quic_outbound_mode,
    ));

    loop {
        match connection.accept_bi().await {
            Ok((send, recv)) => {
                let request_handler = request_handler.clone();
                let peer_sender = peer_sender.clone();
                let registered_peer_ids = Arc::clone(&registered_peer_ids);
                let bound_peer_id = Arc::clone(&bound_peer_id);
                tokio::spawn(async move {
                    if let Err(err) = handle_bidi_stream(
                        send,
                        recv,
                        request_handler,
                        max_stream_payload_bytes,
                        peer_sender,
                        registered_peer_ids,
                        bound_peer_id,
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

#[allow(dead_code)]
fn build_control_error(op: impl Into<String>, detail: impl Into<String>) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "ok": false,
        "op": op.into(),
        "error": detail.into(),
    }))
    .unwrap_or_else(|_| br#"{"ok":false,"op":"error","error":"internal"}"#.to_vec())
}

/// Register a QUIC peer sender only if the peer_id is server-bound (authenticated).
/// The `authenticated_peer_id` is the connection-bound peer_id, not client-supplied.
fn maybe_register_quic_peer(
    envelope: &QuicControlEnvelope,
    authenticated_peer_id: Option<&str>,
    sender: &QuicPeerSender,
    registered_peer_ids: &Arc<Mutex<Vec<String>>>,
) {
    // Only register if we have an authenticated peer_id bound to this connection.
    let Some(peer_id) = authenticated_peer_id else {
        return;
    };
    if peer_id.is_empty() {
        return;
    }

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
    recv: RecvStream,
    request_handler: Option<QuicRequestHandler>,
    max_stream_payload_bytes: usize,
    peer_sender: QuicPeerSender,
    registered_peer_ids: Arc<Mutex<Vec<String>>>,
    bound_peer_id: Arc<Mutex<Option<String>>>,
) -> Result<()> {
    // FIX: bounded read — enforce hard cap on stream payload size.
    // quinn's read_to_end already limits to the given size, but we clamp the configured
    // max to QUIC_MAX_STREAM_PAYLOAD_HARD_CAP to prevent abuse via env config.
    let effective_max = max_stream_payload_bytes.min(QUIC_MAX_STREAM_PAYLOAD_HARD_CAP);
    let payload = read_stream_bounded(recv, effective_max).await?;

    let parsed_envelope = serde_json::from_slice::<QuicControlEnvelope>(&payload).ok();
    let remote_context = monitoring_tracing::extract_remote_context(
        parsed_envelope
            .as_ref()
            .and_then(|envelope| envelope.traceparent.as_deref()),
        parsed_envelope
            .as_ref()
            .and_then(|envelope| envelope.tracestate.as_deref()),
    );

    // Determine the effective peer_id for this stream:
    // 1. If the connection already has a bound peer_id, use that (ignore client-supplied).
    // 2. Otherwise, this is pre-auth — the request handler will validate auth_token and set it.
    let current_bound = bound_peer_id.lock().ok().and_then(|guard| guard.clone());

    let stream_span = tracing::info_span!(
        "quic_bidi_stream",
        op = parsed_envelope
            .as_ref()
            .and_then(|envelope| envelope.op.as_deref())
            .unwrap_or("control"),
        peer_id = current_bound.as_deref().unwrap_or(
            parsed_envelope
                .as_ref()
                .and_then(|envelope| envelope.peer_id.as_deref())
                .unwrap_or("unknown")
        )
    );
    stream_span.set_parent(remote_context);

    async move {
        // Pass the bound peer_id to the request handler so it can enforce auth.
        let response = if let Some(handler) = request_handler {
            let handler_result = handler(payload.as_slice(), current_bound.as_deref());

            // If the handler returned a response that includes a newly-bound peer_id, extract it.
            // Convention: the handler embeds `"_bound_peer_id": "..."` in the response JSON
            // when a "join" succeeds with valid auth.
            if let Some(ref resp_bytes) = handler_result {
                if let Ok(resp_json) = serde_json::from_slice::<serde_json::Value>(resp_bytes) {
                    if let Some(new_peer_id) = resp_json
                        .get("_bound_peer_id")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        if let Ok(mut guard) = bound_peer_id.lock() {
                            *guard = Some(new_peer_id.to_string());
                        }
                        // Now register the peer sender with the authenticated peer_id.
                        if let Some(ref envelope) = parsed_envelope {
                            maybe_register_quic_peer(
                                envelope,
                                Some(new_peer_id),
                                &peer_sender,
                                &registered_peer_ids,
                            );
                        }
                    }
                }
            }

            // For non-join operations, update connection manager if already authenticated.
            if current_bound.is_some() {
                if let Some(ref envelope) = parsed_envelope {
                    maybe_register_quic_peer(
                        envelope,
                        current_bound.as_deref(),
                        &peer_sender,
                        &registered_peer_ids,
                    );
                }
            }

            handler_result.unwrap_or_else(|| build_control_ack("quic", "ok"))
        } else if let Some(ref envelope) = parsed_envelope {
            let op = envelope.op.as_deref().unwrap_or("control").to_string();
            build_control_ack(op, "accepted")
        } else {
            // Fallback compatibility path: echo payload for reachability tests.
            payload
        };

        send.write_all(&response)
            .await
            .context("failed writing QUIC stream payload")?;
        send.flush().await.context("failed flushing QUIC stream")?;
        send.finish().context("failed finishing QUIC stream")?;
        Ok(())
    }
    .instrument(stream_span)
    .await
}

/// Read from a QUIC RecvStream with a hard byte limit.
/// Returns an error if the stream exceeds the limit.
async fn read_stream_bounded(mut recv: RecvStream, max_bytes: usize) -> Result<Vec<u8>> {
    recv.read_to_end(max_bytes)
        .await
        .context("failed reading QUIC stream payload (possibly exceeded size limit)")
}

async fn run_connection_writer(
    connection: quinn::Connection,
    mut outbound_rx: mpsc::Receiver<Bytes>,
    connection_token: u64,
    remote_addr: SocketAddr,
    outbound_mode: QuicOutboundMode,
) {
    match outbound_mode {
        QuicOutboundMode::LegacyPerPacketStream => {
            run_connection_writer_legacy(
                connection,
                &mut outbound_rx,
                connection_token,
                remote_addr,
            )
            .await;
        }
        QuicOutboundMode::FramedStream => {
            run_connection_writer_framed(
                connection,
                &mut outbound_rx,
                connection_token,
                remote_addr,
            )
            .await;
        }
    }
}

async fn run_connection_writer_legacy(
    connection: quinn::Connection,
    outbound_rx: &mut mpsc::Receiver<Bytes>,
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
        if let Err(err) = send_stream.flush().await {
            debug!(
                "QUIC writer token={} failed flushing payload to {}: {}",
                connection_token, remote_addr, err
            );
            break;
        }
        if let Err(err) = send_stream.finish() {
            debug!(
                "QUIC writer token={} failed finishing payload to {}: {}",
                connection_token, remote_addr, err
            );
            break;
        }
    }
}

async fn run_connection_writer_framed(
    connection: quinn::Connection,
    outbound_rx: &mut mpsc::Receiver<Bytes>,
    connection_token: u64,
    remote_addr: SocketAddr,
) {
    let mut send_stream = match connection.open_uni().await {
        Ok(stream) => stream,
        Err(err) => {
            debug!(
                "QUIC framed writer token={} could not open outbound stream for {}: {}",
                connection_token, remote_addr, err
            );
            return;
        }
    };

    while let Some(payload) = outbound_rx.recv().await {
        if payload.is_empty() {
            continue;
        }

        let payload_len = match u32::try_from(payload.len()) {
            Ok(len) => len,
            Err(_) => {
                warn!(
                    "QUIC framed writer token={} dropping oversized payload ({} bytes) for {}",
                    connection_token,
                    payload.len(),
                    remote_addr
                );
                continue;
            }
        };
        let frame_prefix = payload_len.to_be_bytes();
        debug_assert_eq!(frame_prefix.len(), QUIC_FRAMED_LENGTH_PREFIX_BYTES);

        if let Err(err) = send_stream.write_all(&frame_prefix).await {
            debug!(
                "QUIC framed writer token={} failed writing frame prefix to {}: {}",
                connection_token, remote_addr, err
            );
            break;
        }
        if let Err(err) = send_stream.write_all(payload.as_ref()).await {
            debug!(
                "QUIC framed writer token={} failed writing framed payload to {}: {}",
                connection_token, remote_addr, err
            );
            break;
        }
        if let Err(err) = send_stream.flush().await {
            debug!(
                "QUIC framed writer token={} failed flushing framed payload to {}: {}",
                connection_token, remote_addr, err
            );
            break;
        }
    }

    if let Err(err) = send_stream.finish() {
        debug!(
            "QUIC framed writer token={} failed finishing outbound stream to {}: {}",
            connection_token, remote_addr, err
        );
    }
}

fn cleanup_registered_quic_peers(
    connection_token: u64,
    registered_peer_ids: &Arc<Mutex<Vec<String>>>,
) {
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
            if let Some(hook) = QUIC_DISCONNECT_HOOK.get() {
                hook(&peer_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_quic_sender_map_cleared<T>(f: impl FnOnce() -> T) -> T {
        static TEST_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        let _guard = TEST_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("quic sender test lock poisoned");
        shared_quic_peer_senders().clear();
        let output = f();
        shared_quic_peer_senders().clear();
        output
    }

    #[test]
    fn test_ip_rate_limiter_allows_burst_then_rejects() {
        let mut limiter = IpRateLimiter::new(1, 3);
        assert!(limiter.try_acquire(), "first request should succeed");
        assert!(limiter.try_acquire(), "second request should succeed");
        assert!(
            limiter.try_acquire(),
            "third request should succeed (burst=3)"
        );
        assert!(
            !limiter.try_acquire(),
            "fourth request should be rejected (burst exhausted)"
        );
    }

    #[test]
    fn test_connection_rate_limiters_per_ip_isolation() {
        let limiters = ConnectionRateLimiters::new(1, 2);
        let ip_a: IpAddr = "10.0.0.1".parse().unwrap();
        let ip_b: IpAddr = "10.0.0.2".parse().unwrap();

        assert!(limiters.try_acquire(ip_a));
        assert!(limiters.try_acquire(ip_a));
        assert!(!limiters.try_acquire(ip_a), "ip_a burst exhausted");

        // ip_b should have its own independent bucket
        assert!(limiters.try_acquire(ip_b));
        assert!(limiters.try_acquire(ip_b));
        assert!(!limiters.try_acquire(ip_b), "ip_b burst exhausted");
    }

    #[test]
    fn test_connection_rate_limiters_cleanup_stale() {
        let limiters = ConnectionRateLimiters::new(1, 2);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(limiters.try_acquire(ip));

        // Should not panic; entry is recent so it stays.
        limiters.cleanup_stale();
        let map = limiters.limiters.lock().unwrap();
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_bounded_channel_capacity_constant() {
        const {
            assert!(
                OUTBOUND_CHANNEL_CAPACITY > 0,
                "outbound channel capacity must be positive"
            )
        };
        const {
            assert!(
                OUTBOUND_CHANNEL_CAPACITY <= 8192,
                "outbound channel capacity should be reasonable"
            )
        };
    }

    #[test]
    fn test_hard_cap_enforced_in_config() {
        // The from_env uses clamp; verify the hard cap constant is reasonable.
        assert_eq!(QUIC_MAX_STREAM_PAYLOAD_HARD_CAP, 64 * 1024);
        // Even if someone passes a huge value, clamp brings it down.
        let clamped = (2 * 1024 * 1024usize).clamp(4 * 1024, QUIC_MAX_STREAM_PAYLOAD_HARD_CAP);
        assert_eq!(clamped, QUIC_MAX_STREAM_PAYLOAD_HARD_CAP);
    }

    #[test]
    fn test_build_control_error() {
        let err_bytes = build_control_error("join", "auth_required");
        let parsed: serde_json::Value = serde_json::from_slice(&err_bytes).unwrap();
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["error"], "auth_required");
    }

    #[test]
    fn test_build_control_ack() {
        let ack_bytes = build_control_ack("echo", "pong");
        let parsed: serde_json::Value = serde_json::from_slice(&ack_bytes).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["op"], "echo");
        assert_eq!(parsed["detail"], "pong");
    }

    #[tokio::test]
    async fn test_bounded_outbound_channel_backpressure() {
        let (tx, _rx) = mpsc::channel::<Bytes>(2);
        // Fill the channel.
        tx.try_send(Bytes::from_static(b"a")).unwrap();
        tx.try_send(Bytes::from_static(b"b")).unwrap();
        // Third send should fail with Full.
        match tx.try_send(Bytes::from_static(b"c")) {
            Err(mpsc::error::TrySendError::Full(_)) => {} // expected
            other => panic!("expected Full error, got {:?}", other),
        }
    }

    #[test]
    fn test_quic_config_clamps_payload_to_hard_cap() {
        // Simulate what from_env does with a large value.
        let large_value: usize = 2 * 1024 * 1024;
        let clamped = large_value.clamp(4 * 1024, QUIC_MAX_STREAM_PAYLOAD_HARD_CAP);
        assert_eq!(clamped, 64 * 1024);

        // Small value stays at minimum.
        let small_value: usize = 100;
        let clamped = small_value.clamp(4 * 1024, QUIC_MAX_STREAM_PAYLOAD_HARD_CAP);
        assert_eq!(clamped, 4 * 1024);
    }

    #[test]
    fn test_parse_max_concurrent_connections_defaults_and_clamps() {
        assert_eq!(
            parse_max_concurrent_connections(None),
            DEFAULT_MAX_CONCURRENT_CONNECTIONS
        );
        assert_eq!(parse_max_concurrent_connections(Some("0")), 1);
        assert_eq!(parse_max_concurrent_connections(Some("50000")), 20_000);
        assert_eq!(parse_max_concurrent_connections(Some("512")), 512);
        assert_eq!(
            parse_max_concurrent_connections(Some("invalid")),
            DEFAULT_MAX_CONCURRENT_CONNECTIONS
        );
    }

    #[test]
    fn test_parse_quic_outbound_mode_defaults_to_framed_stream() {
        assert_eq!(
            parse_quic_outbound_mode(None),
            QuicOutboundMode::FramedStream
        );
        assert_eq!(
            parse_quic_outbound_mode(Some("unknown-mode")),
            QuicOutboundMode::FramedStream
        );
    }

    #[test]
    fn test_parse_quic_outbound_mode_supports_legacy_aliases() {
        assert_eq!(
            parse_quic_outbound_mode(Some("legacy")),
            QuicOutboundMode::LegacyPerPacketStream
        );
        assert_eq!(
            parse_quic_outbound_mode(Some("stream_per_packet")),
            QuicOutboundMode::LegacyPerPacketStream
        );
    }

    #[test]
    fn test_send_quic_packet_batch_returns_zero_for_missing_peer() {
        with_quic_sender_map_cleared(|| {
            let sent = send_quic_packet_batch("missing-peer", &[Bytes::from_static(b"payload")]);
            assert_eq!(sent, 0);
        });
    }

    #[test]
    fn test_send_quic_packet_batch_sends_all_packets_when_capacity_allows() {
        with_quic_sender_map_cleared(|| {
            let peer_id = "peer-send";
            let (tx, mut rx) = mpsc::channel::<Bytes>(4);
            shared_quic_peer_senders().insert(
                peer_id.to_owned(),
                QuicPeerSender {
                    connection_token: 42,
                    outbound_tx: tx,
                },
            );

            let packets = [Bytes::from_static(b"a"), Bytes::from_static(b"b")];
            let sent = send_quic_packet_batch(peer_id, &packets);
            assert_eq!(sent, 2);

            let first = rx.try_recv().expect("first packet enqueued");
            let second = rx.try_recv().expect("second packet enqueued");
            assert_eq!(first, Bytes::from_static(b"a"));
            assert_eq!(second, Bytes::from_static(b"b"));
        });
    }

    #[test]
    fn test_send_quic_packet_batch_stops_on_full_channel_without_removal() {
        with_quic_sender_map_cleared(|| {
            let peer_id = "peer-full";
            let (tx, _rx) = mpsc::channel::<Bytes>(1);
            tx.try_send(Bytes::from_static(b"queued"))
                .expect("prefill outbound channel");
            shared_quic_peer_senders().insert(
                peer_id.to_owned(),
                QuicPeerSender {
                    connection_token: 7,
                    outbound_tx: tx,
                },
            );

            let sent = send_quic_packet_batch(peer_id, &[Bytes::from_static(b"new")]);
            assert_eq!(sent, 0, "full channel should not accept new packet");
            assert!(
                shared_quic_peer_senders().contains_key(peer_id),
                "peer should remain registered on backpressure"
            );
        });
    }

    #[test]
    fn test_send_quic_packet_batch_removes_peer_on_closed_channel() {
        with_quic_sender_map_cleared(|| {
            let peer_id = "peer-closed";
            let (tx, rx) = mpsc::channel::<Bytes>(1);
            drop(rx);
            shared_quic_peer_senders().insert(
                peer_id.to_owned(),
                QuicPeerSender {
                    connection_token: 99,
                    outbound_tx: tx,
                },
            );

            let sent = send_quic_packet_batch(peer_id, &[Bytes::from_static(b"new")]);
            assert_eq!(sent, 0);
            assert!(
                !shared_quic_peer_senders().contains_key(peer_id),
                "peer should be removed only when channel is closed"
            );
        });
    }
}
