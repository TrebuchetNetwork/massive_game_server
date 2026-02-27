mod commands;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use commands::monitor::{feature_flags, health_check};
use commands::performance::worker_stats;
use commands::players::list_recent_players;

#[derive(Debug, Parser)]
#[command(name = "mgs-admin", version, about = "Massive Game Server admin CLI")]
struct Cli {
    #[arg(
        long,
        env = "MGS_ADMIN_BASE_URL",
        default_value = "http://127.0.0.1:8080"
    )]
    base_url: String,
    #[arg(long, env = "MGS_ADMIN_BEARER_TOKEN")]
    admin_token: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Health,
    Players {
        #[arg(long, default_value_t = 16)]
        limit: usize,
    },
    Metrics,
    FeatureFlags,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Health => health_check(&cli.base_url).await?,
        Command::Players { limit } => {
            list_recent_players(&cli.base_url, cli.admin_token.as_deref(), limit).await?
        }
        Command::Metrics => worker_stats(&cli.base_url, cli.admin_token.as_deref()).await?,
        Command::FeatureFlags => feature_flags(&cli.base_url, cli.admin_token.as_deref()).await?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli_app() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
