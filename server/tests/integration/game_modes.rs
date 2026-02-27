// Tests for game mode logic: FFA, TDM, CTF match state transitions,
// flag mechanics, scoring, and dynamic mode transitions.

use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::constants::{POINTS_FLAG_CAPTURE, POINTS_FLAG_RETURN};
use massive_game_server_core::core::types::{PlayerAoIs, PlayerID, Vec2};
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
    }
    player_id
}

// ── Match state transitions ──────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn waiting_to_active_on_min_players() {
    let server = setup_test_server();
    {
        let mi = server.match_info.read();
        assert_eq!(mi.match_state, fb::MatchStateType::Waiting);
    }

    // Add one player (MIN_PLAYERS_TO_START=1) should trigger Active.
    add_player(&server, "player1", 1, 0.0, 0.0);
    server.run_game_logic_update(0.016).await;

    let mi = server.match_info.read();
    assert_eq!(mi.match_state, fb::MatchStateType::Active);
    assert!(mi.time_remaining > 0.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn active_to_ended_when_time_expires() {
    let server = setup_test_server();
    add_player(&server, "player1", 1, 0.0, 0.0);
    server.run_game_logic_update(0.016).await;

    // Set timer to almost expired.
    {
        let mut mi = server.match_info.write();
        assert_eq!(mi.match_state, fb::MatchStateType::Active);
        mi.time_remaining = 0.01;
    }

    server.run_game_logic_update(0.1).await;
    assert_eq!(
        server.match_info.read().match_state,
        fb::MatchStateType::Ended
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ended_to_waiting_after_cooldown() {
    let server = setup_test_server();
    add_player(&server, "player1", 1, 0.0, 0.0);
    server.run_game_logic_update(0.016).await;

    // Force into Ended state.
    {
        let mut mi = server.match_info.write();
        mi.match_state = fb::MatchStateType::Ended;
        mi.time_remaining = -9.95;
    }
    server.run_game_logic_update(0.1).await;

    assert_eq!(
        server.match_info.read().match_state,
        fb::MatchStateType::Waiting
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn player_stats_reset_on_match_start() {
    let server = setup_test_server();
    let pid = add_player(&server, "player1", 1, 0.0, 0.0);

    // Give player some stats.
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.kills = 5;
        ps.deaths = 3;
        ps.score = 50;
    }

    // Trigger match start.
    server.run_game_logic_update(0.016).await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert_eq!(ps.kills, 0);
    assert_eq!(ps.deaths, 0);
    assert_eq!(ps.score, 0);
}

// ── CTF flag mechanics ───────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn ctf_flag_grab_and_capture() {
    let server = setup_test_server();

    // Force CTF mode.
    {
        let mut mi = server.match_info.write();
        mi.game_mode = fb::GameModeType::CaptureTheFlag;
    }

    add_player(&server, "runner", 1, 0.0, 0.0);
    add_player(&server, "defender", 2, 0.0, 0.0);
    server.run_game_logic_update(0.016).await;

    // Verify flags initialized.
    {
        let mi = server.match_info.read();
        assert_eq!(mi.flag_states.len(), 2);
        assert_eq!(
            mi.flag_states.get(&1).unwrap().status,
            fb::FlagStatus::AtBase
        );
        assert_eq!(
            mi.flag_states.get(&2).unwrap().status,
            fb::FlagStatus::AtBase
        );
    }

    let runner_id = server.player_manager.id_pool.get_or_create("runner");

    // Move runner to enemy flag base.
    let enemy_base = MassiveGameServer::get_flag_base_position(2);
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&runner_id) {
        ps.x = enemy_base.x;
        ps.y = enemy_base.y;
    }
    server.run_game_logic_update(0.016).await;

    // Runner should be carrying team 2's flag.
    let ps = server.player_manager.get_player_state(&runner_id).unwrap();
    assert_eq!(ps.is_carrying_flag_team_id, 2);
    {
        let mi = server.match_info.read();
        assert_eq!(
            mi.flag_states.get(&2).unwrap().status,
            fb::FlagStatus::Carried
        );
    }

    // Move runner to own base to capture.
    let own_base = MassiveGameServer::get_flag_base_position(1);
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&runner_id) {
        ps.x = own_base.x;
        ps.y = own_base.y;
    }
    server.run_game_logic_update(0.016).await;

    // Flag should be captured.
    let ps = server.player_manager.get_player_state(&runner_id).unwrap();
    assert_eq!(ps.is_carrying_flag_team_id, 0);
    assert_eq!(ps.score, POINTS_FLAG_CAPTURE);
    assert_eq!(ps.flag_captures, 1);

    let mi = server.match_info.read();
    assert_eq!(mi.team_scores.get(&1).copied(), Some(1));
    assert_eq!(
        mi.flag_states.get(&2).unwrap().status,
        fb::FlagStatus::AtBase
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ctf_flag_return_own_dropped_flag() {
    let server = setup_test_server();
    {
        let mut mi = server.match_info.write();
        mi.game_mode = fb::GameModeType::CaptureTheFlag;
    }

    let _pid1 = add_player(&server, "team1_player", 1, 0.0, 0.0);
    let _pid2 = add_player(&server, "team2_player", 2, 0.0, 0.0);
    server.run_game_logic_update(0.016).await;

    // Simulate team 1 flag being dropped somewhere.
    let drop_pos = Vec2::new(200.0, 100.0);
    {
        let mut mi = server.match_info.write();
        if let Some(flag) = mi.flag_states.get_mut(&1) {
            flag.status = fb::FlagStatus::Dropped;
            flag.position = drop_pos;
            flag.respawn_timer = 0.0; // Immediately returnable
        }
    }

    // Move team1_player to the dropped flag location.
    let returner_id = server.player_manager.id_pool.get_or_create("team1_player");
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&returner_id) {
        ps.x = drop_pos.x;
        ps.y = drop_pos.y;
    }
    server.run_game_logic_update(0.016).await;

    // Flag should be returned to base.
    {
        let mi = server.match_info.read();
        let flag = mi.flag_states.get(&1).unwrap();
        assert_eq!(flag.status, fb::FlagStatus::AtBase);
    }

    // Player should get flag return score and stat.
    let ps = server
        .player_manager
        .get_player_state(&returner_id)
        .unwrap();
    assert_eq!(ps.score, POINTS_FLAG_RETURN);
    assert_eq!(ps.flag_returns, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn ctf_dropped_flag_auto_returns_after_timer() {
    let server = setup_test_server();
    {
        let mut mi = server.match_info.write();
        mi.game_mode = fb::GameModeType::CaptureTheFlag;
    }

    add_player(&server, "p1", 1, -500.0, -500.0);
    add_player(&server, "p2", 2, 500.0, 500.0);
    server.run_game_logic_update(0.016).await;

    // Drop team 2 flag with a very short timer.
    {
        let mut mi = server.match_info.write();
        if let Some(flag) = mi.flag_states.get_mut(&2) {
            flag.status = fb::FlagStatus::Dropped;
            flag.position = Vec2::new(100.0, 100.0);
            flag.respawn_timer = 0.05;
        }
    }

    // Tick enough to expire the timer.
    server.run_game_logic_update(0.1).await;

    let mi = server.match_info.read();
    let flag = mi.flag_states.get(&2).unwrap();
    assert_eq!(flag.status, fb::FlagStatus::AtBase);
}

#[tokio::test(flavor = "multi_thread")]
async fn ctf_three_captures_wins_match() {
    let server = setup_test_server();
    {
        let mut mi = server.match_info.write();
        mi.game_mode = fb::GameModeType::CaptureTheFlag;
    }

    let runner_id = add_player(&server, "runner", 1, 0.0, 0.0);
    add_player(&server, "defender", 2, -500.0, -500.0);
    server.run_game_logic_update(0.016).await;

    let enemy_base = MassiveGameServer::get_flag_base_position(2);
    let own_base = MassiveGameServer::get_flag_base_position(1);

    for _ in 0..3 {
        // Grab enemy flag.
        if let Some(mut ps) = server.player_manager.get_player_state_mut(&runner_id) {
            ps.x = enemy_base.x;
            ps.y = enemy_base.y;
        }
        server.run_game_logic_update(0.016).await;

        // Capture.
        if let Some(mut ps) = server.player_manager.get_player_state_mut(&runner_id) {
            ps.x = own_base.x;
            ps.y = own_base.y;
        }
        server.run_game_logic_update(0.016).await;
    }

    let mi = server.match_info.read();
    assert_eq!(mi.match_state, fb::MatchStateType::Ended);
    assert_eq!(mi.team_scores.get(&1).copied(), Some(3));
}

// ── Flag base positions ──────────────────────────────────────────────

#[test]
fn flag_base_positions_are_symmetric() {
    let team1 = MassiveGameServer::get_flag_base_position(1);
    let team2 = MassiveGameServer::get_flag_base_position(2);

    // Team 1 on the left, team 2 on the right.
    assert!(team1.x < 0.0);
    assert!(team2.x > 0.0);
    // Y should be the same.
    assert!((team1.y - team2.y).abs() < f32::EPSILON);
    // X should be symmetric around zero.
    assert!((team1.x.abs() - team2.x.abs()).abs() < f32::EPSILON);
}

#[test]
fn flag_base_position_for_invalid_team() {
    let pos = MassiveGameServer::get_flag_base_position(0);
    assert!((pos.x).abs() < f32::EPSILON);
    assert!((pos.y).abs() < f32::EPSILON);
}
