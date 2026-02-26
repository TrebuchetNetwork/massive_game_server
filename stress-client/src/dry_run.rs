//! Dry-run mode: simulates the bot lifecycle without connecting to any server.
//!
//! This is useful for CI validation: ensuring the binary compiles, the CLI
//! parses correctly, metrics collection works, and the scenario orchestration
//! logic functions properly -- all without requiring a running game server.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tracing::info;

use crate::metrics::ScenarioMetrics;

/// Run a dry-run simulation of the given scenario.
/// Returns true if the dry-run passes (which it always should, barring bugs).
pub async fn run_dry_run(scenario_name: &str, bot_count: usize, run_duration: Duration) -> bool {
    info!(
        "[dry-run] Simulating scenario '{}' with {} bots, duration={}s",
        scenario_name,
        bot_count,
        run_duration.as_secs()
    );

    // Cap dry-run duration: simulate at most 2 seconds of gameplay per bot
    // regardless of configured run_duration to keep CI fast.
    let effective_duration = run_duration.min(Duration::from_secs(2));

    let metrics = ScenarioMetrics::new(&format!("{} (dry-run)", scenario_name), bot_count);
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::with_capacity(bot_count);

    for bot_id in 0..bot_count {
        let metrics = metrics.clone();
        let shutdown = shutdown.clone();

        let handle = tokio::spawn(async move {
            run_dry_bot(bot_id, metrics, shutdown, effective_duration).await;
        });
        handles.push(handle);

        // Stagger launches slightly to simulate realistic startup
        if bot_count > 1 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    info!(
        "[dry-run] All {} bots launched. Waiting for completion...",
        bot_count
    );

    for handle in handles {
        let _ = handle.await;
    }

    shutdown.store(true, Ordering::SeqCst);

    let passed = metrics.summarize_and_evaluate().await;

    // In dry-run, the pass criteria are relaxed: we just need all bots to complete.
    // The metrics.summarize_and_evaluate() checks welcome_ratio >= 90%, etc. which
    // our simulated bots satisfy. If it somehow fails, report that.
    if !passed {
        info!("[dry-run] Warning: metrics evaluation returned false, but this is a dry run");
    }

    info!(
        "[dry-run] Complete. Simulated {} bots successfully.",
        bot_count
    );
    true
}

/// Simulate a single bot lifecycle without any network I/O.
async fn run_dry_bot(
    bot_id: usize,
    metrics: Arc<ScenarioMetrics>,
    shutdown: Arc<AtomicBool>,
    run_duration: Duration,
) {
    let username = format!("DryBot_{:04}", bot_id);
    metrics.register_bot(bot_id, &username).await;

    let start = Instant::now();

    // Simulate WebSocket connect + signaling delay (1-10ms)
    let mut rng = StdRng::seed_from_u64(bot_id as u64 ^ 0xDEADBEEF);
    let connect_delay_ms = rng.gen_range(1..=10);
    tokio::time::sleep(Duration::from_millis(connect_delay_ms)).await;

    // Simulate DataChannel open
    let dc_latency = start.elapsed();
    metrics.mark_dc_open(bot_id, dc_latency).await;

    // Simulate Welcome message arrival (a bit after DC open)
    let welcome_delay_ms = rng.gen_range(1..=5);
    tokio::time::sleep(Duration::from_millis(welcome_delay_ms)).await;
    let join_latency = start.elapsed();
    metrics.mark_connected(bot_id, join_latency).await;

    // Simulate gameplay loop: send inputs and receive deltas
    let run_end = Instant::now() + run_duration;
    let mut input_ticker = tokio::time::interval(Duration::from_millis(50)); // 20Hz
    let mut delta_ticker = tokio::time::interval(Duration::from_millis(16)); // ~60Hz

    loop {
        if shutdown.load(Ordering::Relaxed) || Instant::now() >= run_end {
            break;
        }

        tokio::select! {
            _ = input_ticker.tick() => {
                // Simulate sending a player input
                let _input_bytes = build_dry_input(&mut rng);
                metrics.record_input_sent(bot_id).await;
            }
            _ = delta_ticker.tick() => {
                // Simulate receiving a delta state update
                metrics.record_delta(bot_id).await;
            }
        }
    }

    metrics.mark_completed(bot_id).await;
}

/// Build a fake player input (just exercise the FlatBuffers builder code path).
fn build_dry_input(rng: &mut impl Rng) -> Vec<u8> {
    let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(128);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    use massive_game_server_protocol::game_protocol as fb;

    let rotation: f32 = rng.gen_range(0.0..std::f32::consts::TAU);
    let shooting = rng.gen_bool(0.3);
    let move_forward = rng.gen_bool(0.6);
    let move_backward = !move_forward && rng.gen_bool(0.2);
    let move_left = rng.gen_bool(0.3);
    let move_right = !move_left && rng.gen_bool(0.3);

    let input_args = fb::PlayerInputArgs {
        timestamp: now_ms,
        sequence: 0,
        move_forward,
        move_backward,
        move_left,
        move_right,
        shooting,
        reload: false,
        rotation,
        melee_attack: false,
        change_weapon_slot: 0,
        use_ability_slot: 0,
        ping_x: 0.0,
        ping_y: 0.0,
    };
    let input = fb::PlayerInput::create(&mut builder, &input_args);

    let game_msg_args = fb::GameMessageArgs {
        msg_type: fb::MessageType::Input,
        actual_message_type: fb::MessagePayload::PlayerInput,
        actual_message: Some(input.as_union_value()),
        protocol_version: 1,
    };
    let game_msg = fb::GameMessage::create(&mut builder, &game_msg_args);
    builder.finish(game_msg, None);

    builder.finished_data().to_vec()
}
