// massive_game_server/server/src/memory/pools.rs

use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct ObjectPool<T> {
    free: Mutex<Vec<T>>,
    in_use: AtomicUsize,
    factory: Arc<dyn Fn() -> T + Send + Sync>,
}

impl<T> ObjectPool<T> {
    pub fn new<F>(initial_capacity: usize, factory: F) -> Self
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        let factory_arc: Arc<dyn Fn() -> T + Send + Sync> = Arc::new(factory);
        let mut free = Vec::with_capacity(initial_capacity);
        for _ in 0..initial_capacity {
            free.push(factory_arc());
        }
        Self {
            free: Mutex::new(free),
            in_use: AtomicUsize::new(0),
            factory: factory_arc,
        }
    }

    pub fn acquire(&self) -> T {
        let mut free = self.free.lock();
        self.in_use.fetch_add(1, Ordering::Relaxed);
        free.pop().unwrap_or_else(|| (self.factory)())
    }

    pub fn release(&self, value: T) {
        let mut free = self.free.lock();
        free.push(value);
        self.in_use.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn in_use_count(&self) -> usize {
        self.in_use.load(Ordering::Relaxed)
    }

    pub fn free_count(&self) -> usize {
        self.free.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_tracks_usage() {
        let pool = ObjectPool::new(1, || 7_u32);
        let value = pool.acquire();
        assert_eq!(value, 7);
        assert_eq!(pool.in_use_count(), 1);
        pool.release(value);
        assert_eq!(pool.in_use_count(), 0);
        assert_eq!(pool.free_count(), 1);
    }
}
