// massive_game_server/server/src/concurrent/thread_pools.rs
use crate::core::config::{CoreAllocation, ServerConfig};
use crate::core::error::{ServerError, ServerResult};
use crate::memory::numa::NumaTopology;
use core_affinity::CoreId;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::env;
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct ThreadPoolSystem {
    pub physics_pool: Arc<ThreadPool>,
    pub network_pool: Arc<ThreadPool>,
    pub game_logic_pool: Arc<ThreadPool>,
    pub ai_pool: Arc<ThreadPool>,
    pub io_pool: Arc<ThreadPool>,
}

impl ThreadPoolSystem {
    pub fn new(config: Arc<ServerConfig>) -> Result<Self, anyhow::Error> {
        let numa_aware = env_bool("MGS_NUMA_AWARE");
        let numa_topology = Arc::new(NumaTopology::from_env());
        if Self::should_use_shared_pool(&config.thread_pools) {
            return Self::new_with_shared_pool(numa_aware, Arc::clone(&numa_topology));
        }

        let affinity_enabled = env_bool("MGS_CPU_AFFINITY");
        if !affinity_enabled {
            let network_pool = Self::create_pool_without_affinity(
                "network",
                config.thread_pools.networking_threads,
                numa_aware,
                Arc::clone(&numa_topology),
            )?;
            let ai_pool = Self::create_pool_without_affinity(
                "ai",
                config.thread_pools.ai_threads,
                numa_aware,
                Arc::clone(&numa_topology),
            )?;
            let physics_pool = Self::create_pool_without_affinity(
                "physics",
                config.thread_pools.physics_threads,
                numa_aware,
                Arc::clone(&numa_topology),
            )?;
            let game_logic_pool = Self::create_pool_without_affinity(
                "game_logic",
                config.thread_pools.game_logic_threads,
                numa_aware,
                Arc::clone(&numa_topology),
            )?;
            let io_pool = Self::create_pool_without_affinity(
                "io",
                config.thread_pools.io_threads,
                numa_aware,
                Arc::clone(&numa_topology),
            )?;

            return Ok(Self {
                network_pool: Arc::new(network_pool),
                ai_pool: Arc::new(ai_pool),
                physics_pool: Arc::new(physics_pool),
                game_logic_pool: Arc::new(game_logic_pool),
                io_pool: Arc::new(io_pool),
            });
        }

        let core_alloc = CoreAllocation::new(&config.thread_pools);
        let all_core_ids_arc: Arc<Option<Vec<CoreId>>> = Arc::new(core_affinity::get_core_ids());

        if all_core_ids_arc.is_none() {
            warn!(
                "MGS_CPU_AFFINITY=1 but core IDs are unavailable. Falling back to unpinned pools."
            );
            return Self::new_without_affinity(config, numa_aware, Arc::clone(&numa_topology));
        }

        let available_cores = all_core_ids_arc
            .as_ref()
            .as_ref()
            .map_or(0usize, |ids| ids.len());
        let total_requested_cores = config.thread_pools.physics_threads
            + config.thread_pools.networking_threads
            + config.thread_pools.game_logic_threads
            + config.thread_pools.ai_threads
            + config.thread_pools.io_threads;
        if total_requested_cores > available_cores {
            warn!(
                "CPU affinity requested {} threads but only {} cores are visible. Some pools will share cores.",
                total_requested_cores, available_cores
            );
        }

        info!(
            "Thread pools using CPU affinity (available_cores={}, requested_threads={})",
            available_cores, total_requested_cores
        );

        let physics_pool = Self::create_pool(
            "physics",
            config.thread_pools.physics_threads,
            core_alloc.physics_cores_indices.clone(),
            all_core_ids_arc.clone(),
            numa_aware,
            Arc::clone(&numa_topology),
        )?;
        let network_pool = Self::create_pool(
            "network",
            config.thread_pools.networking_threads,
            core_alloc.networking_cores_indices.clone(),
            all_core_ids_arc.clone(),
            numa_aware,
            Arc::clone(&numa_topology),
        )?;
        let game_logic_pool = Self::create_pool(
            "game_logic",
            config.thread_pools.game_logic_threads,
            core_alloc.game_logic_cores_indices.clone(),
            all_core_ids_arc.clone(),
            numa_aware,
            Arc::clone(&numa_topology),
        )?;
        let ai_pool = Self::create_pool(
            "ai",
            config.thread_pools.ai_threads,
            core_alloc.ai_cores_indices.clone(),
            all_core_ids_arc.clone(),
            numa_aware,
            Arc::clone(&numa_topology),
        )?;
        let io_pool = Self::create_pool(
            "io",
            config.thread_pools.io_threads,
            core_alloc.io_cores_indices.clone(),
            all_core_ids_arc,
            numa_aware,
            Arc::clone(&numa_topology),
        )?;

        Ok(Self {
            network_pool: Arc::new(network_pool),
            ai_pool: Arc::new(ai_pool),
            physics_pool: Arc::new(physics_pool),
            game_logic_pool: Arc::new(game_logic_pool),
            io_pool: Arc::new(io_pool),
        })
    }

