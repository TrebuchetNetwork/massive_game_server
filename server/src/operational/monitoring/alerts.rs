// massive_game_server/server/src/operational/monitoring/alerts.rs

use std::collections::HashMap;

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
