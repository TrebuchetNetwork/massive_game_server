// massive_game_server/server/src/memory/pools.rs

use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Default maximum pool size when none is specified.
const DEFAULT_MAX_POOL_SIZE: usize = 4096;

/// Number of shards used to reduce mutex contention.
/// Each shard holds an independent free-list protected by its own mutex.
const POOL_SHARD_COUNT: usize = 4;

struct Shard<T> {
    free: Mutex<Vec<T>>,
}

/// A sharded object pool with configurable maximum capacity.
///
/// Objects are distributed across `POOL_SHARD_COUNT` shards, each with its
/// own mutex. This reduces contention when multiple threads acquire/release
/// concurrently. When releasing objects, if the total free count has reached
/// `max_pool_size` the object is simply dropped instead of being returned
/// to the pool.
pub struct ObjectPool<T> {
    shards: Vec<Shard<T>>,
    in_use: AtomicUsize,
    total_free: AtomicUsize,
    factory: Arc<dyn Fn() -> T + Send + Sync>,
    max_pool_size: usize,
}

impl<T> ObjectPool<T> {
    pub fn new<F>(initial_capacity: usize, factory: F) -> Self
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        Self::with_max_size(initial_capacity, DEFAULT_MAX_POOL_SIZE, factory)
    }

    pub fn with_max_size<F>(initial_capacity: usize, max_pool_size: usize, factory: F) -> Self
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        let factory_arc: Arc<dyn Fn() -> T + Send + Sync> = Arc::new(factory);
        let per_shard = initial_capacity / POOL_SHARD_COUNT;
        let remainder = initial_capacity % POOL_SHARD_COUNT;

        let mut shards = Vec::with_capacity(POOL_SHARD_COUNT);
        let mut actual_initial_capacity = 0;
        for i in 0..POOL_SHARD_COUNT {
            let count = per_shard + if i < remainder { 1 } else { 0 };
            actual_initial_capacity += count;
            let mut free = Vec::with_capacity(count);
            for _ in 0..count {
                free.push(factory_arc());
            }
            shards.push(Shard {
                free: Mutex::new(free),
            });
        }

        Self {
            shards,
            in_use: AtomicUsize::new(0),
            total_free: AtomicUsize::new(actual_initial_capacity),
            factory: factory_arc,
            max_pool_size: max_pool_size.max(1),
        }
    }

    /// Pick a shard based on the current thread. This avoids needing a
    /// thread-local counter; the pointer-derived index is good enough for
    /// distributing load and doesn't need to be perfectly uniform.
    fn shard_index(&self) -> usize {
        // Use the address of a stack variable as a cheap thread-discriminator.
        let stack_local: u8 = 0;
        let addr = &stack_local as *const u8 as usize;
        (addr >> 6) % self.shards.len()
    }

    pub fn acquire(&self) -> T {
        let start = self.shard_index();
        // Try the preferred shard first, then probe others.
        for offset in 0..self.shards.len() {
            let idx = (start + offset) % self.shards.len();
            let cached = {
                let mut free = self.shards[idx].free.lock();
                free.pop()
            };
            if let Some(value) = cached {
                self.in_use.fetch_add(1, Ordering::Relaxed);
                self.total_free.fetch_sub(1, Ordering::Relaxed);
                return value;
            }
        }
        // All shards empty — create a new object outside any lock.
        // If the factory panics, the counter stays consistent and no Mutex
        // is held across the unwinding call.
        let value = (self.factory)();
        self.in_use.fetch_add(1, Ordering::Relaxed);
        value
    }

    pub fn release(&self, value: T) {
        self.in_use.fetch_sub(1, Ordering::Relaxed);

        let mut current_free = self.total_free.load(Ordering::Relaxed);
        loop {
            if current_free >= self.max_pool_size {
                drop(value);
                return;
            }
            match self.total_free.compare_exchange_weak(
                current_free,
                current_free + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_free = actual,
            }
        }

        let idx = self.shard_index();
        let mut free = self.shards[idx].free.lock();
        free.push(value);
    }

    pub fn in_use_count(&self) -> usize {
        self.in_use.load(Ordering::Relaxed)
    }

    pub fn free_count(&self) -> usize {
        self.total_free.load(Ordering::Relaxed)
    }

    pub fn max_pool_size(&self) -> usize {
        self.max_pool_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::AtomicU32;

    #[test]
    fn pool_tracks_usage() {
        let pool = ObjectPool::new(1, || 7_u32);
        let value = pool.acquire();
        assert_eq!(value, 7);
        assert_eq!(pool.in_use_count(), 1);
        pool.release(value);
        assert_eq!(pool.in_use_count(), 0);
        assert!(pool.free_count() >= 1);
    }

    #[test]
    fn factory_panic_does_not_increment_in_use_count() {
        let pool = ObjectPool::new(0, || -> u32 { panic!("factory failure") });

        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = pool.acquire();
        }));

        assert!(result.is_err());
        assert_eq!(pool.in_use_count(), 0);
        assert_eq!(pool.free_count(), 0);
    }

    #[test]
    fn pool_respects_max_size() {
        // Pool with max size of 2.
        let pool = ObjectPool::with_max_size(0, 2, || 0_u32);

        // Acquire 5 objects (all created by factory since pool is empty).
        let mut objects = Vec::new();
        for _ in 0..5 {
            objects.push(pool.acquire());
        }
        assert_eq!(pool.in_use_count(), 5);
        assert_eq!(pool.free_count(), 0);

        // Release all 5 — only 2 should be retained in the pool.
        for obj in objects {
            pool.release(obj);
        }
        assert_eq!(pool.in_use_count(), 0);
        // At most max_pool_size objects retained.
        assert!(pool.free_count() <= 2);
    }

    #[test]
    fn pool_max_size_bounds_growth() {
        let create_count = Arc::new(AtomicU32::new(0));
        let cc = create_count.clone();
        let pool = ObjectPool::with_max_size(0, 3, move || {
            cc.fetch_add(1, Ordering::Relaxed);
            42_u32
        });

        // Repeatedly acquire and release — pool should never hold more than 3 free objects.
        for _ in 0..20 {
            let v = pool.acquire();
            pool.release(v);
        }
        assert!(pool.free_count() <= 3);
    }

    #[test]
    fn sharded_pool_distributes_initial_capacity() {
        let pool = ObjectPool::with_max_size(8, 100, || 1_u32);
        assert_eq!(pool.free_count(), 8);
    }
}
