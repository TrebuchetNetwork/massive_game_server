// massive_game_server/server/src/network/quic/mod.rs

pub mod handler;

pub use handler::{
    connected_quic_peer_count, connected_quic_peer_ids, quic_enabled, quic_outbound_mode_name,
    register_quic_disconnect_hook, send_quic_packet_batch, start_quic_runtime,
    start_quic_runtime_from_env, start_quic_runtime_from_env_with_handler, validate_quic_config,
    QuicEndpointConfig, QuicRequestHandler, QuicRuntime,
};
