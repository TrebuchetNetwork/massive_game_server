// massive_game_server/server/src/operational/backup.rs

use crate::operational::monitoring::metrics;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
struct BackupCopiedFile {
    source: String,
    backup_path: String,
    size_bytes: u64,
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

    async fn run_once_inner(&self, reason: &str) -> Result<(), String> {
        let created_at_unix = unix_now();
        let backup_dir_name = format!("backup-{}", created_at_unix);
        let backup_root = self.inner.output_dir.join(backup_dir_name);
        fs::create_dir_all(&backup_root)
            .await
            .map_err(|err| format!("failed to create backup directory: {}", err))?;

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

        let mut restored_files = Vec::new();
        let mut missing_backup_files = Vec::new();
        for copied_file in &manifest.copied_files {
            let source_path = PathBuf::from(&copied_file.source);
            let backup_path = resolve_backup_file_path(&backup_root, copied_file);
            if !backup_path.exists() {
                missing_backup_files.push(backup_path.to_string_lossy().to_string());
                continue;
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

            fs::copy(&backup_path, &source_path).await.map_err(|err| {
                format!(
                    "failed restoring '{}' from '{}': {}",
                    source_path.display(),
                    backup_path.display(),
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
                restored_from: backup_path.to_string_lossy().to_string(),
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
        if let Some(backup_dir_name) = backup_dir_name {
            let trimmed = backup_dir_name.trim();
            if trimmed.is_empty() {
                return Err("backup directory name cannot be empty".to_owned());
            }
            let explicit = self.inner.output_dir.join(trimmed);
            if explicit.exists() {
                return Ok(explicit);
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
        backup_dirs
            .pop()
            .ok_or_else(|| format!("no backups found in '{}'", self.inner.output_dir.display()))
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
    let prefix = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("root");
    format!("{}__{}", prefix, file_name)
}

fn resolve_backup_file_path(backup_root: &Path, copied_file: &BackupCopiedFile) -> PathBuf {
    let recorded_path = PathBuf::from(&copied_file.backup_path);
    if recorded_path.exists() {
        return recorded_path;
    }
    backup_root.join(backup_name_for_path(Path::new(&copied_file.source)))
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
