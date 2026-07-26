use serde::{Deserialize, Serialize};

pub const EPOCH_REDIS_KEY: &str = "world:epoch";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchEpoch {
    pub match_id: u64,
    pub starts_at_ms: i64,
    pub ends_at_ms: i64,
}

impl MatchEpoch {
    pub fn new(match_id: u64, starts_at_ms: i64, duration_secs: f32) -> Self {
        Self {
            match_id,
            starts_at_ms,
            ends_at_ms: starts_at_ms + (duration_secs * 1000.0) as i64,
        }
    }

    pub fn time_remaining_secs(&self, now_ms: i64) -> f32 {
        ((self.ends_at_ms - now_ms).max(0)) as f32 / 1000.0
    }
}

/// First writer wins (SETNX); warns and skips when Redis is unavailable.
pub fn publish_epoch(redis_url: &str, epoch: &MatchEpoch) {
    let publish = || -> Result<(), String> {
        let client = redis::Client::open(redis_url.to_owned()).map_err(|e| e.to_string())?;
        let mut conn = client
            .get_connection_with_timeout(std::time::Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let json = serde_json::to_string(epoch).map_err(|e| e.to_string())?;
        redis::cmd("SET")
            .arg(EPOCH_REDIS_KEY)
            .arg(json)
            .arg("NX")
            .query::<Option<()>>(&mut conn)
            .map_err(|e| e.to_string())?;
        Ok(())
    };
    if let Err(err) = publish() {
        tracing::warn!("match epoch Redis publish skipped: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_computes_end_from_duration() {
        let epoch = MatchEpoch::new(7, 1_000_000, 237.0);
        assert_eq!(epoch.match_id, 7);
        assert_eq!(epoch.ends_at_ms, 1_000_000 + 237_000);
        assert!((epoch.time_remaining_secs(1_100_000) - 137.0).abs() < 0.01);
        assert_eq!(epoch.time_remaining_secs(2_000_000), 0.0);
    }
}
