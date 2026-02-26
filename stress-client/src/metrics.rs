use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Per-bot metrics collected during a stress run.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BotMetrics {
    pub bot_id: usize,
    pub username: String,
    /// Time from WebSocket connect to receiving WelcomeMessage on the data channel.
    pub join_latency: Option<Duration>,
    /// Time from WebSocket connect to data-channel open.
    pub dc_open_latency: Option<Duration>,
    /// Total delta state messages received.
    pub deltas_received: u64,
    /// Total input messages sent.
    pub inputs_sent: u64,
    /// Whether the bot is still connected at measurement time.
    pub connected: bool,
    /// If disconnected, the reason.
    pub disconnect_reason: Option<String>,
    /// Whether the bot completed its run duration gracefully.
    pub completed: bool,
    /// Timestamps of received delta messages for jitter analysis.
    pub delta_timestamps: Vec<Instant>,
}

impl BotMetrics {
    pub fn new(bot_id: usize, username: &str) -> Self {
        Self {
            bot_id,
            username: username.to_string(),
            join_latency: None,
            dc_open_latency: None,
            deltas_received: 0,
            inputs_sent: 0,
            connected: false,
            disconnect_reason: None,
            completed: false,
            delta_timestamps: Vec::with_capacity(512),
        }
    }

    /// Mean inter-delta interval (jitter baseline).
    pub fn mean_delta_interval(&self) -> Option<Duration> {
        if self.delta_timestamps.len() < 2 {
            return None;
        }
        let intervals: Vec<Duration> = self
            .delta_timestamps
            .windows(2)
            .map(|w| w[1].duration_since(w[0]))
            .collect();
        let total: Duration = intervals.iter().sum();
        Some(total / intervals.len() as u32)
    }

    /// 95th-percentile inter-delta interval.
    pub fn p95_delta_interval(&self) -> Option<Duration> {
        if self.delta_timestamps.len() < 2 {
            return None;
        }
        let mut intervals: Vec<Duration> = self
            .delta_timestamps
            .windows(2)
            .map(|w| w[1].duration_since(w[0]))
            .collect();
        intervals.sort();
        let idx = ((intervals.len() as f64) * 0.95).ceil() as usize - 1;
        Some(intervals[idx.min(intervals.len() - 1)])
    }
}

/// Aggregate metrics across all bots in a scenario run.
#[derive(Debug)]
pub struct ScenarioMetrics {
    pub scenario_name: String,
    pub target_bots: usize,
    pub bots: Arc<RwLock<Vec<BotMetrics>>>,
    pub scenario_start: Instant,
    /// Counts for quick atomic reads.
    pub connected_count: AtomicUsize,
    pub welcome_received_count: AtomicUsize,
    pub total_deltas: AtomicU64,
    pub total_inputs_sent: AtomicU64,
}

impl ScenarioMetrics {
    pub fn new(scenario_name: &str, target_bots: usize) -> Arc<Self> {
        Arc::new(Self {
            scenario_name: scenario_name.to_string(),
            target_bots,
            bots: Arc::new(RwLock::new(Vec::with_capacity(target_bots))),
            scenario_start: Instant::now(),
            connected_count: AtomicUsize::new(0),
            welcome_received_count: AtomicUsize::new(0),
            total_deltas: AtomicU64::new(0),
            total_inputs_sent: AtomicU64::new(0),
        })
    }

    pub async fn register_bot(&self, bot_id: usize, username: &str) {
        let mut bots = self.bots.write().await;
        if bot_id >= bots.len() {
            bots.resize_with(bot_id + 1, || BotMetrics::new(0, ""));
        }
        bots[bot_id] = BotMetrics::new(bot_id, username);
    }

    pub async fn mark_dc_open(&self, bot_id: usize, latency: Duration) {
        let mut bots = self.bots.write().await;
        if let Some(m) = bots.get_mut(bot_id) {
            m.dc_open_latency = Some(latency);
        }
    }

    pub async fn mark_connected(&self, bot_id: usize, join_latency: Duration) {
        self.connected_count.fetch_add(1, Ordering::Relaxed);
        self.welcome_received_count.fetch_add(1, Ordering::Relaxed);
        let mut bots = self.bots.write().await;
        if let Some(m) = bots.get_mut(bot_id) {
            m.join_latency = Some(join_latency);
            m.connected = true;
        }
    }

