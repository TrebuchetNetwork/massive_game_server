// massive_game_server/server/src/operational/monitoring/alerts.rs

use reqwest::Client;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct MetricSnapshot {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Clone)]
pub struct AlertRule {
    pub metric_name: String,
    pub threshold: f64,
    pub greater_is_alert: bool,
}

#[derive(Debug, Clone)]
pub struct AlertEvent {
    pub metric_name: String,
    pub current_value: f64,
    pub threshold: f64,
}

#[derive(Debug, Clone)]
pub struct AlertmanagerConfig {
    pub webhook_url: Option<String>,
    pub source: String,
    pub environment: String,
    pub cooldown: Duration,
}

impl AlertmanagerConfig {
    pub fn from_env() -> Self {
        let webhook_url = std::env::var("MGS_ALERTMANAGER_WEBHOOK_URL")
            .ok()
            .or_else(|| std::env::var("MGS_ALERTMANAGER_URL").ok())
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let source = std::env::var("MGS_ALERT_SOURCE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "massive_game_server".to_owned());
        let environment = std::env::var("MGS_ENV")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "development".to_owned());
        let cooldown_secs = std::env::var("MGS_ALERT_COOLDOWN_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(120);

        Self {
            webhook_url,
            source,
            environment,
            cooldown: Duration::from_secs(cooldown_secs),
        }
    }

    pub fn enabled(&self) -> bool {
        self.webhook_url.is_some()
    }
}

#[derive(Clone)]
pub struct AlertmanagerNotifier {
    config: AlertmanagerConfig,
    client: Client,
    last_sent_at_by_metric: Arc<Mutex<HashMap<String, Instant>>>,
}

#[derive(Debug, Serialize)]
struct AlertmanagerPayload {
    labels: HashMap<String, String>,
    annotations: HashMap<String, String>,
}

impl AlertmanagerNotifier {
    pub fn new(config: AlertmanagerConfig) -> Self {
        Self {
            config,
            client: Client::new(),
            last_sent_at_by_metric: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled()
    }

    pub async fn notify_events(&self, events: &[AlertEvent]) {
        if events.is_empty() || !self.enabled() {
            return;
        }

        let Some(webhook_url) = self.config.webhook_url.as_deref() else {
            return;
        };

        let now = Instant::now();
        let mut payload: Vec<AlertmanagerPayload> = Vec::new();
        {
            let mut sent_guard = self
                .last_sent_at_by_metric
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for event in events {
                if let Some(last_sent) = sent_guard.get(&event.metric_name) {
                    if now.duration_since(*last_sent) < self.config.cooldown {
                        continue;
                    }
                }
                sent_guard.insert(event.metric_name.clone(), now);
                payload.push(build_alert_payload(&self.config, event));
            }
        }

        if payload.is_empty() {
            return;
        }

        match self.client.post(webhook_url).json(&payload).send().await {
            Ok(response) if response.status().is_success() => {
                info!(
                    "Dispatched {} alert event(s) to Alertmanager.",
                    payload.len()
                );
            }
            Ok(response) => {
                warn!(
                    "Alertmanager webhook returned non-success status: {}",
                    response.status()
                );
            }
            Err(err) => {
                error!("Failed to dispatch alerts to Alertmanager: {}", err);
            }
        }
    }
}

fn build_alert_payload(config: &AlertmanagerConfig, event: &AlertEvent) -> AlertmanagerPayload {
    let mut labels = HashMap::new();
    labels.insert("alertname".to_owned(), format!("MGS{}", event.metric_name));
    labels.insert("service".to_owned(), config.source.clone());
    labels.insert("severity".to_owned(), "warning".to_owned());
    labels.insert("environment".to_owned(), config.environment.clone());
    labels.insert("metric".to_owned(), event.metric_name.clone());

    let mut annotations = HashMap::new();
    annotations.insert(
        "summary".to_owned(),
        format!(
            "Metric '{}' crossed threshold: value={} threshold={}",
            event.metric_name, event.current_value, event.threshold
        ),
    );
    annotations.insert(
        "description".to_owned(),
        format!(
            "Massive game server metric '{}' fired (current={}, threshold={}).",
            event.metric_name, event.current_value, event.threshold
        ),
    );

    AlertmanagerPayload {
        labels,
        annotations,
    }
}

pub fn default_alert_rules_from_env() -> Vec<AlertRule> {
    let mut rules = Vec::new();

    if let Some(max_frame_ms) = parse_env_f64("MGS_ALERT_MAX_FRAME_MS") {
        rules.push(AlertRule {
            metric_name: "game_frame_time_ms_p95".to_owned(),
            threshold: max_frame_ms.max(1.0),
            greater_is_alert: true,
        });
    }
    if let Some(max_memory_rss_bytes) = parse_env_f64("MGS_ALERT_MAX_RSS_BYTES") {
        rules.push(AlertRule {
            metric_name: "game_memory_rss_bytes".to_owned(),
            threshold: max_memory_rss_bytes.max(1.0),
            greater_is_alert: true,
        });
    }
    if let Some(max_connected_players) = parse_env_f64("MGS_ALERT_MAX_CONNECTED_PLAYERS") {
        rules.push(AlertRule {
            metric_name: "game_players_connected".to_owned(),
            threshold: max_connected_players.max(1.0),
            greater_is_alert: true,
        });
    }
    if let Some(max_auth_failures_per_minute) =
        parse_env_f64("MGS_ALERT_MAX_AUTH_FAILURES_PER_MINUTE")
    {
        rules.push(AlertRule {
            metric_name: "game_auth_failures_per_minute".to_owned(),
            threshold: max_auth_failures_per_minute.max(1.0),
            greater_is_alert: true,
        });
    }

    rules
}

fn parse_env_f64(var_name: &str) -> Option<f64> {
    std::env::var(var_name)
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

pub fn evaluate_alerts(rules: &[AlertRule], snapshots: &[MetricSnapshot]) -> Vec<AlertEvent> {
    let metrics = snapshots
        .iter()
        .map(|metric| (metric.name.as_str(), metric.value))
        .collect::<HashMap<_, _>>();

    let mut alerts = Vec::new();
    for rule in rules {
        let Some(current) = metrics.get(rule.metric_name.as_str()).copied() else {
            continue;
        };
        let fired = if rule.greater_is_alert {
            current >= rule.threshold
        } else {
            current <= rule.threshold
        };
        if fired {
            alerts.push(AlertEvent {
                metric_name: rule.metric_name.clone(),
                current_value: current,
                threshold: rule.threshold,
            });
        }
    }
    alerts
}
