use super::*;

pub(super) fn round_metric(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn percentile_sorted(values_sorted: &[f64], percentile: f64) -> f64 {
    if values_sorted.is_empty() {
        return 0.0;
    }
    if values_sorted.len() == 1 {
        return values_sorted[0];
    }
    let clamped = percentile.clamp(0.0, 1.0);
    let idx = (values_sorted.len() - 1) as f64 * clamped;
    let lower = idx.floor() as usize;
    let upper = idx.ceil() as usize;
    if lower == upper {
        values_sorted[lower]
    } else {
        let weight = idx - lower as f64;
        values_sorted[lower] + (values_sorted[upper] - values_sorted[lower]) * weight
    }
}

pub(super) fn summarize_join_stage_latencies(values: &[f64]) -> JoinStageLatencyStats {
    if values.is_empty() {
        return JoinStageLatencyStats::default();
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
    JoinStageLatencyStats {
        count: sorted.len(),
        avg_ms: round_metric(avg),
        p95_ms: round_metric(percentile_sorted(&sorted, 0.95)),
        max_ms: round_metric(*sorted.last().unwrap_or(&0.0)),
    }
}

pub(super) fn load_dispute_chain_head(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let content = String::from_utf8(bytes).ok()?;
    let last_line = content.lines().rev().find(|line| !line.trim().is_empty())?;
    let record = serde_json::from_str::<PersistedLiveReplayDisputeRecord>(last_line).ok()?;
    Some(record.chain_hash_sha256)
}

pub(super) fn append_dispute_record(
    path: &Path,
    record: &PersistedLiveReplayDisputeRecord,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    let line = serde_json::to_string(record).map_err(|err| err.to_string())?;
    writeln!(file, "{}", line).map_err(|err| err.to_string())
}

pub(super) fn sha256_hex(payload: &[u8]) -> String {
    let digest = Sha256::digest(payload);
    let mut rendered = String::with_capacity(digest.len() * 2);
    for byte in digest {
        rendered.push_str(&format!("{:02x}", byte));
    }
    rendered
}

pub(super) fn hmac_sha256_hex(key: &[u8], payload: &[u8]) -> Option<String> {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).ok()?;
    mac.update(payload);
    let bytes = mac.finalize().into_bytes();
    let mut rendered = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        rendered.push_str(&format!("{:02x}", byte));
    }
    Some(rendered)
}
