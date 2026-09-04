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

fn match_summary_redis_key(base_key: &str) -> String {
    format!("{}:latest_summary", base_key)
}

fn match_snapshot_records_redis_key(base_key: &str) -> String {
    format!("{}:snapshots", base_key)
}

fn latest_match_summary_file_path(store_dir: &Path) -> PathBuf {
    store_dir.join("latest_summary.json")
}

fn match_snapshot_index_file_path(store_dir: &Path) -> PathBuf {
    store_dir.join("snapshot_index.json")
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
        .get_connection_with_timeout(std::time::Duration::from_secs(2))
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
        .get_connection_with_timeout(std::time::Duration::from_secs(2))
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

fn load_latest_match_end_summary_from_file(store_dir: &Path) -> Option<MatchEndSummary> {
    let path = latest_match_summary_file_path(store_dir);
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn load_latest_match_end_summary_from_redis(
    redis_url: &str,
    base_key: &str,
) -> Result<Option<MatchEndSummary>, String> {
    let client = redis::Client::open(redis_url.to_owned())
        .map_err(|err| format!("failed to configure Redis client '{}': {}", redis_url, err))?;
    let mut connection = client
        .get_connection_with_timeout(std::time::Duration::from_secs(2))
        .map_err(|err| format!("failed to connect to Redis '{}': {}", redis_url, err))?;
    let summary_key = match_summary_redis_key(base_key);
    let payload: Option<String> = connection
        .get(&summary_key)
        .map_err(|err| format!("failed to read Redis key '{}': {}", summary_key, err))?;
    payload
        .map(|raw| serde_json::from_str::<MatchEndSummary>(&raw).map_err(|err| err.to_string()))
        .transpose()
}

pub(super) fn load_latest_match_end_summary(
    store_dir: &Path,
    redis_url: Option<&str>,
    redis_key: &str,
) -> Option<MatchEndSummary> {
    let normalized_redis_key = redis_key.trim();
    if let Some(redis_url) = redis_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| !normalized_redis_key.is_empty())
    {
        match load_latest_match_end_summary_from_redis(redis_url, normalized_redis_key) {
            Ok(Some(summary)) => return Some(summary),
            Ok(None) => {}
            Err(err) => tracing::warn!("Failed to load latest match summary from Redis: {}", err),
        }
    }
    load_latest_match_end_summary_from_file(store_dir)
}

fn persist_latest_match_end_summary_to_file(
    store_dir: &Path,
    payload: &[u8],
) -> Result<(), String> {
    fs::create_dir_all(store_dir).map_err(|err| err.to_string())?;
    let path = latest_match_summary_file_path(store_dir);
    fs::write(path, payload).map_err(|err| err.to_string())
}

fn persist_latest_match_end_summary_to_redis(
    redis_url: &str,
    base_key: &str,
    payload: &str,
) -> Result<(), String> {
    let client = redis::Client::open(redis_url.to_owned())
        .map_err(|err| format!("failed to configure Redis client '{}': {}", redis_url, err))?;
    let mut connection = client
        .get_connection_with_timeout(std::time::Duration::from_secs(2))
        .map_err(|err| format!("failed to connect to Redis '{}': {}", redis_url, err))?;
    let summary_key = match_summary_redis_key(base_key);
    connection
        .set::<_, _, ()>(&summary_key, payload)
        .map_err(|err| {
            format!(
                "failed to persist Redis match summary '{}': {}",
                summary_key, err
            )
        })
}

/// Rotate the append-only summary history once it exceeds this size (~10
/// days of blitz matches); one previous generation is kept as `.1`.
const MATCH_SUMMARY_HISTORY_MAX_BYTES: u64 = 16 * 1024 * 1024;

fn append_match_summary_history(store_dir: &Path, payload: &[u8]) -> Result<(), String> {
    use std::io::Write;
    fs::create_dir_all(store_dir).map_err(|err| err.to_string())?;
    let path = store_dir.join("match_summaries.jsonl");
    if let Ok(metadata) = fs::metadata(&path) {
        if metadata.len() >= MATCH_SUMMARY_HISTORY_MAX_BYTES {
            let rotated = store_dir.join("match_summaries.jsonl.1");
            fs::rename(&path, rotated).map_err(|err| err.to_string())?;
        }
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    file.write_all(payload).map_err(|err| err.to_string())?;
    file.write_all(b"\n").map_err(|err| err.to_string())
}

pub(super) fn persist_latest_match_end_summary(
    store_dir: &Path,
    redis_url: Option<&str>,
    redis_key: &str,
    summary: &MatchEndSummary,
) -> Result<(), String> {
    let payload = serde_json::to_vec(summary).map_err(|err| err.to_string())?;
    let payload_str = std::str::from_utf8(&payload).map_err(|err| err.to_string())?;
    // Every finished match appends one line of history for per-mode
    // excitement analysis; `latest_summary.json` alone is overwritten each
    // match and cannot answer "which mode is the most dynamic".
    if let Err(err) = append_match_summary_history(store_dir, &payload) {
        tracing::warn!("failed to append match summary history: {}", err);
    }
    let normalized_redis_key = redis_key.trim();
    if let Some(redis_url) = redis_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| !normalized_redis_key.is_empty())
    {
        persist_latest_match_end_summary_to_redis(redis_url, normalized_redis_key, payload_str)?;
        if let Err(err) = persist_latest_match_end_summary_to_file(store_dir, &payload) {
            tracing::warn!(
                "failed to mirror latest match summary to '{}': {}",
                latest_match_summary_file_path(store_dir).display(),
                err
            );
        }
        return Ok(());
    }
    persist_latest_match_end_summary_to_file(store_dir, &payload)
}

fn load_match_snapshot_records_from_file(
    store_dir: &Path,
) -> Vec<PersistedMatchReplaySnapshotRecord> {
    let path = match_snapshot_index_file_path(store_dir);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Vec::new(),
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn persist_match_snapshot_records_to_file(
    store_dir: &Path,
    records: &[PersistedMatchReplaySnapshotRecord],
) -> Result<(), String> {
    fs::create_dir_all(store_dir).map_err(|err| err.to_string())?;
    let path = match_snapshot_index_file_path(store_dir);
    let payload = serde_json::to_vec(records).map_err(|err| err.to_string())?;
    fs::write(path, payload).map_err(|err| err.to_string())
}

fn append_match_snapshot_record_to_redis(
    redis_url: &str,
    base_key: &str,
    line: &str,
    retention: usize,
) -> Result<(), String> {
    let client = redis::Client::open(redis_url.to_owned())
        .map_err(|err| format!("failed to configure Redis client '{}': {}", redis_url, err))?;
    let mut connection = client
        .get_connection_with_timeout(std::time::Duration::from_secs(2))
        .map_err(|err| format!("failed to connect to Redis '{}': {}", redis_url, err))?;
    let records_key = match_snapshot_records_redis_key(base_key);
    redis::pipe()
        .atomic()
        .cmd("RPUSH")
        .arg(&records_key)
        .arg(line)
        .cmd("LTRIM")
        .arg(&records_key)
        .arg(-(retention as isize))
        .arg(-1)
        .query::<()>(&mut connection)
        .map_err(|err| {
            format!(
                "failed to persist Redis match snapshot record '{}': {}",
                records_key, err
            )
        })
}

pub(super) fn append_match_snapshot_record(
    store_dir: &Path,
    redis_url: Option<&str>,
    redis_key: &str,
    record: &PersistedMatchReplaySnapshotRecord,
    retention: usize,
) -> Result<(), String> {
    let line = serde_json::to_string(record).map_err(|err| err.to_string())?;
    let mut records = load_match_snapshot_records_from_file(store_dir);
    records.push(record.clone());
    if records.len() > retention {
        let keep_from = records.len().saturating_sub(retention);
        records.drain(0..keep_from);
    }
    persist_match_snapshot_records_to_file(store_dir, &records)?;

    let normalized_redis_key = redis_key.trim();
    if let Some(redis_url) = redis_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| !normalized_redis_key.is_empty())
    {
        append_match_snapshot_record_to_redis(
            redis_url,
            normalized_redis_key,
            line.as_str(),
            retention,
        )?;
    }
    Ok(())
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

    fn temp_match_store_dir(test_name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mgs-{}-{}-{}",
            test_name,
            std::process::id(),
            nanos
        ))
    }

    fn sample_match_summary() -> MatchEndSummary {
        MatchEndSummary {
            generated_at_ms: 4321,
            reason: "match_end".to_owned(),
            map_name: "Arena".to_owned(),
            game_mode: "FreeForAll".to_owned(),
            match_duration: 120.0,
            winning_team: 0,
            total_kills: 5,
            kills_per_minute: 2.5,
            final_score_margin: 200,
            phases: Vec::new(),
            coop_gauntlet: false,
            gauntlet: None,
            players: vec![PlayerMatchStats {
                player_id: "player-1".to_owned(),
                player_name: "Player One".to_owned(),
                team_id: 0,
                kills: 5,
                deaths: 2,
                score: 200,
                damage_dealt: 900,
                damage_taken: 450,
                flag_captures: 0,
                flag_returns: 0,
                hot_zone_kills: 1,
                hot_zone_time_seconds: 4.0,
                weapon_kills: vec![1, 2, 1, 1, 0],
                kd_ratio: 2.5,
            }],
            mvp_kills: Some("Player One".to_owned()),
            mvp_damage: Some("Player One".to_owned()),
            mvp_objectives: None,
        }
    }

    fn sample_match_snapshot_record() -> PersistedMatchReplaySnapshotRecord {
        PersistedMatchReplaySnapshotRecord {
            generated_at_ms: 5555,
            reason: "match_end".to_owned(),
            map_name: "Arena".to_owned(),
            file_name: "replay_5555_match_end.json.zst".to_owned(),
            frame_count: 64,
            compressed_bytes: 1024,
        }
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

    #[test]
    fn persist_latest_match_end_summary_persists_file_when_redis_disabled() {
        let store_dir = temp_match_store_dir("match-summary-file-persist");
        let summary = sample_match_summary();

        persist_latest_match_end_summary(&store_dir, None, "", &summary)
            .expect("summary persistence should succeed");

        let loaded = load_latest_match_end_summary(&store_dir, None, "");
        assert_eq!(loaded.unwrap().generated_at_ms, summary.generated_at_ms);

        let _ = fs::remove_dir_all(store_dir);
    }

    #[test]
    fn load_latest_match_end_summary_falls_back_to_file_when_redis_is_invalid() {
        let store_dir = temp_match_store_dir("match-summary-redis-fallback");
        let summary = sample_match_summary();
        persist_latest_match_end_summary(&store_dir, None, "", &summary)
            .expect("summary persistence should succeed");

        let loaded = load_latest_match_end_summary(
            &store_dir,
            Some("redis://"),
            "mgs:test:live_replay:matches",
        );
        assert_eq!(loaded.unwrap().generated_at_ms, summary.generated_at_ms);

        let _ = fs::remove_dir_all(store_dir);
    }

    #[test]
    fn append_match_snapshot_record_persists_index_file_when_redis_disabled() {
        let store_dir = temp_match_store_dir("match-snapshot-index");
        let record = sample_match_snapshot_record();

        append_match_snapshot_record(&store_dir, None, "", &record, 8)
            .expect("snapshot metadata persistence should succeed");

        let records = load_match_snapshot_records_from_file(&store_dir);
        assert_eq!(records, vec![record]);

        let _ = fs::remove_dir_all(store_dir);
    }
}
