use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tracing::{error, info};

use crate::bot::run_bot;
use crate::metrics::ScenarioMetrics;

/// Configuration for a stress test scenario.
pub struct ScenarioConfig {
    /// Human-readable name.
    pub name: String,
    /// WebSocket URL to the server (e.g. ws://localhost:8080/ws).
    pub server_url: String,
    /// Total number of bots to connect.
    pub total_bots: usize,
    /// How long each bot runs after connecting (gameplay loop duration).
    pub run_duration: Duration,
}

/// Result of a scenario run.
#[allow(dead_code)]
pub struct ScenarioResult {
    pub name: String,
    pub passed: bool,
}

// ---------------------------------------------------------------------------
// Scenario: ramp
// Connect 2 clients per second until total_bots are connected.
// ---------------------------------------------------------------------------
pub async fn ramp(
    server_url: &str,
    total_bots: usize,
    run_duration: Duration,
) -> Result<ScenarioResult> {
    let config = ScenarioConfig {
        name: "ramp".to_string(),
        server_url: server_url.to_string(),
        total_bots,
        run_duration,
    };
    let bots_per_sec = 2;
    run_ramp_scenario(config, bots_per_sec).await
}

// ---------------------------------------------------------------------------
// Scenario: burst
// Connect all bots simultaneously.
// ---------------------------------------------------------------------------
pub async fn burst(
    server_url: &str,
    total_bots: usize,
    run_duration: Duration,
) -> Result<ScenarioResult> {
    let config = ScenarioConfig {
        name: "burst".to_string(),
        server_url: server_url.to_string(),
        total_bots,
        run_duration,
    };
    run_burst_scenario(config).await
}

// ---------------------------------------------------------------------------
// Scenario: tail_wave
// Connect 80% bots at a steady rate (4/sec), then burst remaining 20%.
// ---------------------------------------------------------------------------
pub async fn tail_wave(
    server_url: &str,
    total_bots: usize,
    run_duration: Duration,
) -> Result<ScenarioResult> {
    let name = "tail_wave".to_string();
    let steady_count = (total_bots as f64 * 0.8).ceil() as usize;
    let burst_count = total_bots - steady_count;
    let steady_rate: u32 = 4; // per second

    let metrics = ScenarioMetrics::new(&name, total_bots);
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::with_capacity(total_bots);

    let server_url_owned = server_url.to_string();

    // Phase 1: Steady ramp of steady_count bots at steady_rate/sec
    info!(
        "[{}] Phase 1: ramping {} bots at {}/sec",
        name, steady_count, steady_rate
    );
    let interval = Duration::from_secs(1) / steady_rate;

    for bot_id in 0..steady_count {
        let metrics = metrics.clone();
        let shutdown = shutdown.clone();
        let server_url = server_url_owned.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) =
                run_bot(bot_id, &server_url, metrics.clone(), shutdown, run_duration).await
            {
                error!("bot#{}: fatal error: {:#}", bot_id, e);
                metrics.mark_disconnected(bot_id, &format!("{:#}", e)).await;
            }
        });
        handles.push(handle);

        tokio::time::sleep(interval).await;
    }

    // Brief settle period
    info!("[{}] Phase 1 complete. Settling for 2s...", name);
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Phase 2: Burst remaining bots
    info!("[{}] Phase 2: bursting {} bots", name, burst_count);
    for bot_id in steady_count..total_bots {
        let metrics = metrics.clone();
        let shutdown = shutdown.clone();
        let server_url = server_url_owned.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) =
                run_bot(bot_id, &server_url, metrics.clone(), shutdown, run_duration).await
            {
                error!("bot#{}: fatal error: {:#}", bot_id, e);
                metrics.mark_disconnected(bot_id, &format!("{:#}", e)).await;
            }
        });
        handles.push(handle);
    }

    // Wait for all bots to finish
    info!("[{}] Waiting for {} bots to complete...", name, total_bots);
    for handle in handles {
        let _ = handle.await;
    }

    shutdown.store(true, Ordering::SeqCst);
    let passed = metrics.summarize_and_evaluate().await;

    Ok(ScenarioResult { name, passed })
}

// ---------------------------------------------------------------------------
// Internal: ramp scenario implementation
// ---------------------------------------------------------------------------
async fn run_ramp_scenario(config: ScenarioConfig, bots_per_sec: usize) -> Result<ScenarioResult> {
    let metrics = ScenarioMetrics::new(&config.name, config.total_bots);
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::with_capacity(config.total_bots);

    let interval = Duration::from_secs(1) / bots_per_sec as u32;

    info!(
        "[{}] Ramping {} bots at {}/sec to {}",
        config.name, config.total_bots, bots_per_sec, config.server_url
    );

    for bot_id in 0..config.total_bots {
        let metrics = metrics.clone();
        let shutdown = shutdown.clone();
        let server_url = config.server_url.clone();
        let run_duration = config.run_duration;

        let handle = tokio::spawn(async move {
            if let Err(e) =
                run_bot(bot_id, &server_url, metrics.clone(), shutdown, run_duration).await
            {
                error!("bot#{}: fatal error: {:#}", bot_id, e);
                metrics.mark_disconnected(bot_id, &format!("{:#}", e)).await;
            }
        });
        handles.push(handle);

        tokio::time::sleep(interval).await;
    }

    info!(
        "[{}] All {} bots launched. Waiting for completion...",
        config.name, config.total_bots
    );

    for handle in handles {
        let _ = handle.await;
    }

    shutdown.store(true, Ordering::SeqCst);
    let passed = metrics.summarize_and_evaluate().await;

    Ok(ScenarioResult {
        name: config.name,
        passed,
    })
}

// ---------------------------------------------------------------------------
// Internal: burst scenario implementation
// ---------------------------------------------------------------------------
async fn run_burst_scenario(config: ScenarioConfig) -> Result<ScenarioResult> {
    let metrics = ScenarioMetrics::new(&config.name, config.total_bots);
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::with_capacity(config.total_bots);

    info!(
        "[{}] Bursting {} bots simultaneously to {}",
        config.name, config.total_bots, config.server_url
    );

    for bot_id in 0..config.total_bots {
        let metrics = metrics.clone();
        let shutdown = shutdown.clone();
        let server_url = config.server_url.clone();
        let run_duration = config.run_duration;

        let handle = tokio::spawn(async move {
            if let Err(e) =
                run_bot(bot_id, &server_url, metrics.clone(), shutdown, run_duration).await
            {
                error!("bot#{}: fatal error: {:#}", bot_id, e);
                metrics.mark_disconnected(bot_id, &format!("{:#}", e)).await;
            }
        });
        handles.push(handle);
    }

    info!(
        "[{}] All {} bots launched. Waiting for completion...",
        config.name, config.total_bots
    );

    for handle in handles {
        let _ = handle.await;
    }

    shutdown.store(true, Ordering::SeqCst);
    let passed = metrics.summarize_and_evaluate().await;

    Ok(ScenarioResult {
        name: config.name,
        passed,
    })
}
