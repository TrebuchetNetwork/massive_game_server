use super::types::{ArenaRedisStore, PersistentArenaStore};
use redis::Commands;
use std::fs;
use std::io::Write;
use std::path::Path;
use tracing::warn;
use uuid::Uuid;

impl ArenaRedisStore {
    pub(super) fn load_store(&self) -> Result<Option<PersistentArenaStore>, String> {
        let mut connection = self
            .client
            .get_connection_with_timeout(std::time::Duration::from_secs(2))
            .map_err(|err| format!("failed to connect to Redis: {}", err))?;
        let raw: Option<String> = connection
            .get(&self.key)
            .map_err(|err| format!("failed to read Redis key '{}': {}", self.key, err))?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        serde_json::from_str(&raw)
            .map(Some)
            .map_err(|err| format!("failed to parse Redis arena store '{}': {}", self.key, err))
    }

    pub(super) fn persist_store(&self, store: &PersistentArenaStore) -> Result<(), String> {
        let serialized = serde_json::to_string_pretty(store)
            .map_err(|err| format!("failed to serialize arena store for Redis: {}", err))?;
        let mut connection = self
            .client
            .get_connection_with_timeout(std::time::Duration::from_secs(2))
            .map_err(|err| format!("failed to connect to Redis: {}", err))?;
        connection
            .set::<_, _, ()>(&self.key, serialized)
            .map_err(|err| format!("failed to persist Redis key '{}': {}", self.key, err))
    }
}

pub(super) fn init_redis_store(
    redis_url_raw: Option<String>,
    redis_store_key: String,
) -> Option<ArenaRedisStore> {
    let redis_url = redis_url_raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let redis_store_key = redis_store_key.trim();
    if redis_store_key.is_empty() {
        return None;
    }
    let client = match redis::Client::open(redis_url.to_owned()) {
        Ok(client) => client,
        Err(err) => {
            warn!("Failed to configure Redis-backed arena store: {}", err);
            return None;
        }
    };
    Some(ArenaRedisStore {
        client,
        key: redis_store_key.to_owned(),
    })
}

pub(super) fn load_persistent_store(
    path: &Path,
    redis_store: Option<&ArenaRedisStore>,
) -> PersistentArenaStore {
    if let Some(redis_store) = redis_store {
        match redis_store.load_store() {
            Ok(Some(store)) => return store,
            Ok(None) => {}
            Err(err) => warn!("Failed to load Redis-backed arena store: {}", err),
        }
    }

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to read arena store '{}': {}", path.display(), err);
            }
            return PersistentArenaStore::default();
        }
    };

    serde_json::from_str(&raw).unwrap_or_else(|err| {
        warn!(
            "Failed to parse arena store '{}': {}. Starting with empty arena store.",
            path.display(),
            err
        );
        PersistentArenaStore::default()
    })
}

pub(super) fn persist_store(
    path: &Path,
    store: &PersistentArenaStore,
    redis_store: Option<&ArenaRedisStore>,
) -> Result<(), String> {
    if let Some(redis_store) = redis_store {
        redis_store.persist_store(store)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create '{}': {}", parent.display(), err))?;
    }
    let serialized = serde_json::to_string_pretty(store)
        .map_err(|err| format!("failed to serialize arena store: {}", err))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("arena_store.json");
    let tmp_path = path.with_file_name(format!(".{}.{}.tmp", file_name, Uuid::new_v4().simple()));
    let mut tmp_file = fs::File::create(&tmp_path)
        .map_err(|err| format!("failed to create '{}': {}", tmp_path.display(), err))?;
    tmp_file
        .write_all(serialized.as_bytes())
        .map_err(|err| format!("failed to write '{}': {}", tmp_path.display(), err))?;
    tmp_file
        .sync_all()
        .map_err(|err| format!("failed to fsync '{}': {}", tmp_path.display(), err))?;
    fs::rename(&tmp_path, path).map_err(|err| {
        format!(
            "failed to atomically replace '{}' with '{}': {}",
            path.display(),
            tmp_path.display(),
            err
        )
    })?;
    Ok(())
}