    pub async fn record_delta(&self, bot_id: usize) {
        self.total_deltas.fetch_add(1, Ordering::Relaxed);
        let mut bots = self.bots.write().await;
        if let Some(m) = bots.get_mut(bot_id) {
            m.deltas_received += 1;
            m.delta_timestamps.push(Instant::now());
        }
    }

    pub async fn record_input_sent(&self, bot_id: usize) {
        self.total_inputs_sent.fetch_add(1, Ordering::Relaxed);
        let mut bots = self.bots.write().await;
        if let Some(m) = bots.get_mut(bot_id) {
            m.inputs_sent += 1;
        }
    }

    pub async fn mark_completed(&self, bot_id: usize) {
        let mut bots = self.bots.write().await;
        if let Some(m) = bots.get_mut(bot_id) {
            m.completed = true;
        }
    }

    pub async fn mark_disconnected(&self, bot_id: usize, reason: &str) {
        let mut bots = self.bots.write().await;
        if let Some(m) = bots.get_mut(bot_id) {
            // Don't count graceful shutdown as a disconnect
            if m.completed {
                return;
            }
            if m.connected {
                self.connected_count.fetch_sub(1, Ordering::Relaxed);
            }
            m.connected = false;
            m.disconnect_reason = Some(reason.to_string());
        }
    }

