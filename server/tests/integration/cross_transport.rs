use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::PlayerAoIs;
use massive_game_server_core::network::quic::control::build_quic_control_handler;
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientStatesMap, DataChannelsMap, MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::operational::auth::AuthService;
use massive_game_server_core::operational::config::env_registry::AuthEnv;
use massive_game_server_core::server::instance::MassiveGameServer;
use parking_lot::RwLock as ParkingLotRwLock;
use serde_json::Value;
use tokio::sync::RwLock as TokioRwLock;

fn setup_test_server() -> Arc<MassiveGameServer> {
    let config = Arc::new(ServerConfig::default());
    let thread_pool_system =
        Arc::new(ThreadPoolSystem::new(config.clone()).expect("Failed to create thread pools"));
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

fn test_auth_service(store_suffix: &str) -> AuthService {
    let data_root = std::env::temp_dir().join(format!("mgs_cross_transport_{store_suffix}"));
    let _ = std::fs::create_dir_all(&data_root);
    AuthService::new_from_env_config(&AuthEnv {
        store_path: data_root.join("auth_store.json").display().to_string(),
        otp_ttl_seconds: 300,
        session_ttl_seconds: 3600,
        resend_interval_seconds: 30,
        max_verify_attempts: 5,
        token_validation_rate_limit_per_sec: 24,
        token_validation_rate_limit_burst: 48,
        sms_command: None,
        sms_dev_mode: true,
        use_auth_cookies: false,
        deletion_grace_period_hours: 24,
        redis_url: None,
        redis_store_key: "cross_transport".to_owned(),
        gdpr_hash_salt: Some("cross-transport-test-salt".to_owned()),
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn webrtc_and_quic_registrations_share_match_state() {
    let server = setup_test_server();
    let auth_service = test_auth_service("shared_state");

    server.player_manager.add_player(
        "webrtc-peer".to_owned(),
        "webrtc-player".to_owned(),
        0.0,
        0.0,
    );
    let webrtc_id = server.player_manager.id_pool.get_or_create("webrtc-peer");
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&webrtc_id) {
        ps.team_id = 1;
        ps.alive = true;
        ps.x = 0.0;
        ps.y = 0.0;
    }

    let quic_join = server.register_quic_player("quic-peer", Some("quic-player"), Some(2));
    assert!(
        quic_join.is_some(),
        "expected QUIC player registration to succeed"
    );
    let quic_id = server.player_manager.id_pool.get_or_create("quic-peer");
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&quic_id) {
        ps.team_id = 2;
        ps.alive = true;
        ps.x = 36.0;
        ps.y = 0.0;
    }

    assert!(
        server.player_manager.get_player_state(&webrtc_id).is_some(),
        "WebRTC registration should exist in authoritative state"
    );
    assert!(
        server.player_manager.get_player_state(&quic_id).is_some(),
        "QUIC registration should exist in authoritative state"
    );
    assert_eq!(server.player_manager.player_count(), 2);

    let handler = build_quic_control_handler(server.clone(), auth_service);
    let health_payload = handler(br#"{"op":"healthz"}"#, Some("quic-peer"))
        .expect("QUIC control handler should return a response");
    let health: Value = serde_json::from_slice(&health_payload).expect("health json");
    assert_eq!(health["ok"], Value::Bool(true));
    assert_eq!(health["players"].as_u64(), Some(2));
}
