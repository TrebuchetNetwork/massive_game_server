use dashmap::DashMap;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::PlayerAoIs;
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientStatesMap, DataChannelsMap, MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::routes::health::{build_healthz_route, build_readyz_route};
use massive_game_server_core::server::instance::MassiveGameServer;
use parking_lot::RwLock as ParkingLotRwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock as TokioRwLock;

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

fn current_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readyz_returns_503_before_first_tick_and_200_after_progress() {
    let server = setup_test_server();
    let route = build_readyz_route(server.clone());

    let not_ready = warp::test::request().path("/readyz").reply(&route).await;
    assert_eq!(
        not_ready.status(),
        warp::http::StatusCode::SERVICE_UNAVAILABLE
    );
    let not_ready_json: Value =
        serde_json::from_slice(not_ready.body()).expect("readyz not ready json");
    assert_eq!(not_ready_json["ok"], Value::Bool(false));
    assert_eq!(
        not_ready_json["error"],
        Value::String("not_ready".to_owned())
    );

    server
        .last_tick_epoch_ms
        .store(current_epoch_ms(), Ordering::Relaxed);
    server.frame_counter.store(1, Ordering::Relaxed);

    let ready = warp::test::request().path("/readyz").reply(&route).await;
    assert_eq!(ready.status(), warp::http::StatusCode::OK);
    let ready_json: Value = serde_json::from_slice(ready.body()).expect("readyz ready json");
    assert_eq!(ready_json["ok"], Value::Bool(true));
    assert_eq!(ready_json["frame"], Value::from(1u64));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn healthz_returns_503_when_last_tick_is_stale() {
    let server = setup_test_server();
    let route = build_healthz_route(server.clone());

    server
        .last_tick_epoch_ms
        .store(current_epoch_ms().saturating_sub(2_500), Ordering::Relaxed);

    let response = warp::test::request().path("/healthz").reply(&route).await;
    assert_eq!(
        response.status(),
        warp::http::StatusCode::SERVICE_UNAVAILABLE
    );
    let json: Value = serde_json::from_slice(response.body()).expect("healthz stale json");
    assert_eq!(json["ok"], Value::Bool(false));
    assert_eq!(json["error"], Value::String("game_loop_stalled".to_owned()));
}
