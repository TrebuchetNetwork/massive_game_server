use super::phone_utils::{phone_hash_from_stored_or_legacy_value, phone_last4, unix_now};
use super::types::{AuthInner, AuthRedisCache, PersistentAuthStore};
use super::{ACTIVE_PHONE_HASH_PREFIX, DEFAULT_REDIS_STORE_KEY, DELETED_PHONE_HASH_PREFIX};
use parking_lot::Mutex as ParkingLotMutex;
use redis::Commands;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info, warn};

pub(super) fn load_persistent_store(
    path: &Path,
    redis_cache: Option<&AuthRedisCache>,
) -> PersistentAuthStore {
    if let Some(cache) = redis_cache {
        if let Some(store) = cache.load_store() {
            return migrate_persistent_store(store);
        }
    }

    let raw = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => return PersistentAuthStore::default(),
    };
    match serde_json::from_str::<PersistentAuthStore>(&raw) {
        Ok(store) => migrate_persistent_store(store),
        Err(error) => {
            error!(
                "Failed to parse auth store '{}': {}. Starting with empty store.",
                path.display(),
                error
            );
            PersistentAuthStore::default()
        }
    }
}

pub(super) fn migrate_persistent_store(mut store: PersistentAuthStore) -> PersistentAuthStore {
    let mut rebuilt_phone_to_user = HashMap::with_capacity(store.users.len());

    for (user_id, user) in &mut store.users {
        let original_phone = user.phone_number.clone();
        if user.deleted {
            let deleted_hash = if let Some(existing_hash) =
                original_phone.strip_prefix(DELETED_PHONE_HASH_PREFIX)
            {
                existing_hash.to_owned()
            } else {
                phone_hash_from_stored_or_legacy_value(&original_phone)
            };
            user.phone_number = format!("{}{}", DELETED_PHONE_HASH_PREFIX, deleted_hash);
            if user.phone_last4.is_empty() {
                user.phone_last4 = "0000".to_owned();
            }
            rebuilt_phone_to_user.insert(user.phone_number.clone(), user_id.clone());
        } else {
            if user.phone_last4.is_empty()
                && !original_phone.starts_with(ACTIVE_PHONE_HASH_PREFIX)
                && !original_phone.starts_with(DELETED_PHONE_HASH_PREFIX)
            {
                user.phone_last4 = phone_last4(&original_phone);
            }
            let active_hash = phone_hash_from_stored_or_legacy_value(&original_phone);
            user.phone_number = format!("{}{}", ACTIVE_PHONE_HASH_PREFIX, active_hash);
            rebuilt_phone_to_user.insert(user.phone_number.clone(), user_id.clone());
        }
    }

    let active_user_ids: HashSet<String> = store
        .users
        .iter()
        .filter_map(|(user_id, user)| (!user.deleted).then_some(user_id.clone()))
        .collect();
    store
        .pending_deletions
        .retain(|user_id, _| active_user_ids.contains(user_id));
    store.phone_to_user_id = rebuilt_phone_to_user;
    store
}

/// Offloads auth store persistence (file I/O + Redis) to a blocking thread
/// so that tokio worker threads are not stalled.
/// Falls back to synchronous persistence when no tokio runtime is
/// available (e.g. in unit tests).
pub(super) fn spawn_persist_auth_store(
    path: PathBuf,
    store: PersistentAuthStore,
    inner: Arc<AuthInner>,
) {
    let do_persist = move || {
        persist_persistent_store(&path, &store, inner.redis_cache.as_ref());
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::spawn_blocking(do_persist);
    } else {
        do_persist();
    }
}

fn persist_persistent_store(
    path: &Path,
    store: &PersistentAuthStore,
    redis_cache: Option<&AuthRedisCache>,
) {
    if let Some(cache) = redis_cache {
        cache.persist_store(store);
    }

    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            error!(
                "Failed to create auth store directory '{}': {}",
                parent.display(),
                error
            );
            return;
        }
    }
    let serialized = match serde_json::to_string_pretty(store) {
        Ok(serialized) => serialized,
        Err(error) => {
            error!("Failed to serialize auth store: {}", error);
            return;
        }
    };
    let tmp_path = path.with_extension(format!("tmp-{}-{}", std::process::id(), unix_now()));
    if let Err(error) = fs::write(&tmp_path, serialized) {
        error!(
            "Failed to write auth store temp file '{}': {}",
            tmp_path.display(),
            error
        );
        return;
    }
    if let Err(error) = fs::rename(&tmp_path, path) {
        error!(
            "Failed to atomically replace auth store '{}' from '{}': {}",
            path.display(),
            tmp_path.display(),
            error
        );
        let _ = fs::remove_file(&tmp_path);
    }
}