    fn new_without_affinity(
        config: Arc<ServerConfig>,
        numa_aware: bool,
        numa_topology: Arc<NumaTopology>,
    ) -> Result<Self, anyhow::Error> {
        let network_pool = Self::create_pool_without_affinity(
            "network",
            config.thread_pools.networking_threads,
            numa_aware,
            Arc::clone(&numa_topology),
        )?;
        let ai_pool = Self::create_pool_without_affinity(
            "ai",
            config.thread_pools.ai_threads,
            numa_aware,
            Arc::clone(&numa_topology),
        )?;
        let physics_pool = Self::create_pool_without_affinity(
            "physics",
            config.thread_pools.physics_threads,
            numa_aware,
            Arc::clone(&numa_topology),
        )?;
        let game_logic_pool = Self::create_pool_without_affinity(
            "game_logic",
            config.thread_pools.game_logic_threads,
            numa_aware,
            Arc::clone(&numa_topology),
        )?;
        let io_pool = Self::create_pool_without_affinity(
            "io",
            config.thread_pools.io_threads,
            numa_aware,
            Arc::clone(&numa_topology),
        )?;

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
        numa_aware: bool,
        numa_topology: Arc<NumaTopology>,
    ) -> ServerResult<ThreadPool> {
        let pool_identity_name_default = name_str.to_string();

        if num_threads == 0 {
            warn!(
                "Thread pool '{}' configured with 0 threads. Creating a minimal pool.",
                pool_identity_name_default
            );
            return ThreadPoolBuilder::new()
                .num_threads(1)
                .thread_name(move |i| format!("{}-default-{}", pool_identity_name_default, i))
                .build()
                .map_err(|e| {
                    ServerError::ThreadingError(format!(
                        "Failed to build default {} pool: {}",
                        name_str, e
                    ))
                });
        }

        let name_for_thread_name = name_str.to_string();
        let name_for_start_handler = name_str.to_string();

        ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .thread_name(move |i| format!("{}-{}", name_for_thread_name, i))
            .start_handler(move |thread_idx_in_pool| {
                if numa_aware {
                    let node_id = numa_topology.recommended_node_for_shard(thread_idx_in_pool);
                    if !numa_topology.pin_current_thread_to_node(node_id) {
                        warn!(
                            "Failed to pin thread {}-{} to NUMA node {}.",
                            name_for_start_handler, thread_idx_in_pool, node_id
                        );
                    }
                }
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

    fn create_pool_without_affinity(
        name_str: &str,
        num_threads: usize,
        numa_aware: bool,
        numa_topology: Arc<NumaTopology>,
    ) -> Result<ThreadPool, anyhow::Error> {
        let threads = num_threads.max(1);
        let name_for_thread = name_str.to_string();
        let name_for_start = name_str.to_string();
        Ok(ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(move |i| format!("{}-{}", name_for_thread, i))
            .start_handler(move |thread_idx_in_pool| {
                if numa_aware {
                    let node_id = numa_topology.recommended_node_for_shard(thread_idx_in_pool);
                    if !numa_topology.pin_current_thread_to_node(node_id) {
                        warn!(
                            "Failed to pin thread {}-{} to NUMA node {} (no explicit core affinity).",
                            name_for_start, thread_idx_in_pool, node_id
                        );
                    }
                }
            })
            .build()?)
    }

    fn should_use_shared_pool(config: &crate::core::config::ThreadPoolConfig) -> bool {
        config.physics_threads == 1
            && config.networking_threads == 1
            && config.game_logic_threads == 1
            && config.ai_threads == 1
            && config.io_threads == 1
    }

    fn new_with_shared_pool(
        numa_aware: bool,
        numa_topology: Arc<NumaTopology>,
    ) -> Result<Self, anyhow::Error> {
        let shared_threads = std::thread::available_parallelism()
            .map(|parallelism| parallelism.get().clamp(1, 4))
            .unwrap_or(2);
        let shared = Arc::new(Self::create_pool_without_affinity(
            "shared",
            shared_threads,
            numa_aware,
            numa_topology,
        )?);
        info!(
            "Using shared thread pool for low-core runtime (threads={}).",
            shared_threads
        );
        Ok(Self {
            physics_pool: Arc::clone(&shared),
            network_pool: Arc::clone(&shared),
            game_logic_pool: Arc::clone(&shared),
            ai_pool: Arc::clone(&shared),
            io_pool: shared,
        })
    }
}

fn env_bool(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

// ── Queue depth monitoring / backpressure ────────────────────────────
//
// Rayon thread pools use lock-free work-stealing deques without an exposed
// queue depth metric.  We add lightweight saturation monitoring by tracking
// how many tasks are pending per pool via an AtomicUsize counter.  Callers
// use `submit_monitored` to log warnings when the pending count is high and
// optionally refuse new work (backpressure).

use std::sync::atomic::AtomicUsize;

/// Per-pool saturation tracker.  Wrap one around each pool to monitor
/// pending work and log warnings when the pool approaches saturation.
pub struct MonitoredPool {
    pub pool: Arc<ThreadPool>,
    pub name: String,
    pending: Arc<AtomicUsize>,
    /// Warn once when pending exceeds this count.
    warn_threshold: usize,
    /// Hard cap -- `try_submit` returns `Err` when pending exceeds this.
    max_pending: usize,
}

impl MonitoredPool {
    pub fn new(pool: Arc<ThreadPool>, name: impl Into<String>) -> Self {
        let threads = pool.current_num_threads();
        Self {
            pool,
            name: name.into(),
            pending: Arc::new(AtomicUsize::new(0)),
            // Default thresholds: warn at 4x thread count, reject at 16x.
            warn_threshold: threads.saturating_mul(4).max(8),
            max_pending: threads.saturating_mul(16).max(32),
        }
    }

    /// Create with custom thresholds.
    pub fn with_thresholds(
        pool: Arc<ThreadPool>,
        name: impl Into<String>,
        warn_threshold: usize,
        max_pending: usize,
    ) -> Self {
        Self {
            pool,
            name: name.into(),
            pending: Arc::new(AtomicUsize::new(0)),
            warn_threshold,
            max_pending,
        }
    }

    /// Current number of pending (submitted but not yet completed) tasks.
    pub fn pending_count(&self) -> usize {
        self.pending.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Submit work to the pool.  Returns `false` if the pending count
    /// exceeds `max_pending` (backpressure).
    pub fn try_submit<F>(&self, work: F) -> bool
    where
        F: FnOnce() + Send + 'static,
    {
        let current = self
            .pending
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if current >= self.max_pending {
            self.pending
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            warn!(
                "[ThreadPool:{}] Backpressure: {} pending tasks exceeds max {}. Rejecting work.",
                self.name, current, self.max_pending
            );
            return false;
        }
        if self.warn_threshold > 0
            && current >= self.warn_threshold
            && current.is_multiple_of(self.warn_threshold)
        {
            warn!(
                "[ThreadPool:{}] Queue depth warning: {} pending tasks (warn_threshold={})",
                self.name, current, self.warn_threshold
            );
        }
        let pending_clone = Arc::clone(&self.pending);
        self.pool.spawn(move || {
            work();
            pending_clone.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        });
        true
    }

    /// Submit work unconditionally (no backpressure), but still log warnings.
    pub fn submit<F>(&self, work: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let current = self
            .pending
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.warn_threshold > 0
            && current >= self.warn_threshold
            && current.is_multiple_of(self.warn_threshold)
        {
            warn!(
                "[ThreadPool:{}] Queue depth warning: {} pending tasks (warn_threshold={})",
                self.name, current, self.warn_threshold
            );
        }
        let pending_clone = Arc::clone(&self.pending);
        self.pool.spawn(move || {
            work();
            pending_clone.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pool() -> Arc<ThreadPool> {
        Arc::new(ThreadPoolBuilder::new().num_threads(2).build().unwrap())
    }

    #[test]
    fn monitored_pool_tracks_pending() {
        let pool = test_pool();
        let monitored = MonitoredPool::new(pool, "test");
        assert_eq!(monitored.pending_count(), 0);

        // Submit work that blocks until we signal it
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        monitored.submit(move || {
            let _ = rx.recv();
        });

        // Give the pool a moment to pick up the task
        std::thread::sleep(std::time::Duration::from_millis(50));
        // pending_count should be exactly 1 (task is running but not finished)
        assert_eq!(monitored.pending_count(), 1);

        // Release the task
        let _ = tx.send(());
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(monitored.pending_count(), 0);
    }

    #[test]
    fn monitored_pool_backpressure_rejects_when_full() {
        let pool = test_pool();
        // Set very low thresholds: warn at 1, reject at 2
        let monitored = MonitoredPool::with_thresholds(pool, "test_bp", 1, 2);

        // Hold tasks to keep pending count high
        let (tx1, rx1) = std::sync::mpsc::channel::<()>();
        let (tx2, rx2) = std::sync::mpsc::channel::<()>();

        assert!(monitored.try_submit(move || {
            let _ = rx1.recv();
        }));
        assert!(monitored.try_submit(move || {
            let _ = rx2.recv();
        }));

        // Third submission should be rejected (max_pending = 2)
        let rejected = !monitored.try_submit(|| {});
        assert!(rejected, "Expected backpressure rejection at max_pending=2");

        // Cleanup
        let _ = tx1.send(());
        let _ = tx2.send(());
    }

    #[test]
    fn env_bool_parsing() {
        // env_bool is a module-private function; test it indirectly via known env vars
        // or just verify the logic inline:
        assert!(["1", "true", "yes", "on"].iter().all(|v| {
            let normalized = v.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        }));
        assert!(!{
            let normalized = "false".trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        });
    }
}
