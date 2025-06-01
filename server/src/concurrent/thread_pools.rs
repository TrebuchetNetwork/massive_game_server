// massive_game_server/server/src/concurrent/thread_pools.rs
use crate::core::config::{ServerConfig, CoreAllocation};
use crate::core::error::{ServerError, ServerResult};
use rayon::{ThreadPool, ThreadPoolBuilder};
use core_affinity::CoreId;
use std::sync::Arc;
use tracing::{info, error, warn};

pub struct ThreadPoolSystem {
    pub physics_pool: Arc<ThreadPool>,
    pub network_pool: Arc<ThreadPool>,
    pub game_logic_pool: Arc<ThreadPool>,
    pub ai_pool: Arc<ThreadPool>,
    pub io_pool: Arc<ThreadPool>,
}

impl ThreadPoolSystem {
    /*pub fn new_old(config: Arc<ServerConfig>) -> ServerResult<Self> {
        let core_alloc = CoreAllocation::new(&config.thread_pools);
        let all_core_ids_arc: Arc<Option<Vec<CoreId>>> = Arc::new(core_affinity::get_core_ids());


        if all_core_ids_arc.is_none() {
            warn!("Could not get core IDs. Core affinity will not be applied.");
        }
        // Correctly access the length of the Vec<CoreId> inside the Option inside the Arc
        let available_cores = all_core_ids_arc.as_ref().as_ref().map_or(0, |ids_vec| ids_vec.len());

        let total_requested_cores = config.thread_pools.physics_threads +
                                    config.thread_pools.networking_threads +
                                    config.thread_pools.game_logic_threads +
                                    config.thread_pools.ai_threads +
                                    config.thread_pools.io_threads;

        if available_cores > 0 && total_requested_cores > available_cores {
            warn!(
                "Requested {} total cores for thread pools, but only {} cores are available. Performance may be impacted.",
                total_requested_cores, available_cores
            );
        }

        let physics_pool = Self::create_pool(
            "physics",
            config.thread_pools.physics_threads,
            core_alloc.physics_cores_indices.clone(),
            all_core_ids_arc.clone(),
        )?;
        let network_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(config.thread_pools.networking_threads)
            .build()?;
        let game_logic_pool = Self::create_pool(
            "game_logic",
            config.thread_pools.game_logic_threads,
            core_alloc.game_logic_cores_indices.clone(),
            all_core_ids_arc.clone(),
        )?;
        let ai_pool = Self::create_pool(
            "ai",
             config.thread_pools.ai_threads,
            core_alloc.ai_cores_indices.clone(),
            all_core_ids_arc.clone(),
        )?;
        let io_pool = Self::create_pool(
            "io",
            config.thread_pools.io_threads,
            core_alloc.io_cores_indices.clone(),
            all_core_ids_arc, // Last one can move the Arc
        )?;

        Ok(ThreadPoolSystem {
            physics_pool: Arc::new(physics_pool),
            network_pool: Arc::new(network_pool),
            game_logic_pool: Arc::new(game_logic_pool),
            ai_pool: Arc::new(ai_pool),
            io_pool: Arc::new(io_pool),
        })
    }*/

    pub fn new(config: Arc<ServerConfig>) -> Result<Self, anyhow::Error> {
        let network_pool = ThreadPoolBuilder::new()
            .num_threads(config.thread_pools.networking_threads)
            .build()?;
        let ai_pool = ThreadPoolBuilder::new()
            .num_threads(config.thread_pools.ai_threads)
            .build()?;
        let physics_pool = ThreadPoolBuilder::new()
            .num_threads(config.thread_pools.physics_threads)
            .build()?;
        let game_logic_pool = ThreadPoolBuilder::new()
            .num_threads(config.thread_pools.game_logic_threads)
            .build()?;
        let io_pool = ThreadPoolBuilder::new()
            .num_threads(config.thread_pools.io_threads)
            .build()?;

        Ok(Self {
            network_pool: Arc::new(network_pool),
            ai_pool: Arc::new(ai_pool),
            physics_pool: Arc::new(physics_pool),
            game_logic_pool: Arc::new(game_logic_pool),
            io_pool: Arc::new(io_pool),
        })
    }

    fn create_pool(
        name_str: &str,
        num_threads: usize,
        core_indices_to_use: Vec<usize>,
        all_available_core_ids_arc: Arc<Option<Vec<CoreId>>>,
    ) -> ServerResult<ThreadPool> {
        let pool_identity_name_default = name_str.to_string();

        if num_threads == 0 {
            warn!("Thread pool '{}' configured with 0 threads. Creating a minimal pool.", pool_identity_name_default);
             return ThreadPoolBuilder::new()
                .num_threads(1)
                .thread_name(move |i| format!("{}-default-{}", pool_identity_name_default, i))
                .build()
                .map_err(|e| ServerError::ThreadingError(format!("Failed to build default {} pool: {}", name_str, e)));
        }

        let name_for_thread_name = name_str.to_string();
        let name_for_start_handler = name_str.to_string();


        ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .thread_name(move |i| format!("{}-{}", name_for_thread_name, i))
            .start_handler(move |thread_idx_in_pool| {
                // Correctly dereference Arc then Option to get &Vec<CoreId>
                if let Some(available_core_ids_vec) = all_available_core_ids_arc.as_ref().as_ref() {
                    if let Some(global_core_idx_ptr) = core_indices_to_use.get(thread_idx_in_pool) {
                        let global_core_idx = *global_core_idx_ptr; // Dereference to get usize
                        if let Some(core_id_to_pin) = available_core_ids_vec.get(global_core_idx) {
                            if core_affinity::set_for_current(*core_id_to_pin) {
                                info!(
                                    "Pinned thread {}-{} to core ID {:?} (Global Index {})",
                                    name_for_start_handler, thread_idx_in_pool, core_id_to_pin.id, global_core_idx
                                );
                            } else {
                                error!(
                                    "Failed to pin thread {}-{} to core ID {:?} (Global Index {})",
                                    name_for_start_handler, thread_idx_in_pool, core_id_to_pin.id, global_core_idx
                                );
                            }
                        } else {
                             warn!(
                                "Global core index {} (for pool {}, thread {}) is out of bounds for available cores ({}). No affinity set.",
                                global_core_idx, name_for_start_handler, thread_idx_in_pool, available_core_ids_vec.len()
                            );
                        }
                    } else {
                        warn!(
                            "Thread {}-{} has no specific core assignment (pool size: {}, assigned cores: {}). No affinity set.",
                            name_for_start_handler, thread_idx_in_pool, num_threads, core_indices_to_use.len()
                        );
                    }
                } else {
                     warn!("Core IDs vector is None inside Arc for pool {}. No affinity set.", name_for_start_handler);
                }
            })
            .build()
            .map_err(|e| ServerError::ThreadingError(format!("Failed to build {} pool: {}", name_str, e)))
    }
}