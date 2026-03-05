use parking_lot::RwLock;
use seahash::SeaHasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::hash::Hasher;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use tracing::{info, warn};
use uuid::Uuid;
use warp::{Filter, Reply};

#[derive(Clone)]
pub struct FeatureFlagService {
    inner: Arc<FeatureFlagInner>,
}

struct FeatureFlagInner {
    store_path: PathBuf,
    flags: RwLock<HashMap<String, FeatureFlagRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FeatureFlagRecord {
    key: String,
    enabled: bool,
    rollout_percentage: u8,
    description: Option<String>,
    updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureFlagView {
    pub key: String,
    pub enabled: bool,
    pub rollout_percentage: u8,
    pub description: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct SetFlagBody {
    key: String,
    enabled: bool,
    rollout_percentage: Option<u8>,
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct EvaluateFlagBody {
    key: String,
    subject: String,
}

#[derive(Debug, Clone, Serialize)]
struct EvaluateFlagResponse {
    key: String,
    subject: String,
    enabled_for_subject: bool,
    rollout_percentage: u8,
}

#[derive(Debug, Clone, Serialize)]
struct ApiErrorBody {
    code: &'static str,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct ApiResponse<T>
where
    T: Serialize,
{
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ApiErrorBody>,
}

#[derive(Debug)]
enum FlagError {
    InvalidInput(&'static str, String),
    NotFound(&'static str, String),
    Internal(String),
}

impl FlagError {
    fn code(&self) -> &'static str {
        match self {
            FlagError::InvalidInput(code, _) => code,
            FlagError::NotFound(code, _) => code,
            FlagError::Internal(_) => "internal_error",
        }
    }

    fn message(&self) -> String {
        match self {
            FlagError::InvalidInput(_, message)
            | FlagError::NotFound(_, message)
            | FlagError::Internal(message) => message.clone(),
        }
    }
}

impl FeatureFlagService {
    pub fn new_from_env() -> Self {
        let store_path = std::env::var("MGS_FEATURE_FLAG_STORE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/feature_flags.json"));
        let mut flags = load_store(&store_path);

        for (key, enabled) in parse_flags_from_env() {
            let now = unix_now();
            let entry = flags
                .entry(key.clone())
                .or_insert_with(|| FeatureFlagRecord {
                    key: key.clone(),
                    enabled,
                    rollout_percentage: if enabled { 100 } else { 0 },
                    description: Some("Loaded from MGS_FEATURE_FLAGS".to_owned()),
                    updated_at: now,
                });
            entry.enabled = enabled;
            entry.rollout_percentage = if enabled { 100 } else { 0 };
            entry.updated_at = now;
        }

        info!(
            "Feature flag service initialized. store_path='{}', flags={}",
            store_path.display(),
            flags.len()
        );

        Self {
            inner: Arc::new(FeatureFlagInner {
                store_path,
                flags: RwLock::new(flags),
            }),
        }
    }

    fn list_flags(&self) -> Vec<FeatureFlagView> {
        let flags = self.inner.flags.read();
        let mut values: Vec<FeatureFlagView> = flags.values().map(to_view).collect();
        values.sort_by(|left, right| left.key.cmp(&right.key));
        values
    }

    fn set_flag(&self, body: SetFlagBody) -> Result<FeatureFlagView, FlagError> {
        let key = body.key.trim();
        if key.is_empty() {
            return Err(FlagError::InvalidInput(
                "invalid_flag_key",
                "Flag key cannot be empty".to_owned(),
            ));
        }

        let rollout_percentage = body
            .rollout_percentage
            .unwrap_or(if body.enabled { 100 } else { 0 })
            .min(100);
        let now = unix_now();

        {
            let mut flags = self.inner.flags.write();
            let record = flags
                .entry(key.to_owned())
                .or_insert_with(|| FeatureFlagRecord {
                    key: key.to_owned(),
                    enabled: body.enabled,
                    rollout_percentage,
                    description: None,
                    updated_at: now,
                });
            record.enabled = body.enabled;
            record.rollout_percentage = rollout_percentage;
            record.description = body.description;
            record.updated_at = now;
        }

        self.spawn_persist_store();

        let flags = self.inner.flags.read();
        flags
            .get(key)
            .map(to_view)
            .ok_or_else(|| FlagError::Internal("flag not found after update".to_owned()))
    }

    fn evaluate_flag(&self, body: EvaluateFlagBody) -> Result<EvaluateFlagResponse, FlagError> {
        let key = body.key.trim();
        let subject = body.subject.trim();
        if key.is_empty() || subject.is_empty() {
            return Err(FlagError::InvalidInput(
                "invalid_flag_eval",
                "key and subject are required".to_owned(),
            ));
        }

        let flags = self.inner.flags.read();
        let Some(flag) = flags.get(key) else {
            return Err(FlagError::NotFound(
                "flag_not_found",
                format!("flag '{}' does not exist", key),
            ));
        };

        let enabled_for_subject = if !flag.enabled {
            false
        } else if flag.rollout_percentage >= 100 {
            true
        } else if flag.rollout_percentage == 0 {
            false
        } else {
            is_subject_in_rollout(key, subject, flag.rollout_percentage)
        };

        Ok(EvaluateFlagResponse {
            key: key.to_owned(),
            subject: subject.to_owned(),
            enabled_for_subject,
            rollout_percentage: flag.rollout_percentage,
        })
    }

    /// Offloads flag store persistence to a blocking thread so that
    /// tokio worker threads are not stalled by file I/O.
    /// Falls back to synchronous persistence when no tokio runtime is
    /// available (e.g. in unit tests).
    fn spawn_persist_store(&self) {
        let snapshot = self.inner.flags.read().clone();
        let path = self.inner.store_path.clone();
        let do_persist = move || {
            if let Err(err) = persist_store(&path, &snapshot) {
                warn!("Background flag persist failed: {}", err);
            }
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::spawn_blocking(do_persist);
        } else {
            do_persist();
        }
    }
}

fn to_view(record: &FeatureFlagRecord) -> FeatureFlagView {
    FeatureFlagView {
        key: record.key.clone(),
        enabled: record.enabled,
        rollout_percentage: record.rollout_percentage,
        description: record.description.clone(),
        updated_at: record.updated_at,
    }
}

fn is_subject_in_rollout(key: &str, subject: &str, rollout_percentage: u8) -> bool {
    let mut hasher = SeaHasher::new();
    hasher.write(key.as_bytes());
    hasher.write_u8(0x1f);
    hasher.write(subject.as_bytes());
    let bucket = (hasher.finish() % 100) as u8;
    bucket < rollout_percentage
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn load_store(path: &Path) -> HashMap<String, FeatureFlagRecord> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    "Failed to read feature flag store '{}': {}",
                    path.display(),
                    err
                );
            }
            return HashMap::new();
        }
    };

    serde_json::from_str(&raw).unwrap_or_else(|err| {
        warn!(
            "Failed to parse feature flag store '{}': {}. Starting empty.",
            path.display(),
            err
        );
        HashMap::new()
    })
}

fn persist_store(path: &Path, flags: &HashMap<String, FeatureFlagRecord>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create '{}': {}", parent.display(), err))?;
    }
    let serialized = serde_json::to_string_pretty(flags)
        .map_err(|err| format!("failed to serialize flags: {}", err))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("feature_flags.json");
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
    })
}

