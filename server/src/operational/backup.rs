// massive_game_server/server/src/operational/backup.rs

use crate::operational::config::env_registry::BackupEnv;
use crate::operational::monitoring::metrics;
use redis::Commands;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tracing::{info, warn};

#[derive(Clone)]
pub struct BackupManager {
    inner: Arc<BackupConfig>,
}

#[derive(Debug)]
struct BackupConfig {
    enabled: bool,
    interval_seconds: u64,
    output_dir: PathBuf,
    retention_count: usize,
    redis_url: Option<String>,
    redis_store_key: String,
    sources: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupManifest {
    created_at_unix: u64,
    reason: String,
    copied_files: Vec<BackupCopiedFile>,
    missing_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupCopiedFile {
    source: String,
    backup_path: String,
    size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadataRecord {
    pub backup_dir: String,
    pub created_at_unix: u64,
    pub reason: String,
    pub copied_files: Vec<BackupCopiedFile>,
    pub missing_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupRestoreResult {
    pub backup_dir: String,
    pub created_at_unix: u64,
    pub restored_files: Vec<BackupRestoredFile>,
    pub missing_backup_files: Vec<String>,
    pub missing_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupRestoredFile {
    pub source: String,
    pub restored_from: String,
    pub size_bytes: u64,
}

impl BackupManager {
    pub fn from_env_config(env: &BackupEnv) -> Self {
        let mut sources = vec![
            PathBuf::from(env.auth_store_path.as_str()),
            PathBuf::from(env.feature_flags_store_path.as_str()),
            PathBuf::from(env.arena_store_path.as_str()),
            PathBuf::from(env.live_replay_dispute_store_path.as_str()),
        ];
        for path in &env.extra_paths {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                sources.push(PathBuf::from(trimmed));
            }
        }
        sources.sort();
        sources.dedup();

        Self {
            inner: Arc::new(BackupConfig {
                enabled: env.enabled,
                interval_seconds: env.interval_seconds.max(1),
                output_dir: PathBuf::from(env.output_dir.as_str()),
                retention_count: env.retention_count.max(1),
                redis_url: env.redis_url.clone(),
                redis_store_key: env.redis_store_key.clone(),
                sources,
            }),
        }
    }

    pub fn from_env() -> Self {
        let enabled = parse_bool_env("MGS_BACKUP_ENABLED", false);
        let interval_seconds = std::env::var("MGS_BACKUP_INTERVAL_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(3600);
        let output_dir = std::env::var("MGS_BACKUP_DIR")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/backups"));
        let retention_count = std::env::var("MGS_BACKUP_RETENTION_COUNT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(48);
        let redis_url = std::env::var("MGS_BACKUP_REDIS_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                std::env::var("MGS_REDIS_URL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            });
        let redis_store_key = std::env::var("MGS_REDIS_BACKUP_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "mgs:backup:latest".to_owned());

        let mut sources = vec![
            env_path_or_default("MGS_AUTH_STORE_PATH", "data/auth_store.json"),
            env_path_or_default("MGS_FEATURE_FLAGS_STORE_PATH", "data/feature_flags.json"),
            env_path_or_default("MGS_ARENA_STORE_PATH", "data/arena_store.json"),
            env_path_or_default(
                "MGS_LIVE_REPLAY_DISPUTE_STORE_PATH",
                "data/live_replay_disputes.jsonl",
            ),
        ];
        if let Ok(extra_paths) = std::env::var("MGS_BACKUP_EXTRA_PATHS") {
            for raw_path in extra_paths
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                sources.push(PathBuf::from(raw_path));
            }
        }
        sources.sort();
        sources.dedup();

        Self {
            inner: Arc::new(BackupConfig {
                enabled,
                interval_seconds,
                output_dir,
                retention_count,
                redis_url,
                redis_store_key,
                sources,
            }),
        }
    }

    pub fn enabled(&self) -> bool {
        self.inner.enabled
    }

    pub fn interval_seconds(&self) -> u64 {
        self.inner.interval_seconds.max(1)
    }

    pub async fn run_once(&self, reason: &str) -> Result<(), String> {
        if !self.enabled() {
            return Ok(());
        }
        let started_at = Instant::now();
        let result = self.run_once_inner(reason).await;
        match result {
            Ok(()) => {
                metrics::record_backup_result("success", started_at.elapsed().as_secs_f64());
                Ok(())
            }
            Err(err) => {
                metrics::record_backup_result("failure", started_at.elapsed().as_secs_f64());
                Err(err)
            }
        }
    }

    pub async fn restore_from_backup(
        &self,
        backup_dir_name: Option<&str>,
    ) -> Result<BackupRestoreResult, String> {
        let started_at = Instant::now();
        let result = self.restore_from_backup_inner(backup_dir_name).await;
        match result {
            Ok(summary) => {
                metrics::record_backup_result(
                    "restore_success",
                    started_at.elapsed().as_secs_f64(),
                );
                Ok(summary)
            }
            Err(err) => {
                metrics::record_backup_result(
                    "restore_failure",
                    started_at.elapsed().as_secs_f64(),
                );
                Err(err)
            }
        }
    }

    pub async fn restore_latest_backup(&self) -> Result<BackupRestoreResult, String> {
        self.restore_from_backup(None).await
    }

    pub async fn latest_backup_metadata(&self) -> Result<Option<BackupMetadataRecord>, String> {
        if let Some(redis_url) = self.inner.redis_url.clone() {
            let redis_key = self.inner.redis_store_key.clone();
            match tokio::task::spawn_blocking(move || {
                load_latest_backup_metadata_from_redis(redis_url.as_str(), redis_key.as_str())
            })
            .await
            .map_err(|err| format!("backup metadata redis task join failed: {}", err))?
            {
                Ok(Some(metadata)) => return Ok(Some(metadata)),
                Ok(None) => {}
                Err(err) => warn!("failed to load latest backup metadata from redis: {}", err),
            }
        }
        load_latest_backup_metadata_from_fs(&self.inner.output_dir).await
    }

    async fn run_once_inner(&self, reason: &str) -> Result<(), String> {
        let created_at_unix = unix_now();
        let backup_root =
            create_unique_backup_root(&self.inner.output_dir, created_at_unix).await?;

        let mut manifest = BackupManifest {
            created_at_unix,
            reason: reason.to_owned(),
            copied_files: Vec::new(),
            missing_sources: Vec::new(),
        };

        for source_path in &self.inner.sources {
            if !source_path.exists() {
                manifest
                    .missing_sources
                    .push(source_path.to_string_lossy().to_string());
                continue;
            }

            let target_file_name = backup_name_for_path(source_path);
            let target_path = backup_root.join(target_file_name);
            fs::copy(source_path, &target_path).await.map_err(|err| {
                format!(
                    "failed to copy '{}' to '{}': {}",
                    source_path.display(),
                    target_path.display(),
                    err
                )
            })?;
            let size_bytes = fs::metadata(&target_path)
                .await
                .ok()
                .map(|meta| meta.len())
                .unwrap_or(0);
            manifest.copied_files.push(BackupCopiedFile {
                source: source_path.to_string_lossy().to_string(),
                backup_path: target_path.to_string_lossy().to_string(),
                size_bytes,
            });
        }

        let manifest_path = backup_root.join("manifest.json");
        let manifest_json = serde_json::to_vec_pretty(&manifest)
            .map_err(|err| format!("manifest json: {}", err))?;
        fs::write(&manifest_path, manifest_json)
            .await
            .map_err(|err| format!("failed to write backup manifest: {}", err))?;

        if let Some(redis_url) = self.inner.redis_url.clone() {
            let redis_key = self.inner.redis_store_key.clone();
            let latest_metadata = BackupMetadataRecord {
                backup_dir: backup_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                created_at_unix,
                reason: manifest.reason.clone(),
                copied_files: manifest.copied_files.clone(),
                missing_sources: manifest.missing_sources.clone(),
            };
            match tokio::task::spawn_blocking(move || {
                persist_latest_backup_metadata_to_redis(
                    redis_url.as_str(),
                    redis_key.as_str(),
                    &latest_metadata,
                )
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(err)) => warn!("failed to persist latest backup metadata to redis: {}", err),
                Err(err) => warn!("backup metadata redis task join failed: {}", err),
            }
        }

        if let Err(err) = self.prune_old_backups().await {
            warn!("Backup pruning failed: {}", err);
        }

        info!(
            "Backup completed (reason='{}', files={}, missing={}, dir='{}').",
            reason,
            manifest.copied_files.len(),
            manifest.missing_sources.len(),
            backup_root.display()
        );
        Ok(())
    }

    async fn restore_from_backup_inner(
        &self,
        backup_dir_name: Option<&str>,
    ) -> Result<BackupRestoreResult, String> {
        let backup_root = self.resolve_backup_root(backup_dir_name).await?;
        let canonical_backup_root = fs::canonicalize(&backup_root).await.map_err(|err| {
            format!(
                "failed to canonicalize backup root '{}': {}",
                backup_root.display(),
                err
            )
        })?;
        let manifest_path = backup_root.join("manifest.json");
        let manifest_raw = fs::read(&manifest_path).await.map_err(|err| {
            format!(
                "failed to read backup manifest '{}': {}",
                manifest_path.display(),
                err
            )
        })?;
        let manifest: BackupManifest = serde_json::from_slice(&manifest_raw).map_err(|err| {
            format!(
                "invalid backup manifest '{}': {}",
                manifest_path.display(),
                err
            )
        })?;

        let allowed_sources: HashSet<String> = self
            .inner
            .sources
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();

        let mut restored_files = Vec::new();
        let mut missing_backup_files = Vec::new();
        for copied_file in &manifest.copied_files {
            if !allowed_sources.contains(&copied_file.source) {
                return Err(format!(
                    "backup manifest contains unexpected source '{}'",
                    copied_file.source
                ));
            }
            let source_path = PathBuf::from(&copied_file.source);
            let backup_path = resolve_backup_file_path(&canonical_backup_root, copied_file);
            let backup_meta = match fs::symlink_metadata(&backup_path).await {
                Ok(meta) => meta,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    missing_backup_files.push(backup_path.to_string_lossy().to_string());
                    continue;
                }
                Err(err) => {
                    return Err(format!(
                        "failed to stat backup file '{}': {}",
                        backup_path.display(),
                        err
                    ));
                }
            };
            if backup_meta.file_type().is_symlink() {
                return Err(format!(
                    "backup file '{}' is a symlink, refusing restore",
                    backup_path.display()
                ));
            }
            if !backup_meta.is_file() {
                return Err(format!(
                    "backup path '{}' is not a regular file",
                    backup_path.display()
                ));
            }
            let canonical_backup_path = fs::canonicalize(&backup_path).await.map_err(|err| {
                format!(
                    "failed to canonicalize backup file '{}': {}",
                    backup_path.display(),
                    err
                )
            })?;
            if !canonical_backup_path.starts_with(&canonical_backup_root) {
                return Err(format!(
                    "backup path '{}' resolves outside '{}'",
                    canonical_backup_path.display(),
                    canonical_backup_root.display()
                ));
            }

            if let Some(parent) = source_path.parent() {
                fs::create_dir_all(parent).await.map_err(|err| {
                    format!(
                        "failed creating destination directory '{}' during restore: {}",
                        parent.display(),
                        err
                    )
                })?;
            }

            fs::copy(&canonical_backup_path, &source_path)
                .await
                .map_err(|err| {
                    format!(
                        "failed restoring '{}' from '{}': {}",
                        source_path.display(),
                        canonical_backup_path.display(),
                        err
                    )
                })?;

            let size_bytes = fs::metadata(&source_path)
                .await
                .ok()
                .map(|meta| meta.len())
                .unwrap_or(copied_file.size_bytes);
            restored_files.push(BackupRestoredFile {
                source: source_path.to_string_lossy().to_string(),
                restored_from: canonical_backup_path.to_string_lossy().to_string(),
                size_bytes,
            });
        }

        let backup_dir = backup_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        info!(
            "Backup restore completed (backup='{}', restored={}, missing_backup_files={}).",
            backup_dir,
            restored_files.len(),
            missing_backup_files.len()
        );
        Ok(BackupRestoreResult {
            backup_dir,
            created_at_unix: manifest.created_at_unix,
            restored_files,
            missing_backup_files,
            missing_sources: manifest.missing_sources,
        })
    }

    async fn resolve_backup_root(&self, backup_dir_name: Option<&str>) -> Result<PathBuf, String> {
        let canonical_output_dir =
            fs::canonicalize(&self.inner.output_dir)
                .await
                .map_err(|err| {
                    format!(
                        "backup root '{}' is not accessible: {}",
                        self.inner.output_dir.display(),
                        err
                    )
                })?;

        if let Some(backup_dir_name) = backup_dir_name {
            let trimmed = backup_dir_name.trim();
            if trimmed.is_empty() {
                return Err("backup directory name cannot be empty".to_owned());
            }
            if !is_safe_backup_dir_name(trimmed) {
                return Err(format!("invalid backup directory name '{}'", trimmed));
            }

            let explicit = canonical_output_dir.join(trimmed);
            let canonical_explicit = fs::canonicalize(&explicit).await.map_err(|err| {
                format!(
                    "backup directory '{}' does not exist under '{}': {}",
                    trimmed,
                    self.inner.output_dir.display(),
                    err
                )
            })?;
            if !canonical_explicit.starts_with(&canonical_output_dir) {
                return Err(format!(
                    "backup directory '{}' resolves outside backup root '{}'",
                    trimmed,
                    self.inner.output_dir.display()
                ));
            }
            if canonical_explicit.is_dir() {
                return Ok(canonical_explicit);
            }
            return Err(format!(
                "backup directory '{}' does not exist under '{}'",
                trimmed,
                self.inner.output_dir.display()
            ));
        }

        let mut entries = fs::read_dir(&self.inner.output_dir)
            .await
            .map_err(|err| format!("read backup dir failed: {}", err))?;
        let mut backup_dirs: Vec<PathBuf> = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| format!("read backup entry failed: {}", err))?
        {
            let file_type = entry
                .file_type()
                .await
                .map_err(|err| format!("backup entry type failed: {}", err))?;
            if !file_type.is_dir() {
                continue;
            }
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with("backup-") {
                backup_dirs.push(entry.path());
            }
        }

        backup_dirs.sort();
        let selected = backup_dirs
            .pop()
            .ok_or_else(|| format!("no backups found in '{}'", self.inner.output_dir.display()))?;
        let canonical_selected = fs::canonicalize(&selected).await.map_err(|err| {
            format!(
                "failed to canonicalize backup directory '{}': {}",
                selected.display(),
                err
            )
        })?;
        if !canonical_selected.starts_with(&canonical_output_dir) {
            return Err(format!(
                "backup directory '{}' resolves outside backup root '{}'",
                canonical_selected.display(),
                canonical_output_dir.display()
            ));
        }
        Ok(canonical_selected)
    }

    async fn prune_old_backups(&self) -> Result<(), String> {
        let retention_count = self.inner.retention_count.max(1);
        let mut entries = fs::read_dir(&self.inner.output_dir)
            .await
            .map_err(|err| format!("read backup dir failed: {}", err))?;

        let mut backup_dirs: Vec<PathBuf> = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| format!("read backup entry failed: {}", err))?
        {
            let file_type = entry
                .file_type()
                .await
                .map_err(|err| format!("backup entry type failed: {}", err))?;
            if !file_type.is_dir() {
                continue;
            }
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with("backup-") {
                backup_dirs.push(entry.path());
            }
        }

        if backup_dirs.len() <= retention_count {
            return Ok(());
        }
        backup_dirs.sort();
        let to_delete = backup_dirs.len().saturating_sub(retention_count);
        for stale_backup in backup_dirs.into_iter().take(to_delete) {
            fs::remove_dir_all(&stale_backup)
                .await
                .map_err(|err| format!("failed to remove '{}': {}", stale_backup.display(), err))?;
        }
        Ok(())
    }
}

fn backup_name_for_path(path: &Path) -> String {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("backup.bin");
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    let path_hash = hasher.finish();
    format!("{:016x}__{}", path_hash, file_name)
}

fn is_safe_backup_dir_name(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    )
}

fn resolve_backup_file_path(backup_root: &Path, copied_file: &BackupCopiedFile) -> PathBuf {
    backup_root.join(backup_name_for_path(Path::new(&copied_file.source)))
}

fn latest_backup_metadata_redis_key(base_key: &str) -> &str {
    base_key
}

fn load_latest_backup_metadata_from_redis(
    redis_url: &str,
    redis_store_key: &str,
) -> Result<Option<BackupMetadataRecord>, String> {
    let trimmed_key = redis_store_key.trim();
    if trimmed_key.is_empty() {
        return Ok(None);
    }
    let client = redis::Client::open(redis_url.to_owned())
        .map_err(|err| format!("failed to configure Redis client '{}': {}", redis_url, err))?;
    let mut connection = client
        .get_connection()
        .map_err(|err| format!("failed to connect to Redis '{}': {}", redis_url, err))?;
    let payload: Option<String> = connection
        .get(latest_backup_metadata_redis_key(trimmed_key))
        .map_err(|err| format!("failed to read latest backup metadata from Redis: {}", err))?;
    payload
        .map(|raw| {
            serde_json::from_str::<BackupMetadataRecord>(&raw)
                .map_err(|err| format!("invalid latest backup metadata in Redis: {}", err))
        })
        .transpose()
}

fn persist_latest_backup_metadata_to_redis(
    redis_url: &str,
    redis_store_key: &str,
    metadata: &BackupMetadataRecord,
) -> Result<(), String> {
    let trimmed_key = redis_store_key.trim();
    if trimmed_key.is_empty() {
        return Ok(());
    }
    let payload = serde_json::to_string(metadata)
        .map_err(|err| format!("failed to encode latest backup metadata json: {}", err))?;
    let client = redis::Client::open(redis_url.to_owned())
        .map_err(|err| format!("failed to configure Redis client '{}': {}", redis_url, err))?;
    let mut connection = client
        .get_connection()
        .map_err(|err| format!("failed to connect to Redis '{}': {}", redis_url, err))?;
    connection
        .set::<_, _, ()>(latest_backup_metadata_redis_key(trimmed_key), payload)
        .map_err(|err| format!("failed to persist latest backup metadata to Redis: {}", err))
}

async fn load_latest_backup_metadata_from_fs(
    output_dir: &Path,
) -> Result<Option<BackupMetadataRecord>, String> {
    let backup_root = match latest_backup_root(output_dir).await? {
        Some(path) => path,
        None => return Ok(None),
    };
    let manifest_path = backup_root.join("manifest.json");
    let manifest_raw = fs::read(&manifest_path).await.map_err(|err| {
        format!(
            "failed to read backup manifest '{}': {}",
            manifest_path.display(),
            err
        )
    })?;
    let manifest: BackupManifest = serde_json::from_slice(&manifest_raw).map_err(|err| {
        format!(
            "invalid backup manifest '{}': {}",
            manifest_path.display(),
            err
        )
    })?;
    Ok(Some(BackupMetadataRecord {
        backup_dir: backup_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        created_at_unix: manifest.created_at_unix,
        reason: manifest.reason,
        copied_files: manifest.copied_files,
        missing_sources: manifest.missing_sources,
    }))
}

async fn latest_backup_root(output_dir: &Path) -> Result<Option<PathBuf>, String> {
    let mut entries = match fs::read_dir(output_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("read backup dir failed: {}", err)),
    };

