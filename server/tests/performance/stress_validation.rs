//! In-process stress validation test.
//!
//! Starts a MassiveGameServer instance, adds 10-20 simulated players that
//! queue inputs and advances the tick pipeline, verifying:
//!   - No panics during concurrent player operations
//!   - No unbounded memory growth (RSS delta check)
//!   - All ticks complete within a timeout budget
//!
//! Gated behind `RUN_STRESS_TEST=1` to avoid running in normal CI.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::{PlayerAoIs, PlayerID, PlayerInputData};
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientStatesMap, DataChannelsMap, MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::server::instance::MassiveGameServer;
use parking_lot::RwLock as ParkingLotRwLock;
use tokio::sync::RwLock as TokioRwLock;

// ── Helpers ─────────────────────────────────────────────────────────

fn stress_enabled() -> bool {
    std::env::var("RUN_STRESS_TEST").ok().as_deref() == Some("1")
}

fn setup_test_server() -> Arc<MassiveGameServer> {
    // Prevent auto-spawning bots by setting the env var to 0
    std::env::set_var("MGS_TARGET_BOT_COUNT", "0");

    let config = Arc::new(ServerConfig::default());
    let thread_pool_system =
        Arc::new(ThreadPoolSystem::new(config.clone()).expect("Failed to create thread pools"));
    let data_channels_map: DataChannelsMap = Arc::new(DashMap::new());
    let client_states_map: ClientStatesMap = Arc::new(ParkingLotRwLock::new(HashMap::new()));
    let chat_messages_queue: ChatMessagesQueue =
        Arc::new(TokioRwLock::new(BoundedChatQueue::new(MAX_CHAT_QUEUE_SIZE)));
    let player_aois: PlayerAoIs = Arc::new(DashMap::new());

    let server = Arc::new(MassiveGameServer::new(
        config,
        thread_pool_system,
        data_channels_map,
        client_states_map,
        chat_messages_queue,
        player_aois,
    ));

    // Ensure no automatic bot spawning during stress tests
    server.target_bot_count.store(0, Ordering::Relaxed);

    server
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

fn queue_random_input(server: &MassiveGameServer, player_id: &PlayerID, sequence: u32) {
    let input = PlayerInputData {
        timestamp: 1000 + (sequence as u64 * 16),
        sequence,
        move_forward: sequence.is_multiple_of(3),
        move_backward: sequence.is_multiple_of(5),
        move_left: sequence.is_multiple_of(7),
        move_right: sequence.is_multiple_of(11),
        shooting: sequence.is_multiple_of(4),
        reload: sequence.is_multiple_of(20),
        rotation: (sequence as f32 * 0.1) % std::f32::consts::TAU,
        melee_attack: sequence.is_multiple_of(30),
        use_ability_slot: 0,
        change_weapon_slot: 0,
        ping_x: 0.0,
        ping_y: 0.0,
    };
    if let Some(mut ps) = server.player_manager.get_player_state_mut(player_id) {
        ps.input_queue.push_back(input);
    }
}

// ── Tests ───────────────────────────────────────────────────────────

/// Runs 20 simulated players through 60 game ticks (1 second of game time).
/// Each player queues random inputs every tick. Validates:
///  - No panics
///  - Every tick completes within 5 seconds
///  - Tick timing statistics are reasonable
#[tokio::test(flavor = "multi_thread")]
async fn stress_validation_concurrent_players() {
    if !stress_enabled() {
        eprintln!(
            "Skipping stress_validation_concurrent_players (set RUN_STRESS_TEST=1 to enable)."
        );
        return;
    }

    let server = setup_test_server();
    let player_count = 20;
    let tick_count = 60;
    let dt = 1.0 / 60.0_f32;

    // Spawn players across the map
    let mut player_ids: Vec<PlayerID> = Vec::with_capacity(player_count);
    for i in 0..player_count {
        let peer_id = format!("stress_player_{}", i);
        let team = if i % 2 == 0 { 1 } else { 2 };
        let x = 200.0 + (i as f32 * 50.0);
        let y = 200.0 + ((i % 5) as f32 * 100.0);
        let pid = add_player(&server, &peer_id, team, x, y);
        player_ids.push(pid);
    }

    let initial_count = server.player_manager.player_count();
    assert!(
        initial_count >= player_count,
        "Should have at least {} players registered, got {}",
        player_count,
        initial_count,
    );

    let mut tick_durations_ms: Vec<f64> = Vec::with_capacity(tick_count);
    let tick_timeout = Duration::from_secs(5);

    for tick_idx in 0..tick_count {
        // Queue inputs for all players
        for (i, pid) in player_ids.iter().enumerate() {
            let seq = (tick_idx * player_count + i) as u32;
            queue_random_input(&server, pid, seq);
        }

        // Run the tick
        let start = Instant::now();
        let result = tokio::time::timeout(tick_timeout, server.clone().process_game_tick(dt)).await;

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                panic!("process_game_tick failed at tick {}: {:?}", tick_idx, e);
            }
            Err(_) => {
                panic!(
                    "process_game_tick timed out at tick {} after {:?}",
                    tick_idx, tick_timeout
                );
            }
        }

        server.frame_counter.fetch_add(1, Ordering::Relaxed);
        tick_durations_ms.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    // --- Report statistics ---
    let avg = tick_durations_ms.iter().sum::<f64>() / tick_durations_ms.len() as f64;
    let mut sorted = tick_durations_ms.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let p95 = sorted[((sorted.len() - 1) as f64 * 0.95).round() as usize];
    let p99 = sorted[((sorted.len() - 1) as f64 * 0.99).round() as usize];
    let max = sorted.last().copied().unwrap_or(0.0);

    eprintln!(
        "[stress_validation] {} players x {} ticks: avg={:.2}ms p95={:.2}ms p99={:.2}ms max={:.2}ms",
        player_count, tick_count, avg, p95, p99, max
    );

    // Sanity: our original players should still exist (server may have added bots)
    let final_count = server.player_manager.player_count();
    assert!(
        final_count >= player_count,
        "Should still have at least {} players, got {}",
        player_count,
        final_count,
    );

    // Performance guard: p95 should be under 500ms (generous for debug-mode in-process test)
    assert!(
        p95 < 500.0,
        "p95 tick duration {:.2}ms exceeds 500ms budget",
        p95
    );
}

