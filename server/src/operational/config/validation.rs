// massive_game_server/server/src/operational/config/validation.rs

use crate::core::config::ServerConfig;
use anyhow::{anyhow, Result};

pub fn validate_server_config(config: &ServerConfig) -> Result<()> {
    if config.tick_rate == 0 || config.tick_rate > 240 {
        return Err(anyhow!("tick_rate must be in range 1..=240"));
    }
    if config.num_player_shards == 0 {
        return Err(anyhow!("num_player_shards must be > 0"));
    }
    if config.world_partition_grid_dim == 0 {
        return Err(anyhow!("world_partition_grid_dim must be > 0"));
    }
    if config.cluster_shard_count == 0 {
        return Err(anyhow!("cluster_shard_count must be > 0"));
    }
    if config.local_shard_id >= config.cluster_shard_count {
        return Err(anyhow!(
            "local_shard_id must be < cluster_shard_count ({} >= {})",
            config.local_shard_id,
            config.cluster_shard_count
        ));
    }
    if config.max_players_per_match == 0 {
        return Err(anyhow!("max_players_per_match must be > 0"));
    }

    let pools = &config.thread_pools;
    if pools.physics_threads == 0
        || pools.networking_threads == 0
        || pools.game_logic_threads == 0
        || pools.ai_threads == 0
        || pools.io_threads == 0
    {
        return Err(anyhow!("all thread pool counts must be > 0"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config() {
        let config = ServerConfig::default();
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_invalid_tick_rate() {
        let mut config = ServerConfig::default();
        config.tick_rate = 0;
        assert!(validate_server_config(&config).is_err());
        config.tick_rate = 241;
        assert!(validate_server_config(&config).is_err());
    }

    #[test]
    fn test_invalid_thread_pools() {
        let mut config = ServerConfig::default();
        config.thread_pools.physics_threads = 0;
        assert!(validate_server_config(&config).is_err());
    }
}
