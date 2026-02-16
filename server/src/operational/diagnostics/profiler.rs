// massive_game_server/server/src/operational/diagnostics/profiler.rs

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone)]
pub struct ProfilerSnapshot {
    pub counters: HashMap<String, u64>,
    pub total_duration: HashMap<String, Duration>,
}

#[derive(Debug, Default)]
pub struct TickProfiler {
    snapshot: ProfilerSnapshot,
}

impl TickProfiler {
    pub fn begin_scope<'a>(&'a mut self, label: &'a str) -> ProfileGuard<'a> {
        ProfileGuard {
            label,
            started_at: Instant::now(),
            profiler: self,
        }
    }

    pub fn snapshot(&self) -> ProfilerSnapshot {
        self.snapshot.clone()
    }
}

pub struct ProfileGuard<'a> {
    label: &'a str,
    started_at: Instant,
    profiler: &'a mut TickProfiler,
}

impl Drop for ProfileGuard<'_> {
    fn drop(&mut self) {
        let elapsed = self.started_at.elapsed();
        let count_entry = self
            .profiler
            .snapshot
            .counters
            .entry(self.label.to_owned())
            .or_insert(0);
        *count_entry += 1;
        let duration_entry = self
            .profiler
            .snapshot
            .total_duration
            .entry(self.label.to_owned())
            .or_insert(Duration::from_millis(0));
        *duration_entry += elapsed;
    }
}
