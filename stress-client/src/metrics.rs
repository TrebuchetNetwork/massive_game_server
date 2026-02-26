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

        println!("\n{}", "=".repeat(60));
        println!("  Scenario: {}", self.scenario_name);
        println!("  Duration: {:.1}s", elapsed.as_secs_f64());
        println!(
            "  Bots: {}/{} connected, {}/{} welcomed",
            connected, self.target_bots, welcomed, self.target_bots
        );
        println!("  Total deltas received: {}", total_deltas);
        println!("  Total inputs sent:     {}", total_inputs);

        // Join latency stats
        let join_latencies: Vec<f64> = bots
            .iter()
            .filter_map(|b| b.join_latency.map(|d| d.as_secs_f64() * 1000.0))
            .collect();
        if !join_latencies.is_empty() {
            let mean = join_latencies.iter().sum::<f64>() / join_latencies.len() as f64;
            let max = join_latencies.iter().cloned().fold(f64::MIN, f64::max);
            let min = join_latencies.iter().cloned().fold(f64::MAX, f64::min);
            println!(
                "  Join latency (ms):     min={:.0}  mean={:.0}  max={:.0}",
                min, mean, max
            );
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
        if !all_mean_intervals.is_empty() {
            let avg_mean =
                all_mean_intervals.iter().sum::<f64>() / all_mean_intervals.len() as f64;
            println!("  Avg mean delta interval: {:.1}ms", avg_mean);
        }
        if !all_p95_intervals.is_empty() {
            let avg_p95 = all_p95_intervals.iter().sum::<f64>() / all_p95_intervals.len() as f64;
            println!("  Avg p95 delta interval:  {:.1}ms", avg_p95);
        }

        // Completed vs disconnected
        let completed_count = bots.iter().filter(|b| b.completed).count();
        println!("  Completed gracefully:  {}/{}", completed_count, self.target_bots);

        let disconnected: Vec<&BotMetrics> = bots.iter().filter(|b| !b.connected && !b.completed).collect();
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

        println!("{}\n", "=".repeat(60));

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
