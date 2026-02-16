// massive_game_server/server/src/network/rate_limiter.rs

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct TokenBucket {
    refill_per_sec: f64,
    capacity: f64,
    available_tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(refill_per_sec: u32, burst_capacity: u32) -> Self {
        let refill = refill_per_sec.max(1) as f64;
        let cap = burst_capacity.max(1) as f64;
        Self {
            refill_per_sec: refill,
            capacity: cap,
            available_tokens: cap,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now
            .checked_duration_since(self.last_refill)
            .unwrap_or(Duration::from_millis(0))
            .as_secs_f64();
        if elapsed > 0.0 {
            self.available_tokens =
                (self.available_tokens + elapsed * self.refill_per_sec).min(self.capacity);
            self.last_refill = now;
        }
    }

    pub fn try_acquire(&mut self) -> bool {
        self.try_acquire_n(1)
    }

    pub fn try_acquire_n(&mut self, cost: u32) -> bool {
        self.refill();
        let needed = cost.max(1) as f64;
        if self.available_tokens >= needed {
            self.available_tokens -= needed;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_enforces_capacity() {
        let mut bucket = TokenBucket::new(1, 2);
        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
        assert!(!bucket.try_acquire());
    }
}
