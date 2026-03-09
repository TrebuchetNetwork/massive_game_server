use super::*;
use redis::Commands;

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

fn dispute_chain_head_redis_key(base_key: &str) -> String {
    format!("{}:chain_head", base_key)
}

fn dispute_records_redis_key(base_key: &str) -> String {
    format!("{}:records", base_key)
}

fn load_dispute_chain_head_from_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let content = String::from_utf8(bytes).ok()?;
    let last_line = content.lines().rev().find(|line| !line.trim().is_empty())?;
    let record = serde_json::from_str::<PersistedLiveReplayDisputeRecord>(last_line).ok()?;
    Some(record.chain_hash_sha256)
}

fn load_dispute_chain_head_from_redis(
    redis_url: &str,
    base_key: &str,
) -> Result<Option<String>, String> {
    let client = redis::Client::open(redis_url.to_owned())
        .map_err(|err| format!("failed to configure Redis client '{}': {}", redis_url, err))?;
    let mut connection = client
        .get_connection()
        .map_err(|err| format!("failed to connect to Redis '{}': {}", redis_url, err))?;
    let chain_head_key = dispute_chain_head_redis_key(base_key);
    connection
        .get(&chain_head_key)
        .map_err(|err| format!("failed to read Redis key '{}': {}", chain_head_key, err))
}

pub(super) fn load_dispute_chain_head(
    path: &Path,
    redis_url: Option<&str>,
    redis_key: &str,
) -> Option<String> {
    let normalized_redis_key = redis_key.trim();
    if let Some(redis_url) = redis_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| !normalized_redis_key.is_empty())
    {
        match load_dispute_chain_head_from_redis(redis_url, normalized_redis_key) {
            Ok(Some(chain_head)) => return Some(chain_head),
            Ok(None) => {}
            Err(err) => tracing::warn!("Failed to load dispute chain head from Redis: {}", err),
        }
    }
    load_dispute_chain_head_from_file(path)
}

fn append_dispute_record_to_file(path: &Path, line: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    writeln!(file, "{}", line).map_err(|err| err.to_string())
}

fn append_dispute_record_to_redis(
    redis_url: &str,
    base_key: &str,
    line: &str,
    chain_hash: &str,
) -> Result<(), String> {
    let client = redis::Client::open(redis_url.to_owned())
        .map_err(|err| format!("failed to configure Redis client '{}': {}", redis_url, err))?;
    let mut connection = client
        .get_connection()
        .map_err(|err| format!("failed to connect to Redis '{}': {}", redis_url, err))?;
    let records_key = dispute_records_redis_key(base_key);
    let chain_head_key = dispute_chain_head_redis_key(base_key);
    redis::pipe()
        .atomic()
        .cmd("RPUSH")
        .arg(&records_key)
        .arg(line)
        .cmd("SET")
        .arg(&chain_head_key)
        .arg(chain_hash)
        .query::<()>(&mut connection)
        .map_err(|err| {
            format!(
                "failed to persist Redis dispute record '{}' / '{}': {}",
                records_key, chain_head_key, err
            )
        })
}

pub(super) fn append_dispute_record(
    path: &Path,
    redis_url: Option<&str>,
    redis_key: &str,
    record: &PersistedLiveReplayDisputeRecord,
) -> Result<(), String> {
    let line = serde_json::to_string(record).map_err(|err| err.to_string())?;
    let normalized_redis_key = redis_key.trim();
    if let Some(redis_url) = redis_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| !normalized_redis_key.is_empty())
    {
        append_dispute_record_to_redis(
            redis_url,
            normalized_redis_key,
            line.as_str(),
            record.chain_hash_sha256.as_str(),
        )?;
        if let Err(err) = append_dispute_record_to_file(path, line.as_str()) {
            tracing::warn!(
                "failed to mirror live replay dispute record to '{}': {}",
                path.display(),
                err
            );
        }
        return Ok(());
    }
    append_dispute_record_to_file(path, line.as_str())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_persisted_dispute_record(chain_hash: &str) -> PersistedLiveReplayDisputeRecord {
        PersistedLiveReplayDisputeRecord {
            dispute_id: "dispute-1".to_owned(),
            generated_at_ms: 1234,
            total_captured_frames: 32,
            selected_frame_count: 8,
            selected_from_frame: Some(10),
            selected_to_frame: Some(18),
            kill_feed_event_count: 2,
            filter: LiveReplayDisputeFilter {
                from_frame: Some(10),
                to_frame: Some(18),
                player_id: Some("player-1".to_owned()),
            },
            payload_sha256: "payload-hash".to_owned(),
            chain_hash_sha256: chain_hash.to_owned(),
            chain_prev_hash_sha256: Some("prev-hash".to_owned()),
            signature_hmac_sha256: Some("signature".to_owned()),
        }
    }

    fn temp_dispute_path(test_name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mgs-{}-{}-{}.jsonl",
            test_name,
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn append_dispute_record_persists_file_when_redis_disabled() {
        let path = temp_dispute_path("dispute-file-persist");
        let record = sample_persisted_dispute_record("chain-hash-1");

        append_dispute_record(&path, None, "", &record).expect("file persistence should succeed");

        let chain_head = load_dispute_chain_head(&path, None, "");
        assert_eq!(chain_head.as_deref(), Some("chain-hash-1"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn load_dispute_chain_head_falls_back_to_file_when_redis_is_invalid() {
        let path = temp_dispute_path("dispute-redis-fallback");
        let record = sample_persisted_dispute_record("chain-hash-2");
        append_dispute_record(&path, None, "", &record).expect("file persistence should succeed");

        let chain_head =
            load_dispute_chain_head(&path, Some("redis://"), "mgs:test:live_replay:disputes");
        assert_eq!(chain_head.as_deref(), Some("chain-hash-2"));

        let _ = fs::remove_file(path);
    }
}
