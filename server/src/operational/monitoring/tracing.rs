// massive_game_server/server/src/operational/monitoring/tracing.rs

use std::sync::atomic::{AtomicU64, Ordering};

static TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn next_trace_id() -> u64 {
    TRACE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub fn with_trace_fields<R>(label: &str, f: impl FnOnce() -> R) -> R {
    let trace_id = next_trace_id();
    let span = tracing::info_span!("distributed_trace", trace_id, label = label);
    let _guard = span.enter();
    f()
}
