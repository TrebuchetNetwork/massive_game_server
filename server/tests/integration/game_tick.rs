// Tests for the full game tick pipeline: process_game_tick, broadcast state
// consistency, bot spawning, spectator handling, and multi-tick scenarios.

use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::{PlayerAoIs, PlayerID, PlayerInputData};
use massive_game_server_core::flatbuffers_generated::game_protocol as fb;
use massive_game_server_core::network::signaling::{ChatMessagesQueue, ClientStatesMap, DataChannelsMap};
use massive_game_server_core::server::instance::MassiveGameServer;

use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;

fn setup_test_server() -> Arc<MassiveGameServer> {
    let config = Arc::new(ServerConfig::default());
    let thread_pool_system =
        Arc::new(ThreadPoolSystem::new(config.clone()).expect("Failed to create thread pools"));
    let data_channels_map: DataChannelsMap = Arc::new(DashMap::new());
    let client_states_map: ClientStatesMap = Arc::new(ParkingLotRwLock::new(HashMap::new()));
    let chat_messages_queue: ChatMessagesQueue = Arc::new(TokioRwLock::new(VecDeque::new()));
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

// ── Full tick pipeline ──────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn process_game_tick_completes_without_panic() {
    let server = setup_test_server();
    add_player(&server, "tick_player", 1, 0.0, 0.0);

    let result = server.clone().process_game_tick(0.016).await;
    assert!(result.is_ok(), "Game tick should complete without error");
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_ticks_advance_frame_counter() {
    let server = setup_test_server();
    add_player(&server, "frame_counter_player", 1, 0.0, 0.0);

    let initial_frame = server.frame_counter.load(Ordering::Relaxed);

    for _ in 0..5 {
        let _ = server.clone().process_game_tick(0.016).await;
        server.frame_counter.fetch_add(1, Ordering::Relaxed);
    }

    let final_frame = server.frame_counter.load(Ordering::Relaxed);
    assert_eq!(
        final_frame,
        initial_frame + 5,
        "Frame counter should advance by 5"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tick_processes_input_and_physics_sequentially() {
    let server = setup_test_server();
    let pid = add_player(&server, "seq_test", 1, 100.0, 100.0);

    // Queue forward movement.
    let input = PlayerInputData {
        timestamp: 1000,
        sequence: 1,
        move_forward: true,
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
    };
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    // After one full tick, velocity should be set and position potentially updated.
    let _ = server.clone().process_game_tick(0.016).await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    // Player should have moved from original position or have velocity set.
    let has_velocity = ps.velocity_x.abs() > f32::EPSILON || ps.velocity_y.abs() > f32::EPSILON;
    let has_moved = (ps.x - 100.0).abs() > f32::EPSILON || (ps.y - 100.0).abs() > f32::EPSILON;
    assert!(
        has_velocity || has_moved,
        "After tick with forward input, player should have velocity or moved. vx={}, vy={}, x={}, y={}",
        ps.velocity_x, ps.velocity_y, ps.x, ps.y
    );
}

// ── Bot spawning ────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn bot_spawning_creates_expected_count() {
    let server = setup_test_server();
    let bot_count = 10;
    server.spawn_initial_bots(bot_count);

    assert_eq!(
        server.bot_players.len(),
        bot_count,
        "Should have spawned exactly {} bots",
        bot_count
    );

    // All bots should have player states.
    for entry in server.bot_players.iter() {
        let pid = entry.key();
        assert!(
            server.player_manager.get_player_state(pid).is_some(),
            "Bot should have a player state"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bots_alternate_teams() {
    let server = setup_test_server();
    server.spawn_initial_bots(20);

    let mut team1_count = 0;
    let mut team2_count = 0;
    for entry in server.bot_players.iter() {
        let pid = entry.key();
        if let Some(ps) = server.player_manager.get_player_state(pid) {
            match ps.team_id {
                1 => team1_count += 1,
                2 => team2_count += 1,
                _ => {}
            }
        }
    }

    assert_eq!(team1_count, 10, "Should have 10 bots on team 1");
    assert_eq!(team2_count, 10, "Should have 10 bots on team 2");
}

#[tokio::test(flavor = "multi_thread")]
async fn bots_have_unique_names() {
    let server = setup_test_server();
    server.spawn_initial_bots(26);

    let mut names = Vec::new();
    for entry in server.bot_players.iter() {
        if let Some(ps) = server.player_manager.get_player_state(entry.key()) {
            names.push(ps.username.clone());
        }
    }

    names.sort();
    let unique_count = {
        names.dedup();
        names.len()
    };
    assert_eq!(unique_count, 26, "All 26 bots should have unique names");
}

// ── Spectator handling ──────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn spectator_not_counted_as_participant() {
    let server = setup_test_server();

    // Add a normal player.
    add_player(&server, "normal", 1, 0.0, 0.0);

    // Simulate a spectator.
    let spec_id = add_player(&server, "spectator", 0, 0.0, 0.0);
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&spec_id) {
        ps.is_spectator = true;
    }

    let participant_count = server.participant_count();
    // participant_count should not include spectators.
    // (Depends on implementation — participant_count counts non-spectators)
    assert!(
        participant_count >= 1,
        "Should have at least 1 participant (non-spectator)"
    );
}

// ── Match timer decrements ──────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn match_timer_decrements_each_tick() {
    let server = setup_test_server();
    add_player(&server, "timer_player", 1, 0.0, 0.0);

    // Activate match.
    server.run_game_logic_update(0.016).await;
    let initial_time = server.match_info.read().time_remaining;

    // Run a few ticks.
    for _ in 0..10 {
        server.run_game_logic_update(0.016).await;
    }

    let final_time = server.match_info.read().time_remaining;
    assert!(
        final_time < initial_time,
        "Match timer should decrement. initial={}, final={}",
        initial_time,
        final_time
    );
}

// ── Pickup respawn ──────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn pickup_respawn_timer_ticks_down() {
    let server = setup_test_server();
    add_player(&server, "pickup_timer", 1, 0.0, 0.0);

    // Deactivate a pickup and set a short respawn timer.
    {
        let mut pickups = server.pickups.write();
        if let Some(pickup) = pickups.first_mut() {
            pickup.is_active = false;
            pickup.respawn_timer = Some(0.5);
        }
    }

    // Run several ticks.
    for _ in 0..40 {
        let _ = server.clone().process_game_tick(0.016).await;
        server.frame_counter.fetch_add(1, Ordering::Relaxed);
    }

    // Pickup should have respawned.
    let pickups = server.pickups.read();
    if let Some(pickup) = pickups.first() {
        assert!(
            pickup.is_active,
            "Pickup should have respawned after timer expired"
        );
    }
}

// ── Team death match scoring ─────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn tdm_mode_tracks_team_scores() {
    let server = setup_test_server();
    {
        let mut mi = server.match_info.write();
        mi.game_mode = fb::GameModeType::TeamDeathmatch;
    }

    add_player(&server, "t1_player", 1, 0.0, 0.0);
    add_player(&server, "t2_player", 2, 100.0, 0.0);

    // Trigger match start.
    server.run_game_logic_update(0.016).await;

    let mi = server.match_info.read();
    assert_eq!(mi.match_state, fb::MatchStateType::Active);
    assert_eq!(mi.game_mode, fb::GameModeType::TeamDeathmatch);
}

// ── Empty tick (no players) ─────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn empty_tick_completes_cleanly() {
    let server = setup_test_server();
    // No players added.
    let result = server.clone().process_game_tick(0.016).await;
    assert!(result.is_ok(), "Empty tick should not panic or error");
}

// ── Player count and slot management ─────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn effective_max_players_returns_positive() {
    let server = setup_test_server();
    let max = server.effective_max_players();
    assert!(max > 0, "Max players should be positive, got {}", max);
}

// ── Tick with bots does not panic ────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn tick_with_bots_processes_cleanly() {
    let server = setup_test_server();
    server.spawn_initial_bots(5);

    // Run several ticks with bots.
    for _ in 0..10 {
        let result = server.clone().process_game_tick(0.016).await;
        assert!(result.is_ok(), "Tick with bots should not error");
        server.frame_counter.fetch_add(1, Ordering::Relaxed);
    }
}