fn parse_flags_from_env() -> Vec<(String, bool)> {
    let raw = match std::env::var("MGS_FEATURE_FLAGS") {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };

    raw.split(',')
        .filter_map(|entry| {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                return None;
            }
            let mut parts = trimmed.splitn(2, '=');
            let key = parts.next()?.trim();
            if key.is_empty() {
                return None;
            }
            let enabled = parts
                .next()
                .map(|value| {
                    let normalized = value.trim().to_ascii_lowercase();
                    normalized == "1"
                        || normalized == "true"
                        || normalized == "on"
                        || normalized == "yes"
                })
                .unwrap_or(true);
            Some((key.to_owned(), enabled))
        })
        .collect()
}

fn ok_response<T>(data: T) -> warp::reply::Json
where
    T: Serialize,
{
    warp::reply::json(&ApiResponse {
        ok: true,
        data: Some(data),
        error: None::<ApiErrorBody>,
    })
}

fn error_response(code: &'static str, message: String) -> warp::reply::Json {
    warp::reply::json(&ApiResponse::<serde_json::Value> {
        ok: false,
        data: None,
        error: Some(ApiErrorBody { code, message }),
    })
}

fn with_service(
    service: FeatureFlagService,
) -> impl Filter<Extract = (FeatureFlagService,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || service.clone())
}