    /// Print a summary report and return true if pass criteria are met.
    pub async fn summarize_and_evaluate(&self) -> bool {
        let bots = self.bots.read().await;
        let elapsed = self.scenario_start.elapsed();
        let connected = self.connected_count.load(Ordering::Relaxed);
        let welcomed = self.welcome_received_count.load(Ordering::Relaxed);
        let total_deltas = self.total_deltas.load(Ordering::Relaxed);
        let total_inputs = self.total_inputs_sent.load(Ordering::Relaxed);

        println!("\n{}", "=".repeat(72));
        println!("  Scenario: {}", self.scenario_name);
        println!("  Duration: {:.1}s", elapsed.as_secs_f64());
        println!(
            "  Bots: {}/{} connected, {}/{} welcomed",
            connected, self.target_bots, welcomed, self.target_bots
        );
        println!("  Total deltas received: {}", total_deltas);
        println!("  Total inputs sent:     {}", total_inputs);

        // --- Connection statistics ---
        println!("\n  --- Connection Statistics ---");
        println!(
            "  Connections attempted: {}",
            bots.iter().filter(|b| !b.username.is_empty()).count()
        );
        println!("  Connections successful (welcomed): {}", welcomed);
        let failed_count = bots
            .iter()
            .filter(|b| !b.username.is_empty() && !b.connected && !b.completed)
            .count();
        println!("  Connections failed:    {}", failed_count);

        // --- Join latency statistics ---
        let join_latencies: Vec<f64> = bots
            .iter()
            .filter_map(|b| b.join_latency.map(|d| d.as_secs_f64() * 1000.0))
            .collect();
        if !join_latencies.is_empty() {
            let stats = compute_latency_stats(&join_latencies);
            println!("\n  --- Join Latency (ms) ---");
            println!("  min:  {:.1}", stats.min);
            println!("  avg:  {:.1}", stats.avg);
            println!("  p50:  {:.1}", stats.p50);
            println!("  p95:  {:.1}", stats.p95);
            println!("  p99:  {:.1}", stats.p99);
            println!("  max:  {:.1}", stats.max);
        }

        // --- DC open latency statistics ---
        let dc_latencies: Vec<f64> = bots
            .iter()
            .filter_map(|b| b.dc_open_latency.map(|d| d.as_secs_f64() * 1000.0))
            .collect();
        if !dc_latencies.is_empty() {
            let stats = compute_latency_stats(&dc_latencies);
            println!("\n  --- DataChannel Open Latency (ms) ---");
            println!("  min:  {:.1}", stats.min);
            println!("  avg:  {:.1}", stats.avg);
            println!("  p50:  {:.1}", stats.p50);
            println!("  p95:  {:.1}", stats.p95);
            println!("  p99:  {:.1}", stats.p99);
            println!("  max:  {:.1}", stats.max);
        }

        // Delta interval stats
        let mut all_mean_intervals: Vec<f64> = Vec::new();
        let mut all_p95_intervals: Vec<f64> = Vec::new();
        for b in bots.iter() {
            if let Some(mean) = b.mean_delta_interval() {
                all_mean_intervals.push(mean.as_secs_f64() * 1000.0);
            }
            if let Some(p95) = b.p95_delta_interval() {
                all_p95_intervals.push(p95.as_secs_f64() * 1000.0);
            }
        }
        if !all_mean_intervals.is_empty() || !all_p95_intervals.is_empty() {
            println!("\n  --- Delta Interval (ms) ---");
        }
        if !all_mean_intervals.is_empty() {
            let avg_mean = all_mean_intervals.iter().sum::<f64>() / all_mean_intervals.len() as f64;
            println!("  Avg mean delta interval: {:.1}", avg_mean);
        }
        if !all_p95_intervals.is_empty() {
            let avg_p95 = all_p95_intervals.iter().sum::<f64>() / all_p95_intervals.len() as f64;
            println!("  Avg p95 delta interval:  {:.1}", avg_p95);
        }

        // Completed vs disconnected
        let completed_count = bots.iter().filter(|b| b.completed).count();
        println!(
            "\n  Completed gracefully:  {}/{}",
            completed_count, self.target_bots
        );

        let disconnected: Vec<&BotMetrics> = bots
            .iter()
            .filter(|b| !b.connected && !b.completed && !b.username.is_empty())
            .collect();
        if !disconnected.is_empty() {
            println!("  Disconnected bots: {}", disconnected.len());
            for d in disconnected.iter().take(10) {
                println!(
                    "    bot#{}: {}",
                    d.bot_id,
                    d.disconnect_reason.as_deref().unwrap_or("unknown")
                );
            }
            if disconnected.len() > 10 {
                println!("    ... and {} more", disconnected.len() - 10);
            }
        }

        println!("{}\n", "=".repeat(72));

        // Pass criteria:
        // - At least 90% of target bots received a welcome message
        // - At least 80% completed gracefully (or still connected)
        // - At least 1 delta received per welcomed bot on average
        let welcome_ratio = welcomed as f64 / self.target_bots as f64;
        // Bots that completed gracefully OR are still connected at eval time
        let success_count = bots.iter().filter(|b| b.completed || b.connected).count();
        let success_ratio = success_count as f64 / self.target_bots as f64;
        let avg_deltas = if welcomed > 0 {
            total_deltas as f64 / welcomed as f64
        } else {
            0.0
        };

        let pass = welcome_ratio >= 0.90 && success_ratio >= 0.80 && avg_deltas >= 1.0;

        if pass {
            println!(
                "  PASS  (welcome={:.0}%, success={:.0}%, avg_deltas={:.1})",
                welcome_ratio * 100.0,
                success_ratio * 100.0,
                avg_deltas
            );
        } else {
            println!(
                "  FAIL  (welcome={:.0}%, success={:.0}%, avg_deltas={:.1})",
                welcome_ratio * 100.0,
                success_ratio * 100.0,
                avg_deltas
            );
        }

        pass
    }
}

/// Summary statistics for a latency distribution.
struct LatencyStats {
    min: f64,
    avg: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

/// Compute min/avg/p50/p95/p99/max from a slice of f64 latency values.
fn compute_latency_stats(values: &[f64]) -> LatencyStats {
    if values.is_empty() {
        return LatencyStats {
            min: 0.0,
            avg: 0.0,
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
            max: 0.0,
        };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));

    let n = sorted.len();
    let avg = sorted.iter().sum::<f64>() / n as f64;
    let min = sorted[0];
    let max = sorted[n - 1];

    let p50 = percentile_sorted(&sorted, 0.50);
    let p95 = percentile_sorted(&sorted, 0.95);
    let p99 = percentile_sorted(&sorted, 0.99);

    LatencyStats {
        min,
        avg,
        p50,
        p95,
        p99,
        max,
    }
}

/// Compute a percentile from a pre-sorted slice.
fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
