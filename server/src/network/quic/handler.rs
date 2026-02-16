// massive_game_server/server/src/network/quic/handler.rs

use anyhow::{anyhow, Context, Result};
use quinn::{Connecting, Endpoint, RecvStream, SendStream};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct QuicEndpointConfig {
    pub bind_addr: SocketAddr,
    pub max_concurrent_bidi_streams: u32,
}

#[derive(Debug)]
pub struct QuicRuntime {
    endpoint: Endpoint,
    local_addr: SocketAddr,
}

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
        Self {
            bind_addr,
            max_concurrent_bidi_streams,
        }
    }
}

pub fn quic_enabled() -> bool {
    std::env::var("MGS_QUIC_PRIMARY")
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

pub fn validate_quic_config(config: &QuicEndpointConfig) -> Result<()> {
    if config.max_concurrent_bidi_streams == 0 {
        return Err(anyhow!("max_concurrent_bidi_streams must be > 0"));
    }
    Ok(())
}

pub fn start_quic_runtime(config: &QuicEndpointConfig) -> Result<QuicRuntime> {
    validate_quic_config(config)?;

    let certified_key = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
        .context("failed to generate self-signed QUIC certificate")?;
    let cert_der = certified_key.cert.der().to_vec();
    let key_der = certified_key.key_pair.serialize_der();

    let key = rustls::PrivateKey(key_der);
    let cert_chain = vec![rustls::Certificate(cert_der)];

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
    tokio::spawn(async move {
        loop {
            let Some(connecting) = endpoint_for_accept.accept().await else {
                break;
            };
            tokio::spawn(async move {
                if let Err(err) = handle_connecting(connecting).await {
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

pub fn start_quic_runtime_from_env(default_bind_addr: SocketAddr) -> Result<Option<QuicRuntime>> {
    if !quic_enabled() {
        return Ok(None);
    }
    let config = QuicEndpointConfig::from_env(default_bind_addr);
    let runtime = start_quic_runtime(&config)?;
    Ok(Some(runtime))
}

async fn handle_connecting(connecting: Connecting) -> Result<()> {
    let connection = connecting.await.context("failed to establish QUIC connection")?;
    let remote_addr = connection.remote_address();
    info!("QUIC client connected from {}", remote_addr);

    loop {
        match connection.accept_bi().await {
            Ok((send, recv)) => {
                tokio::spawn(async move {
                    if let Err(err) = handle_bidi_stream(send, recv).await {
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

    Ok(())
}

async fn handle_bidi_stream(mut send: SendStream, mut recv: RecvStream) -> Result<()> {
    let payload = recv
        .read_to_end(64 * 1024)
        .await
        .context("failed reading QUIC stream payload")?;

    // Minimal transport-level response so clients can verify QUIC reachability.
    send.write_all(&payload)
        .await
        .context("failed writing QUIC stream payload")?;
    send.flush().await.context("failed flushing QUIC stream")?;
    send.finish().await.context("failed finishing QUIC stream")?;
    Ok(())
}