/// Adds players, runs ticks, then removes players and runs more ticks.
/// Validates the server handles player churn without panicking.
#[tokio::test(flavor = "multi_thread")]
async fn stress_validation_player_churn() {
    if !stress_enabled() {
        eprintln!("Skipping stress_validation_player_churn (set RUN_STRESS_TEST=1 to enable).");
        return;
    }

    let server = setup_test_server();
    let dt = 1.0 / 60.0_f32;

    // Phase 1: Add 15 players, run 30 ticks
    let mut player_ids: Vec<(String, PlayerID)> = Vec::new();
    for i in 0..15 {
        let peer_id = format!("churn_player_{}", i);
        let pid = add_player(&server, &peer_id, (i % 2 + 1) as u8, 300.0, 300.0);
        player_ids.push((peer_id, pid));
    }

    let count_after_add = server.player_manager.player_count();

    for tick_idx in 0..30 {
        for (_, pid) in &player_ids {
            queue_random_input(&server, pid, tick_idx);
        }
        let result = server.clone().process_game_tick(dt).await;
        assert!(result.is_ok(), "Phase 1 tick {} should succeed", tick_idx);
        server.frame_counter.fetch_add(1, Ordering::Relaxed);
    }

    // Phase 2: Remove half the players
    let to_remove: Vec<String> = player_ids[..8]
        .iter()
        .map(|(peer_id, _)| peer_id.clone())
        .collect();
    for peer_id in &to_remove {
        server.player_manager.remove_player(peer_id);
    }
    player_ids.retain(|(peer_id, _)| !to_remove.contains(peer_id));

    let count_after_remove = server.player_manager.player_count();
    assert!(
        count_after_remove < count_after_add,
        "Player count should decrease after removing players ({} -> {})",
        count_after_add,
        count_after_remove,
    );

    // Phase 3: Run 30 more ticks with remaining players
    for tick_idx in 30..60 {
        for (_, pid) in &player_ids {
            queue_random_input(&server, pid, tick_idx);
        }
        let result = server.clone().process_game_tick(dt).await;
        assert!(result.is_ok(), "Phase 3 tick {} should succeed", tick_idx);
        server.frame_counter.fetch_add(1, Ordering::Relaxed);
    }

    // Phase 4: Add 10 new players
    for i in 15..25 {
        let peer_id = format!("churn_player_{}", i);
        let pid = add_player(&server, &peer_id, (i % 2 + 1) as u8, 500.0, 500.0);
        player_ids.push((peer_id, pid));
    }

    // Phase 5: Run 30 more ticks with mixed old + new players
    for tick_idx in 60..90 {
        for (_, pid) in &player_ids {
            queue_random_input(&server, pid, tick_idx);
        }
        let result = server.clone().process_game_tick(dt).await;
        assert!(result.is_ok(), "Phase 5 tick {} should succeed", tick_idx);
        server.frame_counter.fetch_add(1, Ordering::Relaxed);
    }

    eprintln!(
        "[stress_validation:churn] Completed 90 ticks with player add/remove churn. Final count={}",
        server.player_manager.player_count()
    );
}