    let mut backup_dirs: Vec<PathBuf> = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| format!("read backup entry failed: {}", err))?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(|err| format!("backup entry type failed: {}", err))?;
        if !file_type.is_dir() {
            continue;
        }
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with("backup-") {
            backup_dirs.push(entry.path());
        }
    }

    backup_dirs.sort();
    Ok(backup_dirs.pop())
}

async fn create_unique_backup_root(
    output_dir: &Path,
    created_at_unix: u64,
) -> Result<PathBuf, String> {
    fs::create_dir_all(output_dir)
        .await
        .map_err(|err| format!("failed to create backup output directory: {}", err))?;

    let epoch_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..32u8 {
        let backup_dir_name = format!(
            "backup-{}-{:x}-{:02x}",
            created_at_unix, epoch_nanos, attempt
        );
        let backup_root = output_dir.join(backup_dir_name);
        match fs::create_dir(&backup_root).await {
            Ok(()) => return Ok(backup_root),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(format!(
                    "failed to create unique backup directory '{}': {}",
                    backup_root.display(),
                    err
                ));
            }
        }
    }
    Err("failed to allocate unique backup directory after multiple attempts".to_owned())
}

fn env_path_or_default(var_name: &str, default_value: &str) -> PathBuf {
    std::env::var(var_name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_value))
}

