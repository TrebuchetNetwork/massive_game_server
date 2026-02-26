// massive_game_server/server/src/operational/config/loader.rs

use crate::core::config::{ServerConfig, ThreadPoolConfig};
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct PartialThreadPoolConfig {
    physics_threads: Option<usize>,
    networking_threads: Option<usize>,
    game_logic_threads: Option<usize>,
    ai_threads: Option<usize>,
    io_threads: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialServerConfig {
    tick_rate: Option<u64>,
    num_player_shards: Option<usize>,
    world_partition_grid_dim: Option<usize>,
    cluster_shard_count: Option<usize>,
    local_shard_id: Option<usize>,
    max_players_per_match: Option<usize>,
    thread_pools: Option<PartialThreadPoolConfig>,
}

pub fn load_server_config_from_env_and_file() -> Result<ServerConfig> {
    let mut config = ServerConfig::default();

    if let Ok(path) = std::env::var("MGS_CONFIG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let raw = std::fs::read_to_string(trimmed)
                .with_context(|| format!("failed to read config file '{}'", trimmed))?;
            let partial: PartialServerConfig = if trimmed.ends_with(".json") {
                serde_json::from_str(&raw)
                    .with_context(|| format!("failed to parse json config '{}'", trimmed))?
            } else {
                serde_yaml::from_str(&raw)
                    .with_context(|| format!("failed to parse yaml config '{}'", trimmed))?
            };
            apply_partial(&mut config, partial);
        }
    }

    apply_env_overrides(&mut config);
    Ok(config)
}

fn apply_partial(config: &mut ServerConfig, partial: PartialServerConfig) {
    if let Some(tick_rate) = partial.tick_rate {
        config.tick_rate = tick_rate;
    }
    if let Some(num_player_shards) = partial.num_player_shards {
        config.num_player_shards = num_player_shards;
    }
    if let Some(world_partition_grid_dim) = partial.world_partition_grid_dim {
        config.world_partition_grid_dim = world_partition_grid_dim;
        config.num_world_partitions = world_partition_grid_dim * world_partition_grid_dim;
    }
    if let Some(cluster_shard_count) = partial.cluster_shard_count {
        config.cluster_shard_count = cluster_shard_count.max(1);
    }
    if let Some(local_shard_id) = partial.local_shard_id {
        config.local_shard_id = local_shard_id;
    }
    if let Some(max_players_per_match) = partial.max_players_per_match {
        config.max_players_per_match = max_players_per_match;
    }

    if let Some(thread_pools) = partial.thread_pools {
        apply_thread_pool_partial(&mut config.thread_pools, thread_pools);
    }
}

fn apply_thread_pool_partial(config: &mut ThreadPoolConfig, partial: PartialThreadPoolConfig) {
    if let Some(value) = partial.physics_threads {
        config.physics_threads = value;
    }
    if let Some(value) = partial.networking_threads {
        config.networking_threads = value;
    }
    if let Some(value) = partial.game_logic_threads {
        config.game_logic_threads = value;
    }
    if let Some(value) = partial.ai_threads {
        config.ai_threads = value;
    }
    if let Some(value) = partial.io_threads {
        config.io_threads = value;
    }
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
}

fn apply_env_overrides(config: &mut ServerConfig) {
    if let Some(value) = env_u64("MGS_TICK_RATE") {
        config.tick_rate = value;
    }
    if let Some(value) = env_usize("MGS_PLAYER_SHARDS") {
        config.num_player_shards = value;
    }
    if let Some(value) = env_usize("MGS_WORLD_GRID_DIM") {
        config.world_partition_grid_dim = value;
        config.num_world_partitions = value * value;
    }
    if let Some(value) = env_usize("MGS_CLUSTER_SHARDS") {
        config.cluster_shard_count = value.max(1);
    }
    if let Some(value) = env_usize("MGS_LOCAL_SHARD_ID") {
        config.local_shard_id = value;
    }
    if let Some(value) = env_usize("MGS_MAX_PLAYERS_PER_MATCH") {
        config.max_players_per_match = value;
    }

    if let Some(value) = env_usize("MGS_THREADS_PHYSICS") {
        config.thread_pools.physics_threads = value;
    }
    if let Some(value) = env_usize("MGS_THREADS_NETWORK") {
        config.thread_pools.networking_threads = value;
    }
    if let Some(value) = env_usize("MGS_THREADS_GAME") {
        config.thread_pools.game_logic_threads = value;
    }
    if let Some(value) = env_usize("MGS_THREADS_AI") {
        config.thread_pools.ai_threads = value;
    }
    if let Some(value) = env_usize("MGS_THREADS_IO") {
        config.thread_pools.io_threads = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_partial() {
        let mut config = ServerConfig::default();
        let partial = PartialServerConfig {
            tick_rate: Some(120),
            num_player_shards: Some(8),
            world_partition_grid_dim: Some(5),
            cluster_shard_count: Some(2),
            local_shard_id: Some(1),
            max_players_per_match: Some(64),
            thread_pools: Some(PartialThreadPoolConfig {
                physics_threads: Some(4),
                networking_threads: Some(4),
                game_logic_threads: Some(4),
                ai_threads: Some(4),
                io_threads: Some(4),
            }),
        };

        apply_partial(&mut config, partial);

        assert_eq!(config.tick_rate, 120);
        assert_eq!(config.num_player_shards, 8);
        assert_eq!(config.world_partition_grid_dim, 5);
        assert_eq!(config.num_world_partitions, 25);
        assert_eq!(config.cluster_shard_count, 2);
        assert_eq!(config.local_shard_id, 1);
        assert_eq!(config.max_players_per_match, 64);
        assert_eq!(config.thread_pools.physics_threads, 4);
    }
}
