use crate::server::instance::MassiveGameServer;

pub fn parse_list_env(var_name: &str) -> Vec<String> {
    std::env::var(var_name)
        .ok()
        .into_iter()
        .flat_map(|raw| {
            raw.split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

pub fn env_flag(var_name: &str) -> bool {
    std::env::var(var_name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

pub fn parse_u64_env(var_name: &str, default_value: u64) -> u64 {
    std::env::var(var_name)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

pub fn recent_frame_p95_ms(server: &MassiveGameServer) -> Option<f64> {
    let history = server.tick_durations_history.read();
    if history.is_empty() {
        return None;
    }
    let mut samples_ms: Vec<f64> = history
        .iter()
        .rev()
        .take(240)
        .map(|sample| sample.as_secs_f64() * 1000.0)
        .collect();
    if samples_ms.is_empty() {
        return None;
    }
    samples_ms.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let p95_idx = ((samples_ms.len().saturating_sub(1) as f64) * 0.95).round() as usize;
    samples_ms.get(p95_idx).copied()
}
