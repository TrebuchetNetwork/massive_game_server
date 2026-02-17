// server/src/operational/monitoring/metrics.rs
use anyhow::{Context, Result};
use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::PrometheusBuilder;
use once_cell::sync::OnceCell;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tracing::{info, warn};

static METRICS_ENABLED: AtomicBool = AtomicBool::new(false);
static METRICS_INSTALL_ONCE: OnceCell<()> = OnceCell::new();

fn parse_bool_env(var_name: &str, default_value: bool) -> bool {
    std::env::var(var_name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(default_value)
}

fn parse_listener_addr() -> SocketAddr {
    let raw = std::env::var("MGS_PROMETHEUS_LISTEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "0.0.0.0:9090".to_owned());
    raw.parse::<SocketAddr>().unwrap_or_else(|err| {
        warn!(
            "Invalid MGS_PROMETHEUS_LISTEN='{}' ({}), falling back to 0.0.0.0:9090",
            raw, err
        );
        SocketAddr::from(([0, 0, 0, 0], 9090))
    })
}

fn describe_metrics_catalog() {
    describe_counter!("game_frames_total", "Total number of game frames processed");
    describe_histogram!("game_frame_time_seconds", "Frame processing time in seconds");
    describe_histogram!("game_physics_time_seconds", "Physics update time in seconds");
    describe_histogram!("game_network_time_seconds", "Network update time in seconds");
    describe_gauge!("game_players_connected", "Number of connected players");
    describe_gauge!("game_cpu_usage_percent", "CPU usage percentage");
    describe_gauge!("game_memory_rss_bytes", "Process resident set size in bytes");
    describe_gauge!(
        "game_memory_heap_allocated_bytes",
        "Process heap allocated bytes when available"
    );
    describe_counter!(
        "game_network_bytes_total",
        "Total network payload bytes observed by direction"
    );
    describe_gauge!(
        "game_ws_connections_active",
        "Active WebSocket signaling connections"
    );
    describe_counter!(
        "game_auth_attempts_total",
        "Authentication attempts by stage and result"
    );
    describe_histogram!(
        "game_auth_token_resolution_seconds",
        "Auth token lookup/validation latency in seconds"
    );
    describe_counter!(
        "game_backup_runs_total",
        "Automated backup runs by result (success/failure)"
    );
    describe_histogram!(
        "game_backup_duration_seconds",
        "Automated backup duration in seconds"
    );
    describe_histogram!(
        "game_shutdown_duration_seconds",
        "Graceful shutdown duration in seconds"
    );
}

pub fn init_metrics_exporter_from_env() -> Result<()> {
    if !parse_bool_env("MGS_METRICS_ENABLED", true) {
        METRICS_ENABLED.store(false, Ordering::Release);
        warn!("Prometheus metrics exporter disabled via MGS_METRICS_ENABLED.");
        return Ok(());
    }

    if METRICS_INSTALL_ONCE.get().is_none() {
        let listener = parse_listener_addr();
        PrometheusBuilder::new()
            .with_http_listener(listener)
            .install()
            .context("failed to install Prometheus exporter")?;
        describe_metrics_catalog();
        let _ = METRICS_INSTALL_ONCE.set(());
        info!("Prometheus exporter listening on {}", listener);
    }
    METRICS_ENABLED.store(true, Ordering::Release);
    Ok(())
}

#[inline]
fn enabled() -> bool {
    METRICS_ENABLED.load(Ordering::Acquire)
}

pub fn record_frame_metrics(frame_duration_seconds: f64, connected_players: usize) {
    if !enabled() {
        return;
    }
    histogram!("game_frame_time_seconds").record(frame_duration_seconds);
    counter!("game_frames_total").increment(1);
    gauge!("game_players_connected").set(connected_players as f64);
}

pub fn record_subsystem_time(subsystem: &str, duration_seconds: f64) {
    if !enabled() {
        return;
    }
    match subsystem {
        "physics" => histogram!("game_physics_time_seconds").record(duration_seconds),
        "network" => histogram!("game_network_time_seconds").record(duration_seconds),
        _ => {}
    }
}

pub fn record_cpu_usage(usage_percent: f64) {
    if !enabled() {
        return;
    }
    gauge!("game_cpu_usage_percent").set(usage_percent.max(0.0));
}

pub fn record_memory_usage(resident_bytes: Option<u64>, heap_allocated_bytes: Option<u64>) {
    if !enabled() {
        return;
    }
    if let Some(rss) = resident_bytes {
        gauge!("game_memory_rss_bytes").set(rss as f64);
    }
    if let Some(heap) = heap_allocated_bytes {
        gauge!("game_memory_heap_allocated_bytes").set(heap as f64);
    }
}

pub fn record_network_bytes(direction: &'static str, bytes: usize) {
    if !enabled() {
        return;
    }
    counter!("game_network_bytes_total", "direction" => direction).increment(bytes as u64);
}

pub fn set_ws_connections_active(count: usize) {
    if !enabled() {
        return;
    }
    gauge!("game_ws_connections_active").set(count as f64);
}

pub fn record_auth_attempt(stage: &'static str, result: &'static str) {
    if !enabled() {
        return;
    }
    counter!(
        "game_auth_attempts_total",
        "stage" => stage,
        "result" => result
    )
    .increment(1);
}

pub fn record_auth_token_resolution(duration_seconds: f64, result: &'static str) {
    if !enabled() {
        return;
    }
    histogram!(
        "game_auth_token_resolution_seconds",
        "result" => result
    )
    .record(duration_seconds.max(0.0));
}

pub fn record_backup_result(result: &'static str, duration_seconds: f64) {
    if !enabled() {
        return;
    }
    counter!("game_backup_runs_total", "result" => result).increment(1);
    histogram!("game_backup_duration_seconds", "result" => result)
        .record(duration_seconds.max(0.0));
}

pub fn record_shutdown_duration(duration_seconds: f64) {
    if !enabled() {
        return;
    }
    histogram!("game_shutdown_duration_seconds").record(duration_seconds.max(0.0));
}

pub struct MetricsSystem {
    pub start_time: Instant,
}

impl MetricsSystem {
    pub fn new() -> Result<Self> {
        init_metrics_exporter_from_env()?;
        Ok(Self {
            start_time: Instant::now(),
        })
    }
}
