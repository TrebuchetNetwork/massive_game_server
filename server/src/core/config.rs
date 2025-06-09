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

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        ThreadPoolConfig {
            physics_threads: 4,
            networking_threads: 6,
            game_logic_threads: 12,
            ai_threads: 8,
            io_threads: 8,
        }
    }
}


#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub tick_rate: u64,
    pub num_player_shards: usize,
    pub num_world_partitions: usize, 
    pub world_partition_grid_dim: usize, 
    pub thread_pools: ThreadPoolConfig,
    pub max_players_per_match: usize, // <<< ADD THIS LINE
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            tick_rate: super::constants::SERVER_TICK_RATE,
            num_player_shards: 12,// Match core count for better distribution super::constants::PLAYER_SHARDS_COUNT,
            num_world_partitions: super::constants::PARTITION_GRID_SIZE * super::constants::PARTITION_GRID_SIZE,
            world_partition_grid_dim: super::constants::PARTITION_GRID_SIZE,
            thread_pools: ThreadPoolConfig::default(),
            max_players_per_match: 140, // <<< ADD THIS LINE (or your desired default)
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
        for _ in 0..config.physics_threads { physics_cores_indices.push(current_core); current_core += 1; }

        let mut networking_cores_indices = Vec::new();
        for _ in 0..config.networking_threads { networking_cores_indices.push(current_core); current_core += 1; }

        let mut game_logic_cores_indices = Vec::new();
        for _ in 0..config.game_logic_threads { game_logic_cores_indices.push(current_core); current_core += 1; }

        let mut ai_cores_indices = Vec::new();
        for _ in 0..config.ai_threads { ai_cores_indices.push(current_core); current_core += 1; }

        let mut io_cores_indices = Vec::new();
        for _ in 0..config.io_threads { io_cores_indices.push(current_core); current_core += 1; }

        CoreAllocation {
            physics_cores_indices,
            networking_cores_indices,
            game_logic_cores_indices,
            ai_cores_indices,
            io_cores_indices,
        }
    }
}
