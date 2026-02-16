// massive_game_server/server/src/network/quic/mod.rs

pub mod handler;

pub use handler::{
    quic_enabled, start_quic_runtime, start_quic_runtime_from_env, validate_quic_config,
    QuicEndpointConfig, QuicRuntime,
};
