use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::PlayerAoIs;
use massive_game_server_core::flatbuffers_generated::game_protocol as fb;
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientStatesMap, DataChannelsMap, MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::server::instance::MassiveGameServer;

use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;

fn setup_test_server() -> Arc<MassiveGameServer> {
    let config = Arc::new(ServerConfig::default());
    let thread_pool_system =
        Arc::new(ThreadPoolSystem::new(config.clone()).expect("Failed to create thread pools"));

    let data_channels_map: DataChannelsMap = Arc::new(DashMap::new());
    let client_states_map: ClientStatesMap = Arc::new(ParkingLotRwLock::new(HashMap::new()));
    let chat_messages_queue: ChatMessagesQueue = Arc::new(TokioRwLock::new(BoundedChatQueue::new(MAX_CHAT_QUEUE_SIZE)));
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

fn add_or_update_player(server: &MassiveGameServer, peer_id: &str, team_id: u8, x: f32, y: f32) {
    server
        .player_manager
        .add_player(peer_id.to_owned(), peer_id.to_owned(), x, y);
    let player_id = server.player_manager.id_pool.get_or_create(peer_id);
    if let Some(mut player_state) = server.player_manager.get_player_state_mut(&player_id) {
        player_state.team_id = team_id;
        player_state.x = x;
        player_state.y = y;
        player_state.alive = true;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ctf_capture_and_match_lifecycle_transitions() {
    let server = setup_test_server();
    add_or_update_player(server.as_ref(), "runner", 1, 0.0, 0.0);
    add_or_update_player(server.as_ref(), "defender", 2, 0.0, 0.0);

    // Waiting -> Active transition and initial flag setup.
    server.run_game_logic_update(0.016).await;
    {
        let match_info = server.match_info.read();
        assert_eq!(match_info.match_state, fb::MatchStateType::Active);
        assert_eq!(match_info.flag_states.len(), 2);
    }

    let runner_id = server.player_manager.id_pool.get_or_create("runner");

    // Runner grabs team 2 flag.
    let enemy_flag_base = MassiveGameServer::get_flag_base_position(2);
    if let Some(mut runner_state) = server.player_manager.get_player_state_mut(&runner_id) {
        runner_state.x = enemy_flag_base.x;
        runner_state.y = enemy_flag_base.y;
    }
    server.run_game_logic_update(0.016).await;
    let runner_state = server
        .player_manager
        .get_player_state(&runner_id)
        .expect("runner should exist after flag grab");
    assert_eq!(runner_state.is_carrying_flag_team_id, 2);

    // Runner returns to team 1 base and captures.
    let own_flag_base = MassiveGameServer::get_flag_base_position(1);
    if let Some(mut runner_state) = server.player_manager.get_player_state_mut(&runner_id) {
        runner_state.x = own_flag_base.x;
        runner_state.y = own_flag_base.y;
    }
    server.run_game_logic_update(0.016).await;
    {
        let match_info = server.match_info.read();
        assert_eq!(match_info.team_scores.get(&1).copied(), Some(1));
    }
    let runner_state = server
        .player_manager
        .get_player_state(&runner_id)
        .expect("runner should exist after capture");
    assert_eq!(runner_state.is_carrying_flag_team_id, 0);
    assert_eq!(runner_state.score, 100);

    // Active -> Ended transition when timer expires.
    {
        let mut match_info = server.match_info.write();
        match_info.match_state = fb::MatchStateType::Active;
        match_info.time_remaining = 0.01;
    }
    server.run_game_logic_update(0.1).await;
    assert_eq!(
        server.match_info.read().match_state,
        fb::MatchStateType::Ended
    );

    // Ended -> Waiting transition after post-match cooldown; team score should persist.
    {
        let mut match_info = server.match_info.write();
        match_info.time_remaining = -9.95;
    }
    server.run_game_logic_update(0.1).await;
    let match_info = server.match_info.read();
    assert_eq!(match_info.match_state, fb::MatchStateType::Waiting);
    assert_eq!(match_info.team_scores.get(&1).copied(), Some(1));
}