fn inline_admin_expected_token() -> Option<&'static String> {
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            std::env::var("MGS_ADMIN_BEARER_TOKEN")
                .or_else(|_| std::env::var("MGS_ADMIN_TOKEN"))
                .ok()
                .map(|raw| raw.trim().to_owned())
                .filter(|raw| !raw.is_empty())
        })
        .as_ref()
}

fn parse_bearer_token(authorization_header: Option<&str>) -> Option<&str> {
    let raw = authorization_header?.trim();
    if raw.is_empty() {
        return None;
    }
    raw.strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    left_bytes.len() == right_bytes.len() && left_bytes.ct_eq(right_bytes).into()
}

fn inline_admin_authorized(authorization_header: Option<&str>) -> bool {
    let Some(expected) = inline_admin_expected_token() else {
        return false;
    };
    let Some(provided) = parse_bearer_token(authorization_header) else {
        return false;
    };
    constant_time_eq(expected.as_str(), provided)
}

pub fn build_feature_flag_routes(
    service: FeatureFlagService,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    // 64 KB body limit for all JSON endpoints to prevent resource exhaustion
    let json_body_limit = 1024 * 64;

    let list = warp::path!("api" / "ops" / "feature-flags")
        .and(warp::get())
        .and(warp::header::optional::<String>("authorization"))
        .and(with_service(service.clone()))
        .map(|authorization: Option<String>, flags: FeatureFlagService| {
            if !inline_admin_authorized(authorization.as_deref()) {
                return error_response(
                    "admin_auth_required",
                    "Admin bearer token required.".to_owned(),
                );
            }
            ok_response(flags.list_flags())
        });

    let set = warp::path!("api" / "ops" / "feature-flags" / "set")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: SetFlagBody, flags: FeatureFlagService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match flags.set_flag(body) {
                    Ok(result) => ok_response(result),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let evaluate = warp::path!("api" / "ops" / "feature-flags" / "evaluate")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service))
        .map(
            |authorization: Option<String>, body: EvaluateFlagBody, flags: FeatureFlagService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match flags.evaluate_flag(body) {
                    Ok(result) => ok_response(result),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    list.or(set).or(evaluate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollout_is_deterministic_for_subject() {
        let value_a = is_subject_in_rollout("fx", "user-1", 30);
        let value_b = is_subject_in_rollout("fx", "user-1", 30);
        assert_eq!(value_a, value_b);
    }

    #[test]
    fn env_flags_parse_boolean_values() {
        temp_env::with_var(
            "MGS_FEATURE_FLAGS",
            Some("a=1,b=0,c=true,d=false,e"),
            || {
                let entries = parse_flags_from_env();
                let map: HashMap<String, bool> = entries.into_iter().collect();
                assert_eq!(map.get("a"), Some(&true));
                assert_eq!(map.get("b"), Some(&false));
                assert_eq!(map.get("c"), Some(&true));
                assert_eq!(map.get("d"), Some(&false));
                assert_eq!(map.get("e"), Some(&true));
            },
        );
    }
}
