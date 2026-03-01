// massive_game_server/server/src/operational/config/loader.rs

use crate::core::config::{ServerConfig, ThreadPoolConfig};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialThreadPoolConfig {
    physics_threads: Option<usize>,
    networking_threads: Option<usize>,
    game_logic_threads: Option<usize>,
    ai_threads: Option<usize>,
    io_threads: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialServerConfig {
    tick_rate: Option<u64>,
    num_player_shards: Option<usize>,
    world_partition_grid_dim: Option<usize>,
    num_world_partitions: Option<usize>,
    cluster_shard_count: Option<usize>,
    local_shard_id: Option<usize>,
    #[serde(alias = "max_players")]
    max_players_per_match: Option<usize>,
    thread_pools: Option<PartialThreadPoolConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct ConfigDocument {
    includes: Option<Vec<String>>,
    server: Option<PartialServerConfig>,
    #[serde(flatten)]
    flat_server: PartialServerConfig,
}

impl ConfigDocument {
    fn into_partial(self) -> PartialServerConfig {
        let mut merged = self.flat_server;
        if let Some(server) = self.server {
            merge_partial(&mut merged, server);
        }
        merged
    }
}

pub fn load_server_config_from_env_and_file() -> Result<ServerConfig> {
    let mut config = ServerConfig::default();

    if let Ok(path) = std::env::var("MGS_CONFIG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let mut visited = HashSet::new();
            let partial = load_partial_with_includes(Path::new(trimmed), &mut visited)?;
            apply_partial(&mut config, partial);
        }
    }

    apply_env_overrides(&mut config);
    Ok(config)
}

fn parse_partial_document(config_path: &Path, raw: &str) -> Result<ConfigDocument> {
    let is_json = config_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    if is_json {
        serde_json::from_str(raw)
            .with_context(|| format!("failed to parse json config '{}'", config_path.display()))
    } else {
        serde_yaml::from_str(raw)
            .with_context(|| format!("failed to parse yaml config '{}'", config_path.display()))
    }
}

fn load_partial_with_includes(
    config_path: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<PartialServerConfig> {
    let canonical_path = std::fs::canonicalize(config_path)
        .with_context(|| format!("failed to resolve config file '{}'", config_path.display()))?;

    if !visited.insert(canonical_path.clone()) {
        return Err(anyhow!(
            "config include cycle detected at '{}'",
            canonical_path.display()
        ));
    }

    let raw = std::fs::read_to_string(&canonical_path)
        .with_context(|| format!("failed to read config file '{}'", canonical_path.display()))?;
    let document = parse_partial_document(&canonical_path, &raw)?;

    let mut merged = PartialServerConfig::default();
    let parent_dir = canonical_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    if let Some(includes) = document.includes.as_ref() {
        for include in includes {
            let include_trimmed = include.trim();
            if include_trimmed.is_empty() {
                continue;
            }

            let include_path = if Path::new(include_trimmed).is_absolute() {
                PathBuf::from(include_trimmed)
            } else {
                parent_dir.join(include_trimmed)
            };

            let include_partial =
                load_partial_with_includes(&include_path, visited).with_context(|| {
                    format!(
                        "failed loading include '{}' from '{}'",
                        include_trimmed,
                        canonical_path.display()
                    )
                })?;
            merge_partial(&mut merged, include_partial);
        }
    }

    merge_partial(&mut merged, document.into_partial());
    visited.remove(&canonical_path);
    Ok(merged)
}

fn merge_partial(base: &mut PartialServerConfig, overlay: PartialServerConfig) {
    if let Some(value) = overlay.tick_rate {
        base.tick_rate = Some(value);
    }
    if let Some(value) = overlay.num_player_shards {
        base.num_player_shards = Some(value);
    }
    if let Some(value) = overlay.world_partition_grid_dim {
        base.world_partition_grid_dim = Some(value);
    }
    if let Some(value) = overlay.num_world_partitions {
        base.num_world_partitions = Some(value);
    }
    if let Some(value) = overlay.cluster_shard_count {
        base.cluster_shard_count = Some(value);
    }
    if let Some(value) = overlay.local_shard_id {
        base.local_shard_id = Some(value);
    }
    if let Some(value) = overlay.max_players_per_match {
        base.max_players_per_match = Some(value);
    }
    if let Some(thread_overlay) = overlay.thread_pools {
        match base.thread_pools.as_mut() {
            Some(thread_base) => merge_thread_pool_partial(thread_base, thread_overlay),
            None => base.thread_pools = Some(thread_overlay),
        }
    }
}

fn merge_thread_pool_partial(base: &mut PartialThreadPoolConfig, overlay: PartialThreadPoolConfig) {
    if let Some(value) = overlay.physics_threads {
        base.physics_threads = Some(value);
    }
    if let Some(value) = overlay.networking_threads {
        base.networking_threads = Some(value);
    }
    if let Some(value) = overlay.game_logic_threads {
        base.game_logic_threads = Some(value);
    }
    if let Some(value) = overlay.ai_threads {
        base.ai_threads = Some(value);
    }
    if let Some(value) = overlay.io_threads {
        base.io_threads = Some(value);
    }
}

fn perfect_square_root(value: usize) -> Option<usize> {
    let root = (value as f64).sqrt() as usize;
    if root.saturating_mul(root) == value {
        Some(root)
    } else {
        None
    }
}

fn apply_partial(config: &mut ServerConfig, partial: PartialServerConfig) {
    if let Some(tick_rate) = partial.tick_rate {
        config.tick_rate = tick_rate;
    }
    if let Some(num_player_shards) = partial.num_player_shards {
        config.num_player_shards = num_player_shards;
    }
    if let Some(world_partition_grid_dim) = partial.world_partition_grid_dim {
        config.world_partition_grid_dim = world_partition_grid_dim.max(1);
        config.num_world_partitions = config
            .world_partition_grid_dim
            .saturating_mul(config.world_partition_grid_dim);
    } else if let Some(num_world_partitions) = partial.num_world_partitions {
        config.num_world_partitions = num_world_partitions.max(1);
        if let Some(root) = perfect_square_root(config.num_world_partitions) {
            config.world_partition_grid_dim = root.max(1);
        }
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
    if let Some(value) =
        env_usize("MGS_MAX_PLAYERS_PER_MATCH").or_else(|| env_usize("MGS_MAX_PLAYERS"))
    {
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
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn with_env_lock<T>(f: impl FnOnce() -> T) -> T {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock poisoned");
        f()
    }

    fn set_env(var_name: &str, value: Option<&str>) {
        match value {
            Some(raw) => {
                // SAFETY: test-only process environment mutation under global lock.
                unsafe { std::env::set_var(var_name, raw) }
            }
            None => {
                // SAFETY: test-only process environment mutation under global lock.
                unsafe { std::env::remove_var(var_name) }
            }
        }
    }

    #[test]
    fn test_apply_partial() {
        let mut config = ServerConfig::default();
        let partial = PartialServerConfig {
            tick_rate: Some(120),
            num_player_shards: Some(8),
            world_partition_grid_dim: Some(5),
            num_world_partitions: None,
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

    #[test]
    fn env_override_accepts_legacy_max_players_name() {
        with_env_lock(|| {
            let previous_new = std::env::var("MGS_MAX_PLAYERS_PER_MATCH").ok();
            let previous_legacy = std::env::var("MGS_MAX_PLAYERS").ok();
            set_env("MGS_MAX_PLAYERS_PER_MATCH", None);
            set_env("MGS_MAX_PLAYERS", Some("321"));

            let mut config = ServerConfig::default();
            apply_env_overrides(&mut config);
            assert_eq!(config.max_players_per_match, 321);

            set_env("MGS_MAX_PLAYERS_PER_MATCH", previous_new.as_deref());
            set_env("MGS_MAX_PLAYERS", previous_legacy.as_deref());
        });
    }

    #[test]
    fn load_nested_yaml_with_includes() {
        with_env_lock(|| {
            let temp_root = std::env::temp_dir().join(format!(
                "mgs-config-loader-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&temp_root).expect("create temp config root");

            let base_path = temp_root.join("base.yaml");
            let production_path = temp_root.join("production.yaml");

            std::fs::write(
                &base_path,
                "server:\n  tick_rate: 30\n  world_partition_grid_dim: 6\n  max_players: 120\n",
            )
            .expect("write base config");
            std::fs::write(
                &production_path,
                "includes:\n  - base.yaml\nserver:\n  tick_rate: 144\n  max_players: 300\n  thread_pools:\n    physics_threads: 7\n",
            )
            .expect("write production config");

            let env_vars = [
                "MGS_CONFIG_PATH",
                "MGS_TICK_RATE",
                "MGS_PLAYER_SHARDS",
                "MGS_WORLD_GRID_DIM",
                "MGS_CLUSTER_SHARDS",
                "MGS_LOCAL_SHARD_ID",
                "MGS_MAX_PLAYERS_PER_MATCH",
                "MGS_MAX_PLAYERS",
                "MGS_THREADS_PHYSICS",
                "MGS_THREADS_NETWORK",
                "MGS_THREADS_GAME",
                "MGS_THREADS_AI",
                "MGS_THREADS_IO",
            ];

            let previous_env: Vec<(&str, Option<String>)> = env_vars
                .iter()
                .map(|name| (*name, std::env::var(name).ok()))
                .collect();

            for name in env_vars {
                set_env(name, None);
            }
            set_env(
                "MGS_CONFIG_PATH",
                Some(production_path.to_string_lossy().as_ref()),
            );

            let config = load_server_config_from_env_and_file().expect("load config with includes");
            assert_eq!(config.tick_rate, 144);
            assert_eq!(config.world_partition_grid_dim, 6);
            assert_eq!(config.num_world_partitions, 36);
            assert_eq!(config.max_players_per_match, 300);
            assert_eq!(config.thread_pools.physics_threads, 7);

            for (name, value) in previous_env {
                set_env(name, value.as_deref());
            }

            let _ = std::fs::remove_dir_all(&temp_root);
        });
    }
}
