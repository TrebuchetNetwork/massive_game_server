// massive_game_server/server/src/core/config.rs
// Basic configuration structure
// Removed unused: use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ThreadPoolConfig {
    pub physics_threads: usize,
    pub networking_threads: usize,
    pub game_logic_threads: usize,
    pub ai_threads: usize,
    pub io_threads: usize,
}

fn allocate_weighted_threads(
    total_budget: usize,
    minimums: [usize; 5],
    weights: [f64; 5],
) -> [usize; 5] {
    let mut counts = minimums;
    let minimum_total: usize = minimums.iter().sum();
    if total_budget <= minimum_total {
        return counts;
    }

    let remaining = total_budget - minimum_total;
    let weight_sum: f64 = weights.iter().map(|weight| weight.max(0.0)).sum();
    if weight_sum <= f64::EPSILON {
        for idx in 0..remaining {
            counts[idx % counts.len()] += 1;
        }
        return counts;
    }

    let mut distributed = 0usize;
    let mut remainders: Vec<(usize, f64)> = Vec::with_capacity(counts.len());
    for (idx, weight) in weights.iter().enumerate() {
        let exact = remaining as f64 * (*weight / weight_sum);
        let floor = exact.floor() as usize;
        counts[idx] += floor;
        distributed += floor;
        remainders.push((idx, exact - floor as f64));
    }

    let mut leftover = remaining.saturating_sub(distributed);
    remainders.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    let mut round_robin_idx = 0usize;
    while leftover > 0 {
        let target_pool = remainders[round_robin_idx % remainders.len()].0;
        counts[target_pool] += 1;
        leftover -= 1;
        round_robin_idx = round_robin_idx.saturating_add(1);
    }

    counts
}

fn thread_pool_config_for_cores(cores: usize) -> ThreadPoolConfig {
    if cores <= 4 {
        ThreadPoolConfig {
            physics_threads: 1,
            networking_threads: 1,
            game_logic_threads: 1,
            ai_threads: 1,
            io_threads: 1,
        }
    } else if cores <= 8 {
        ThreadPoolConfig {
            physics_threads: 2,
            networking_threads: 2,
            game_logic_threads: 2,
            ai_threads: 1,
            io_threads: 1,
        }
    } else {
        // Keep worker totals bounded on high-core hosts to avoid context-switch overhead.
        let total_budget = cores.saturating_sub(2).clamp(10, 24).min(cores);
        let minimums = if cores >= 10 {
            [2, 2, 2, 2, 2]
        } else {
            [2, 2, 2, 1, 1]
        };
        let [physics_threads, networking_threads, game_logic_threads, ai_threads, io_threads] =
            allocate_weighted_threads(total_budget, minimums, [0.24, 0.18, 0.24, 0.20, 0.14]);
        ThreadPoolConfig {
            physics_threads,
            networking_threads,
            game_logic_threads,
            ai_threads,
            io_threads,
        }
    }
}

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4); // fallback
        thread_pool_config_for_cores(cores)
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub tick_rate: u64,
    pub num_player_shards: usize,
    pub num_world_partitions: usize,
    pub world_partition_grid_dim: usize,
    pub thread_pools: ThreadPoolConfig,
    pub cluster_shard_count: usize,
    pub local_shard_id: usize,
    pub max_players_per_match: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            tick_rate: super::constants::SERVER_TICK_RATE,
            num_player_shards: super::constants::PLAYER_SHARDS_COUNT,
            num_world_partitions: super::constants::PARTITION_GRID_SIZE
                * super::constants::PARTITION_GRID_SIZE,
            world_partition_grid_dim: super::constants::PARTITION_GRID_SIZE,
            thread_pools: ThreadPoolConfig::default(),
            cluster_shard_count: 1,
            local_shard_id: 0,
            max_players_per_match: 400,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoreAllocation {
    pub physics_cores_indices: Vec<usize>,
    pub networking_cores_indices: Vec<usize>,
    pub game_logic_cores_indices: Vec<usize>,
    pub ai_cores_indices: Vec<usize>,
    pub io_cores_indices: Vec<usize>,
}

impl CoreAllocation {
    pub fn new(config: &ThreadPoolConfig) -> Self {
        let mut current_core = 0;
        let mut physics_cores_indices = Vec::new();
        for _ in 0..config.physics_threads {
            physics_cores_indices.push(current_core);
            current_core += 1;
        }

        let mut networking_cores_indices = Vec::new();
        for _ in 0..config.networking_threads {
            networking_cores_indices.push(current_core);
            current_core += 1;
        }

        let mut game_logic_cores_indices = Vec::new();
        for _ in 0..config.game_logic_threads {
            game_logic_cores_indices.push(current_core);
            current_core += 1;
        }

        let mut ai_cores_indices = Vec::new();
        for _ in 0..config.ai_threads {
            ai_cores_indices.push(current_core);
            current_core += 1;
        }

        let mut io_cores_indices = Vec::new();
        for _ in 0..config.io_threads {
            io_cores_indices.push(current_core);
            current_core += 1;
        }

        CoreAllocation {
            physics_cores_indices,
            networking_cores_indices,
            game_logic_cores_indices,
            ai_cores_indices,
            io_cores_indices,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_pool_config_low_core_defaults() {
        let cfg = thread_pool_config_for_cores(4);
        assert_eq!(cfg.physics_threads, 1);
        assert_eq!(cfg.networking_threads, 1);
        assert_eq!(cfg.game_logic_threads, 1);
        assert_eq!(cfg.ai_threads, 1);
        assert_eq!(cfg.io_threads, 1);
    }

    #[test]
    fn thread_pool_config_mid_core_defaults() {
        let cfg = thread_pool_config_for_cores(8);
        assert_eq!(cfg.physics_threads, 2);
        assert_eq!(cfg.networking_threads, 2);
        assert_eq!(cfg.game_logic_threads, 2);
        assert_eq!(cfg.ai_threads, 1);
        assert_eq!(cfg.io_threads, 1);
    }

    #[test]
    fn thread_pool_config_high_core_is_budgeted() {
        let cfg = thread_pool_config_for_cores(32);
        let total_threads = cfg.physics_threads
            + cfg.networking_threads
            + cfg.game_logic_threads
            + cfg.ai_threads
            + cfg.io_threads;
        assert!(
            total_threads <= 24,
            "high-core default should cap total worker threads"
        );
        assert!(
            cfg.physics_threads >= 2
                && cfg.networking_threads >= 2
                && cfg.game_logic_threads >= 2
                && cfg.ai_threads >= 2
                && cfg.io_threads >= 2
        );
    }
}
