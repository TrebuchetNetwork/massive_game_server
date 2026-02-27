// massive_game_server/server/src/operational/diagnostics/deadlock.rs

use std::backtrace::Backtrace;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::warn;

#[derive(Clone, Default)]
pub struct DeadlockHeartbeat {
    last_tick_ms: Arc<AtomicU64>,
}

impl DeadlockHeartbeat {
    pub fn beat(&self) {
        self.last_tick_ms.store(unix_now_ms(), Ordering::Relaxed);
    }

    pub fn last_tick_ms(&self) -> u64 {
        self.last_tick_ms.load(Ordering::Relaxed)
    }
}

pub fn spawn_deadlock_watchdog(
    heartbeat: DeadlockHeartbeat,
    check_interval: Duration,
    stale_after: Duration,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(check_interval);
        loop {
            ticker.tick().await;
            let now_ms = unix_now_ms();
            let last = heartbeat.last_tick_ms();
            if last == 0 {
                continue;
            }
            let age_ms = now_ms.saturating_sub(last);
            if age_ms > stale_after.as_millis() as u64 {
                warn!(
                    "Deadlock watchdog: heartbeat stale for {}ms (threshold={}ms)",
                    age_ms,
                    stale_after.as_millis()
                );
            }
        }
    });
}

pub fn spawn_frame_progress_watchdog(
    frame_counter: Arc<AtomicU64>,
    check_interval: Duration,
    stale_after: Duration,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(check_interval);
        let mut last_frame = frame_counter.load(Ordering::Relaxed);
        let mut stagnant_ms: u64 = 0;
        loop {
            ticker.tick().await;
            let current = frame_counter.load(Ordering::Relaxed);
            if current == last_frame {
                stagnant_ms = stagnant_ms.saturating_add(check_interval.as_millis() as u64);
                if stagnant_ms >= stale_after.as_millis() as u64 {
                    let bt = Backtrace::capture();
                    warn!(
                        "Frame progress watchdog: frame counter stagnant at {} for {}ms. Backtrace:\n{:?}",
                        current, stagnant_ms, bt
                    );
                    stagnant_ms = 0;
                }
            } else {
                stagnant_ms = 0;
                last_frame = current;
            }
        }
    });
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_millis(0))
        .as_millis() as u64
}
