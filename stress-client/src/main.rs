mod bot;
mod dry_run;
mod metrics;
mod scenarios;

use std::process;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use tracing::info;

#[derive(Debug, Clone, ValueEnum)]
enum Scenario {
    /// Connect 2 bots/sec until target count
    Ramp120,
    /// Connect all bots simultaneously
    Burst120,
    /// Connect 80% steady, then burst remaining 20%
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

    /// Number of bots per scenario (overrides the default 120)
    #[arg(short = 'n', long, default_value_t = 120, env = "MGS_STRESS_BOTS")]
    bot_count: usize,

    /// Dry-run mode: simulates the bot lifecycle without connecting to a server.
    /// Useful for CI validation that the binary compiles and runs correctly.
    #[arg(long, default_value_t = false)]
    dry_run: bool,

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
        "stress-client starting: server={}, scenario={:?}, bot_count={}, run_duration={}s, dry_run={}",
        cli.server_url, cli.scenario, cli.bot_count, cli.run_duration_secs, cli.dry_run
    );

    if cli.dry_run {
        let result = dry_run::run_dry_run(&cli.scenario_name(), cli.bot_count, run_duration).await;
        if result {
            info!("Dry-run PASSED");
            process::exit(0);
        } else {
            info!("Dry-run FAILED");
            process::exit(1);
        }
    }

    let mut all_passed = true;

    match cli.scenario {
        Scenario::Ramp120 => {
            let result = scenarios::ramp(&cli.server_url, cli.bot_count, run_duration)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("ramp error: {:#}", e);
                    scenarios::ScenarioResult {
                        name: "ramp".into(),
                        passed: false,
                    }
                });
            all_passed = result.passed;
        }
        Scenario::Burst120 => {
            let result = scenarios::burst(&cli.server_url, cli.bot_count, run_duration)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("burst error: {:#}", e);
                    scenarios::ScenarioResult {
                        name: "burst".into(),
                        passed: false,
                    }
                });
            all_passed = result.passed;
        }
        Scenario::TailWave => {
            let result = scenarios::tail_wave(&cli.server_url, cli.bot_count, run_duration)
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
            let scenario_names = ["ramp", "burst", "tail_wave"];

            for name in scenario_names {
                info!("--- Running scenario: {} ---", name);
                let result = match name {
                    "ramp" => scenarios::ramp(&cli.server_url, cli.bot_count, run_duration).await,
                    "burst" => scenarios::burst(&cli.server_url, cli.bot_count, run_duration).await,
                    "tail_wave" => {
                        scenarios::tail_wave(&cli.server_url, cli.bot_count, run_duration).await
                    }
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

impl Cli {
    fn scenario_name(&self) -> String {
        match self.scenario {
            Scenario::Ramp120 => "ramp".to_string(),
            Scenario::Burst120 => "burst".to_string(),
            Scenario::TailWave => "tail_wave".to_string(),
            Scenario::All => "all".to_string(),
        }
    }
}
