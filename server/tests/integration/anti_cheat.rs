use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::constants::{
    MAX_INPUT_SEQUENCE_GAP, PLAYER_BASE_SPEED, POSITION_VALIDATION_VIOLATION_THRESHOLD,
};
use massive_game_server_core::core::types::{
    PlayerAoIs, PlayerID, PlayerInputData, ServerWeaponType,
};
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientStatesMap, DataChannelsMap, MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::server::instance::MassiveGameServer;

use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
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

fn make_input(sequence: u32) -> PlayerInputData {
    PlayerInputData {
        timestamp: 1000 + (sequence as u64 * 16),
        sequence,
        move_forward: false,
        move_backward: false,
        move_left: false,
        move_right: false,
        shooting: false,
        reload: false,
        rotation: 0.0,
        melee_attack: false,
        use_ability_slot: 0,
        change_weapon_slot: 0,
        ping_x: 0.0,
        ping_y: 0.0,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn suspicious_sequence_gap_is_rejected_before_processing() {
    let server = setup_test_server();
    let pid = add_player(&server, "seq-gap", 1, 0.0, 0.0);

    let accepted_first;
    let accepted_gap;
    let queued_len;
    let last_queued_sequence;
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        accepted_first = ps.queue_input(make_input(1));
        accepted_gap = ps.queue_input(make_input(MAX_INPUT_SEQUENCE_GAP + 2));
        queued_len = ps.input_queue.len();
        last_queued_sequence = ps.last_queued_input_sequence;
    } else {
        panic!("missing player state");
    }

    assert!(accepted_first, "initial input should be accepted");
    assert!(!accepted_gap, "suspicious sequence gap should be rejected");
    assert_eq!(queued_len, 1, "rejected input should not remain queued");
    assert_eq!(
        last_queued_sequence, 1,
        "rejected input must not advance accepted sequence"
    );

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert_eq!(
        ps.last_processed_input_sequence, 1,
        "only the accepted input should have been processed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fire_rate_abuse_does_not_create_projectile_before_cooldown() {
    let server = setup_test_server();
    let pid = add_player(&server, "cooldown-abuse", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.shooting = true;
    input.rotation = 0.0;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.weapon = ServerWeaponType::Rifle;
        ps.ammo = 30;
        ps.last_shot_time = Some(Instant::now());
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert_eq!(
        server.projectiles_to_add.len(),
        0,
        "cooldown abuse should not enqueue a projectile"
    );
    assert_eq!(ps.ammo, 30, "rejected shot should not consume ammo");
}

#[tokio::test(flavor = "multi_thread")]
async fn teleported_player_is_clamped_after_violation_threshold() {
    let server = setup_test_server();
    let pid = add_player(&server, "teleporter", 1, 0.0, 0.0);

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.last_valid_position = (0.0, 0.0);
        ps.x = 220.0;
        ps.y = 0.0;
        ps.velocity_x = PLAYER_BASE_SPEED * 4.0;
        ps.velocity_y = 0.0;
        ps.violation_count = POSITION_VALIDATION_VIOLATION_THRESHOLD;
    }

    server.run_physics_update(0.016).await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.x.abs() < f32::EPSILON && ps.y.abs() < f32::EPSILON,
        "teleported player should be snapped back to last valid position, got ({}, {})",
        ps.x,
        ps.y
    );
    assert!(
        ps.velocity_x.abs() < f32::EPSILON && ps.velocity_y.abs() < f32::EPSILON,
        "teleported player should have velocity zeroed after clamp"
    );
    assert!(
        ps.violation_count > POSITION_VALIDATION_VIOLATION_THRESHOLD,
        "violation count should increase when impossible movement is detected"
    );
}
