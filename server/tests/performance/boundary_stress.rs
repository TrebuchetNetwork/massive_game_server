use std::collections::HashMap;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::PlayerAoIs;
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientStatesMap, DataChannelsMap, MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::server::instance::MassiveGameServer;
use metrics::histogram;
use parking_lot::RwLock as ParkingLotRwLock;
use tokio::sync::RwLock as TokioRwLock;

#[tokio::test]
async fn stress_test_game_tick() {
    if !stress_enabled() {
        eprintln!("Skipping stress_test_game_tick (set RUN_STRESS_TEST=1 to enable).");
        return;
    }

    let server = Arc::new(setup_test_server());
    let iterations = env_usize("STRESS_TICKS", 120);
    let samples_ms = run_ticks(server, iterations).await;

    assert!(
        !samples_ms.is_empty(),
        "No tick samples collected in stress_test_game_tick."
    );
    report_and_optionally_enforce(
        "baseline",
        &samples_ms,
        "STRESS_P95_BUDGET_MS",
        "STRESS_MAX_TICK_MS",
    );
}

#[tokio::test]
async fn stress_test_game_tick_with_bots() {
    if !stress_enabled() {
        eprintln!("Skipping stress_test_game_tick_with_bots (set RUN_STRESS_TEST=1 to enable).");
        return;
    }

    let server = Arc::new(setup_test_server());
    let bot_count = env_usize("STRESS_BOTS", 200);
    let target_bot_count = env_usize("STRESS_TARGET_BOT_COUNT", bot_count);
    server
        .target_bot_count
        .store(target_bot_count as u64, AtomicOrdering::Relaxed);
    server.spawn_initial_bots(bot_count);

    let iterations = env_usize("STRESS_TICKS", 120);
    let samples_ms = run_ticks(Arc::clone(&server), iterations).await;

    assert!(
        !samples_ms.is_empty(),
        "No tick samples collected in stress_test_game_tick_with_bots."
    );
    report_and_optionally_enforce(
        "bots",
        &samples_ms,
        "STRESS_BOT_P95_BUDGET_MS",
        "STRESS_BOT_MAX_TICK_MS",
    );
}

fn stress_enabled() -> bool {
    std::env::var("RUN_STRESS_TEST").ok().as_deref() == Some("1")
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_f64(name: &str) -> Option<f64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
}

async fn run_ticks(server: Arc<MassiveGameServer>, iterations: usize) -> Vec<f64> {
    let delta_time = 1.0 / 60.0;
    let mut samples_ms = Vec::with_capacity(iterations);
    let tick_timeout = Duration::from_secs(env_usize("STRESS_TICK_TIMEOUT_SECS", 20) as u64);

    for tick_idx in 0..iterations {
        let start = Instant::now();
        let tick_result =
            tokio::time::timeout(tick_timeout, server.clone().process_game_tick(delta_time)).await;
        match tick_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                panic!("process_game_tick failed at tick {}: {:?}", tick_idx, e);
            }
            Err(_) => {
                panic!(
                    "process_game_tick timed out at tick {} after {:?}. This indicates a stall/deadlock in the tick pipeline.",
                    tick_idx,
                    tick_timeout
                );
            }
        }
        server.frame_counter.fetch_add(1, AtomicOrdering::Relaxed);
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        histogram!("game_tick_duration_ms").record(elapsed_ms);
        samples_ms.push(elapsed_ms);
    }

    samples_ms
}

fn report_and_optionally_enforce(
    label: &str,
    samples_ms: &[f64],
    p95_budget_env: &str,
    max_budget_env: &str,
) {
    let avg_ms = samples_ms.iter().sum::<f64>() / samples_ms.len() as f64;
    let p95_ms = percentile(samples_ms, 0.95);
    let max_ms = samples_ms.iter().fold(
        0.0_f64,
        |acc, value| if *value > acc { *value } else { acc },
    );

    eprintln!(
        "[stress:{label}] samples={} avg_ms={:.2} p95_ms={:.2} max_ms={:.2}",
        samples_ms.len(),
        avg_ms,
        p95_ms,
        max_ms
    );

    if let Some(p95_budget) = env_f64(p95_budget_env) {
        assert!(
            p95_ms <= p95_budget,
            "[stress:{label}] p95 tick {:.2}ms exceeded {}={:.2}ms",
            p95_ms,
            p95_budget_env,
            p95_budget
        );
    }

    if let Some(max_budget) = env_f64(max_budget_env) {
        assert!(
            max_ms <= max_budget,
            "[stress:{label}] max tick {:.2}ms exceeded {}={:.2}ms",
            max_ms,
            max_budget_env,
            max_budget
        );
    }
}

fn percentile(samples_ms: &[f64], p: f64) -> f64 {
    if samples_ms.is_empty() {
        return 0.0;
    }
    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));

    let index = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[index]
}

fn setup_test_server() -> MassiveGameServer {
    let config = Arc::new(ServerConfig::default());
    let thread_pool_system =
        Arc::new(ThreadPoolSystem::new(config.clone()).expect("Failed to create thread pools"));
    let data_channels_map: DataChannelsMap = Arc::new(DashMap::new());
    let client_states_map: ClientStatesMap = Arc::new(ParkingLotRwLock::new(HashMap::new()));
    let chat_messages_queue: ChatMessagesQueue =
        Arc::new(TokioRwLock::new(BoundedChatQueue::new(MAX_CHAT_QUEUE_SIZE)));
    let player_aois: PlayerAoIs = Arc::new(DashMap::new());

    MassiveGameServer::new(
        config,
        thread_pool_system,
        data_channels_map,
        client_states_map,
        chat_messages_queue,
        player_aois,
    )
}