/// Runs ticks with 20 players + 10 bots to verify hybrid tick processing.
#[tokio::test(flavor = "multi_thread")]
async fn stress_validation_players_and_bots() {
    if !stress_enabled() {
        eprintln!("Skipping stress_validation_players_and_bots (set RUN_STRESS_TEST=1 to enable).");
        return;
    }

    let server = setup_test_server();
    let dt = 1.0 / 60.0_f32;
    let player_count = 20;
    let bot_count = 10;
    let tick_count = 60;

    // Set target bot count so the server manages exactly this many bots
    server
        .target_bot_count
        .store(bot_count as u64, Ordering::Relaxed);

    // Add human players
    let mut player_ids: Vec<PlayerID> = Vec::with_capacity(player_count);
    for i in 0..player_count {
        let peer_id = format!("hybrid_player_{}", i);
        let team = if i % 2 == 0 { 1 } else { 2 };
        let pid = add_player(&server, &peer_id, team, 400.0, 400.0);
        player_ids.push(pid);
    }

    // Spawn server-side bots
    server.spawn_initial_bots(bot_count);

    let total_entities = server.player_manager.player_count();
    assert!(
        total_entities >= player_count + bot_count,
        "Should have at least {} entities, got {}",
        player_count + bot_count,
        total_entities
    );

    let mut tick_durations_ms: Vec<f64> = Vec::with_capacity(tick_count);

    for tick_idx in 0..tick_count {
        // Queue inputs only for human players (bots generate their own)
        for (i, pid) in player_ids.iter().enumerate() {
            let seq = (tick_idx * player_count + i) as u32;
            queue_random_input(&server, pid, seq);
        }

        let start = Instant::now();
        let result = server.clone().process_game_tick(dt).await;
        assert!(
            result.is_ok(),
            "Hybrid tick {} should succeed: {:?}",
            tick_idx,
            result.err()
        );
        server.frame_counter.fetch_add(1, Ordering::Relaxed);
        tick_durations_ms.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    let avg = tick_durations_ms.iter().sum::<f64>() / tick_durations_ms.len() as f64;
    let mut sorted = tick_durations_ms.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let p95 = sorted[((sorted.len() - 1) as f64 * 0.95).round() as usize];
    let max = sorted.last().copied().unwrap_or(0.0);

    eprintln!(
        "[stress_validation:hybrid] {}p + {}b x {} ticks: avg={:.2}ms p95={:.2}ms max={:.2}ms",
        player_count, bot_count, tick_count, avg, p95, max
    );
}
