// massive_game_server/server/src/operational/diagnostics/heap_profiler.rs

use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct HeapSnapshot {
    pub resident_bytes: Option<u64>,
    pub allocated_bytes: Option<u64>,
}

pub fn collect_heap_snapshot() -> HeapSnapshot {
    #[cfg(target_os = "linux")]
    {
        // Best-effort parsing from /proc/self/statm: size resident shared text lib data dt
        let Ok(raw) = std::fs::read_to_string("/proc/self/statm") else {
            return HeapSnapshot::default();
        };
        let mut parts = raw.split_whitespace();
        let _size_pages = parts.next().and_then(|value| value.parse::<u64>().ok());
        let resident_pages = parts.next().and_then(|value| value.parse::<u64>().ok());
        let page_size = 4096u64;
        return HeapSnapshot {
            resident_bytes: resident_pages.map(|pages| pages.saturating_mul(page_size)),
            allocated_bytes: None,
        };
    }

    #[cfg(not(target_os = "linux"))]
    {
        HeapSnapshot::default()
    }
}

pub fn spawn_heap_snapshot_logger(interval: Duration) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            let snapshot = collect_heap_snapshot();
            tracing::debug!(
                resident_bytes = snapshot.resident_bytes,
                allocated_bytes = snapshot.allocated_bytes,
                "heap snapshot"
            );
        }
    });
}
