// server/src/operational/monitoring/metrics.rs
use metrics::{counter, gauge, histogram, describe_counter, describe_gauge, describe_histogram};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::time::Instant;
// Removed unused Arc and Mutex imports
use anyhow::{Context, Result}; // Use anyhow::Result and Context

pub struct MetricsSystem {
    start_time: Instant,
}

impl MetricsSystem {
    // Changed return type to anyhow::Result
    pub fn new() -> Result<Self> {
        // Initialize Prometheus exporter
        PrometheusBuilder::new()
            .with_http_listener(([0, 0, 0, 0], 9090))
            .install()
            .context("Failed to install Prometheus exporter")?; // Use anyhow::Context for error wrapping

        // Describe all metrics
        describe_counter!("game_frames_total", "Total number of game frames processed");
        describe_gauge!("game_players_connected", "Number of connected players");
        describe_gauge!("game_cpu_usage_percent", "CPU usage percentage"); // Added this based on previous discussions
        describe_histogram!("game_frame_time_seconds", "Frame processing time in seconds");
        describe_histogram!("game_physics_time_seconds", "Physics update time in seconds");
        describe_histogram!("game_network_time_seconds", "Network update time in seconds");
        // Add more descriptions as needed

        Ok(MetricsSystem {
            start_time: Instant::now(),
        })
    }

    pub fn record_frame_time(&self, duration: f64) {
        histogram!("game_frame_time_seconds").record(duration);
        counter!("game_frames_total").increment(1);
    }

    pub fn update_player_count(&self, count: usize) {
        gauge!("game_players_connected").set(count as f64);
    }

    pub fn record_subsystem_time(&self, subsystem: &str, duration: f64) {
        match subsystem {
            "physics" => histogram!("game_physics_time_seconds").record(duration),
            "network" => histogram!("game_network_time_seconds").record(duration),
            // Add other subsystems as needed
            _ => {}
        }
    }

    // Example for CPU usage, actual implementation would be platform specific
    pub fn record_cpu_usage(&self, usage_percent: f64) {
        gauge!("game_cpu_usage_percent").set(usage_percent);
    }
}

// Logging setup
pub fn init_logging() -> Result<()> { // Changed return type to anyhow::Result
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, fmt};

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "massive_game_server=debug,warn".into()),
        )
        .with(fmt::layer()) // Use fmt::layer()
        .try_init() // Use try_init for fallible initialization
        .context("Failed to initialize tracing subscriber")?; // Use anyhow::Context

    Ok(())
}