fn parse_bool_env(var_name: &str, default_value: bool) -> bool {
    std::env::var(var_name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(default_value)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;

    #[test]
    fn test_backup_name_for_path() {
        let path = Path::new("/var/data/users.json");
        let name = backup_name_for_path(path);
        assert!(name.ends_with("__users.json"));
        assert_eq!(name.len(), 16 + "__users.json".len());
    }

    #[test]
    fn backup_name_for_path_disambiguates_same_filename_different_paths() {
        let left = backup_name_for_path(Path::new("/var/data/users.json"));
        let right = backup_name_for_path(Path::new("/srv/data/users.json"));
        assert_ne!(left, right);
        assert!(left.ends_with("__users.json"));
        assert!(right.ends_with("__users.json"));
    }

    #[test]
    fn test_parse_bool_env_default() {
        assert!(parse_bool_env("NON_EXISTENT_VAR_123", true));
        assert!(!parse_bool_env("NON_EXISTENT_VAR_123", false));
    }

    #[test]
    fn backup_dir_name_rejects_traversal() {
        assert!(is_safe_backup_dir_name("backup-123"));
        assert!(!is_safe_backup_dir_name("../backup-123"));
        assert!(!is_safe_backup_dir_name("nested/backup-123"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn backup_roundtrip_restores_source_contents() {
        let test_root = std::env::temp_dir().join(format!(
            "mgs-backup-roundtrip-{}-{}",
            std::process::id(),
            unix_now()
        ));
        let source_path = test_root.join("data/auth_store.json");
        let backup_dir = test_root.join("backups");
        stdfs::create_dir_all(source_path.parent().expect("source parent"))
            .expect("create test source directory");
        stdfs::write(&source_path, r#"{"v":"original"}"#).expect("write original source");

        let manager = BackupManager {
            inner: Arc::new(BackupConfig {
                enabled: true,
                interval_seconds: 1,
                output_dir: backup_dir.clone(),
                retention_count: 4,
                redis_url: None,
                redis_store_key: "mgs:test:backup:latest".to_owned(),
                sources: vec![source_path.clone()],
            }),
        };

        manager
            .run_once("roundtrip-test")
            .await
            .expect("backup run");
        stdfs::write(&source_path, r#"{"v":"mutated"}"#).expect("mutate source");

        let restored = manager
            .restore_latest_backup()
            .await
            .expect("restore latest backup");
        let restored_content = stdfs::read_to_string(&source_path).expect("read restored file");

        assert!(restored
            .restored_files
            .iter()
            .any(|file| file.source == source_path.to_string_lossy()));
        assert_eq!(restored_content, r#"{"v":"original"}"#);

        let _ = stdfs::remove_dir_all(&test_root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn backup_runs_create_distinct_directories_even_in_same_second() {
        let test_root = std::env::temp_dir().join(format!(
            "mgs-backup-distinct-dirs-{}-{}",
            std::process::id(),
            unix_now()
        ));
        let source_path = test_root.join("data/auth_store.json");
        let backup_dir = test_root.join("backups");
        stdfs::create_dir_all(source_path.parent().expect("source parent"))
            .expect("create source directory");
        stdfs::write(&source_path, r#"{"v":"initial"}"#).expect("write source");

        let manager = BackupManager {
            inner: Arc::new(BackupConfig {
                enabled: true,
                interval_seconds: 1,
                output_dir: backup_dir.clone(),
                retention_count: 8,
                redis_url: None,
                redis_store_key: "mgs:test:backup:latest".to_owned(),
                sources: vec![source_path.clone()],
            }),
        };

        manager.run_once("run-one").await.expect("first backup");
        manager.run_once("run-two").await.expect("second backup");

        let mut backup_dirs = stdfs::read_dir(&backup_dir)
            .expect("read backup dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("backup-"))
            .collect::<Vec<_>>();
        backup_dirs.sort();
        assert_eq!(
            backup_dirs.len(),
            2,
            "expected two distinct backup directories"
        );
        assert_ne!(backup_dirs[0], backup_dirs[1]);

        let _ = stdfs::remove_dir_all(&test_root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn latest_backup_metadata_falls_back_to_file_when_redis_is_invalid() {
        let test_root = std::env::temp_dir().join(format!(
            "mgs-backup-metadata-fallback-{}-{}",
            std::process::id(),
            unix_now()
        ));
        let source_path = test_root.join("data/auth_store.json");
        let backup_dir = test_root.join("backups");
        stdfs::create_dir_all(source_path.parent().expect("source parent"))
            .expect("create source directory");
        stdfs::write(&source_path, r#"{"v":"initial"}"#).expect("write source");

        let manager = BackupManager {
            inner: Arc::new(BackupConfig {
                enabled: true,
                interval_seconds: 1,
                output_dir: backup_dir,
                retention_count: 8,
                redis_url: Some("redis://".to_owned()),
                redis_store_key: "mgs:test:backup:latest".to_owned(),
                sources: vec![source_path.clone()],
            }),
        };

        manager
            .run_once("metadata-fallback")
            .await
            .expect("backup run");

        let latest = manager
            .latest_backup_metadata()
            .await
            .expect("latest backup metadata");
        assert_eq!(
            latest.as_ref().map(|record| record.reason.as_str()),
            Some("metadata-fallback")
        );
        assert_eq!(
            latest.as_ref().map(|record| record.copied_files.len()),
            Some(1)
        );

        let _ = stdfs::remove_dir_all(&test_root);
    }
}
