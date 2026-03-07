use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::constants::{
    AOI_MAX_VISIBLE_PLAYERS, AOI_UPDATE_INTERVAL_SECS, MOBILE_AOI_MAX_VISIBLE_PLAYERS,
};
use massive_game_server_core::core::types::{PlayerAoIs, PlayerID};
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientState, ClientStatesMap, DataChannelsMap,
    MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::server::instance::MassiveGameServer;

use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
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

fn add_player(server: &MassiveGameServer, peer_id: &str, team_id: u8, x: f32, y: f32) -> PlayerID {
    server
        .player_manager
        .add_player(peer_id.to_owned(), peer_id.to_owned(), x, y);
    let player_id = server.player_manager.id_pool.get_or_create(peer_id);
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&player_id) {
        ps.team_id = team_id;
        ps.x = x;
        ps.y = y;
        ps.alive = true;
        ps.last_valid_position = (x, y);
    }
    server
        .spatial_index
        .update_player_position(player_id.clone(), x, y);
    player_id
}

fn mark_aoi_stale(server: &MassiveGameServer, player_id: &PlayerID) {
    let mut aoi_entry = server
        .player_aois
        .entry(player_id.as_ref().to_string())
        .or_default();
    aoi_entry.value_mut().last_update =
        Instant::now() - Duration::from_secs_f32(AOI_UPDATE_INTERVAL_SECS + 0.05);
}

fn visible_player_ids(server: &MassiveGameServer, player_id: &PlayerID) -> HashSet<String> {
    server
        .player_aois
        .get(player_id.as_ref())
        .expect("player AoI should exist")
        .visible_players
        .iter()
        .map(|id| id.as_ref().to_string())
        .collect()
}

fn mark_connected_client(server: &MassiveGameServer, player_id: &PlayerID, is_mobile: bool) {
    let mut client_states = server.client_states_map.write();
    let client_state = ClientState {
        is_mobile,
        ..ClientState::default()
    };
    client_states.insert(player_id.as_ref().to_string(), client_state);
}

#[tokio::test(flavor = "multi_thread")]
async fn mobile_aoi_prefers_nearest_twenty_four_players() {
    let server = setup_test_server();
    let observer_id = add_player(&server, "mobile_observer", 1, 0.0, 0.0);
    mark_connected_client(&server, &observer_id, true);

    let mut expected_visible = HashSet::new();
    for idx in 0..30 {
        let peer_id = format!("cluster_{idx:02}");
        let distance = 10.0 + idx as f32 * 5.0;
        add_player(&server, &peer_id, 2, distance, 0.0);
        if idx < MOBILE_AOI_MAX_VISIBLE_PLAYERS {
            expected_visible.insert(peer_id);
        }
    }

    mark_aoi_stale(&server, &observer_id);
    server.synchronize_state(true).await;

    let visible = visible_player_ids(&server, &observer_id);
    assert_eq!(visible.len(), MOBILE_AOI_MAX_VISIBLE_PLAYERS);
    assert_eq!(
        visible, expected_visible,
        "mobile AoI should retain the nearest players when the cap is exceeded"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn desktop_aoi_caps_dense_clusters() {
    let server = setup_test_server();
    let observer_id = add_player(&server, "desktop_observer", 1, 0.0, 0.0);
    mark_connected_client(&server, &observer_id, false);

    for idx in 0..110 {
        let peer_id = format!("desktop_cluster_{idx:03}");
        let distance = 8.0 + idx as f32 * 3.0;
        add_player(&server, &peer_id, 2, distance, 0.0);
    }

    mark_aoi_stale(&server, &observer_id);
    server.synchronize_state(true).await;

    let visible = visible_player_ids(&server, &observer_id);
    assert_eq!(visible.len(), AOI_MAX_VISIBLE_PLAYERS);
}

#[tokio::test(flavor = "multi_thread")]
async fn aoi_refresh_replaces_far_and_near_players_after_teleport() {
    let server = setup_test_server();
    let observer_id = add_player(&server, "teleport_observer", 1, 0.0, 0.0);
    mark_connected_client(&server, &observer_id, false);
    let near_id = add_player(&server, "near_player", 2, 20.0, 0.0);
    let far_id = add_player(&server, "far_player", 2, 900.0, 0.0);

    mark_aoi_stale(&server, &observer_id);
    server.synchronize_state(true).await;

    let initial_visible = visible_player_ids(&server, &observer_id);
    assert!(initial_visible.contains("near_player"));
    assert!(!initial_visible.contains("far_player"));

    if let Some(mut near_state) = server.player_manager.get_player_state_mut(&near_id) {
        near_state.x = 900.0;
        near_state.y = 0.0;
        near_state.last_valid_position = (900.0, 0.0);
    }
    server
        .spatial_index
        .update_player_position(near_id.clone(), 900.0, 0.0);

    if let Some(mut far_state) = server.player_manager.get_player_state_mut(&far_id) {
        far_state.x = 15.0;
        far_state.y = 0.0;
        far_state.last_valid_position = (15.0, 0.0);
    }
    server
        .spatial_index
        .update_player_position(far_id.clone(), 15.0, 0.0);

    mark_aoi_stale(&server, &observer_id);
    server.synchronize_state(true).await;

    let refreshed_visible = visible_player_ids(&server, &observer_id);
    assert!(
        refreshed_visible.contains("far_player"),
        "AoI should include the player moved into range"
    );
    assert!(
        !refreshed_visible.contains("near_player"),
        "AoI should drop the player moved out of range"
    );
}
