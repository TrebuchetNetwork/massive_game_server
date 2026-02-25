mod bot;
mod metrics;
mod scenarios;

use std::process;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use tracing::info;

#[derive(Debug, Clone, ValueEnum)]
enum Scenario {
    /// Connect 2 bots/sec until 120 total
    Ramp120,
    /// Connect all 120 bots simultaneously
    Burst120,
    /// Connect 96 steady, then burst 24 more
    TailWave,
    /// Run all scenarios sequentially
    All,
}

#[derive(Parser, Debug)]
#[command(
    name = "stress-client",
    about = "Stress test client for massive_game_server",
    version
)]
struct Cli {
    /// WebSocket URL of the game server signaling endpoint
    #[arg(
        short = 'u',
        long,
        default_value = "ws://127.0.0.1:8080/ws",
        env = "MGS_STRESS_URL"
    )]
    server_url: String,

    /// Scenario to run
    #[arg(short, long, value_enum, default_value_t = Scenario::Ramp120)]
    scenario: Scenario,

    /// How long each bot runs its gameplay loop (seconds)
    #[arg(short = 'd', long, default_value_t = 30)]
    run_duration_secs: u64,

    /// Log verbosity (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info", env = "RUST_LOG")]
    log_level: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialise tracing
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_thread_ids(true)
        .init();

    let run_duration = Duration::from_secs(cli.run_duration_secs);

    info!(
        "stress-client starting: server={}, scenario={:?}, run_duration={}s",
        cli.server_url, cli.scenario, cli.run_duration_secs
    );

    let mut all_passed = true;

    match cli.scenario {
        Scenario::Ramp120 => {
            let result = scenarios::ramp_120(&cli.server_url, run_duration)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("ramp_120 error: {:#}", e);
                    scenarios::ScenarioResult {
                        name: "ramp_120".into(),
                        passed: false,
                    }
                });
            all_passed = result.passed;
        }
        Scenario::Burst120 => {
            let result = scenarios::burst_120(&cli.server_url, run_duration)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("burst_120 error: {:#}", e);
                    scenarios::ScenarioResult {
                        name: "burst_120".into(),
                        passed: false,
                    }
                });
            all_passed = result.passed;
        }
        Scenario::TailWave => {
            let result = scenarios::tail_wave(&cli.server_url, run_duration)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("tail_wave error: {:#}", e);
                    scenarios::ScenarioResult {
                        name: "tail_wave".into(),
                        passed: false,
                    }
                });
            all_passed = result.passed;
        }
        Scenario::All => {
            let scenario_names = ["ramp_120", "burst_120", "tail_wave"];

            for name in scenario_names {
                info!("--- Running scenario: {} ---", name);
                let result = match name {
                    "ramp_120" => scenarios::ramp_120(&cli.server_url, run_duration).await,
                    "burst_120" => scenarios::burst_120(&cli.server_url, run_duration).await,
                    "tail_wave" => scenarios::tail_wave(&cli.server_url, run_duration).await,
                    _ => unreachable!(),
                };
                let result = result.unwrap_or_else(|e| {
                    eprintln!("{} error: {:#}", name, e);
                    scenarios::ScenarioResult {
                        name: name.into(),
                        passed: false,
                    }
                });
                if !result.passed {
                    all_passed = false;
                }
                // Brief pause between scenarios to let the server settle
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    if all_passed {
        info!("All scenarios PASSED");
        process::exit(0);
    } else {
        info!("One or more scenarios FAILED");
        process::exit(1);
    }
}
