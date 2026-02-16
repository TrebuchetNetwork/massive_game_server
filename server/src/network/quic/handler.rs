// massive_game_server/server/src/network/quic/handler.rs

use anyhow::{anyhow, Result};
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct QuicEndpointConfig {
    pub bind_addr: SocketAddr,
    pub max_concurrent_bidi_streams: u32,
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