// ── Redis cache implementation ────────────────────────────────────────────────

impl AuthRedisCache {
    pub(super) fn with_connection<T>(
        &self,
        mut operation: impl FnMut(&mut redis::Connection) -> redis::RedisResult<T>,
    ) -> redis::RedisResult<T> {
        let mut guard = self.connection.lock();
        if guard.is_none() {
            *guard = Some(self.client.get_connection()?);
        }

        match operation(guard.as_mut().expect("redis connection initialized")) {
            Ok(value) => Ok(value),
            Err(first_error) => {
                *guard = None;
                let mut reconnected = self.client.get_connection()?;
                let retry_result = operation(&mut reconnected);
                *guard = Some(reconnected);
                retry_result.or(Err(first_error))
            }
        }
    }

    pub(super) fn load_store(&self) -> Option<PersistentAuthStore> {
        let raw: Option<String> =
            match self.with_connection(|connection| connection.get(&self.store_key)) {
                Ok(value) => value,
                Err(error) => {
                    warn!("Failed to fetch auth store from Redis: {}", error);
                    return None;
                }
            };
        let payload = raw?;
        match serde_json::from_str::<PersistentAuthStore>(&payload) {
            Ok(store) => {
                info!(
                    "Loaded auth store from Redis key '{}' (users={}).",
                    self.store_key,
                    store.users.len()
                );
                Some(store)
            }
            Err(error) => {
                warn!(
                    "Failed to parse auth store from Redis key '{}': {}",
                    self.store_key, error
                );
                None
            }
        }
    }

    pub(super) fn persist_store(&self, store: &PersistentAuthStore) {
        let serialized = match serde_json::to_string(store) {
            Ok(value) => value,
            Err(error) => {
                error!("Failed to serialize auth store for Redis: {}", error);
                return;
            }
        };

        let result: redis::RedisResult<()> =
            self.with_connection(|connection| connection.set(&self.store_key, serialized.clone()));
        if let Err(error) = result {
            warn!(
                "Failed to persist auth store to Redis key '{}': {}",
                self.store_key, error
            );
        }
    }
}

/// Redact the password portion of a URL for safe logging.
/// Turns `redis://user:secret@host` into `redis://user:***@host`.
fn redact_url_password(url: &str) -> String {
    // Match ://user:password@ or just ://:password@ or ://password@
    // We look for :// then everything up to @ and redact the password part.
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(at_pos) = after_scheme.find('@') {
            let userinfo = &after_scheme[..at_pos];
            let rest = &after_scheme[at_pos..]; // includes the '@'
            if let Some(colon_pos) = userinfo.find(':') {
                let user = &userinfo[..colon_pos];
                return format!("{}://{}:***{}", &url[..scheme_end], user, rest);
            }
        }
    }
    url.to_owned()
}

pub(super) fn init_redis_cache(
    redis_url_raw: Option<&str>,
    store_key_raw: Option<&str>,
) -> Option<AuthRedisCache> {
    let redis_url = redis_url_raw
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(str::to_owned)?;
    let safe_url = redact_url_password(&redis_url);
    let store_key = store_key_raw
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| DEFAULT_REDIS_STORE_KEY.to_owned());

    let client = match redis::Client::open(redis_url.clone()) {
        Ok(client) => client,
        Err(error) => {
            warn!(
                "Redis auth cache disabled: invalid MGS_REDIS_URL '{}': {}",
                safe_url, error
            );
            return None;
        }
    };

    let connection = match client.get_connection() {
        Ok(connection) => connection,
        Err(error) => {
            warn!(
                "Redis auth cache disabled: unable to connect to '{}': {}",
                safe_url, error
            );
            return None;
        }
    };

    info!(
        "Redis auth cache enabled. url='{}', key='{}'",
        safe_url, store_key
    );
    Some(AuthRedisCache {
        client,
        connection: ParkingLotMutex::new(Some(connection)),
        store_key,
    })
}
