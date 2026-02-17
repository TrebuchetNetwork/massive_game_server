// massive_game_server/server/src/operational/backup.rs

use crate::operational::monitoring::metrics;
use serde::Serialize;
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

#[derive(Debug, Serialize)]
struct BackupManifest {
    created_at_unix: u64,
    reason: String,
    copied_files: Vec<BackupCopiedFile>,
    missing_sources: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BackupCopiedFile {
    source: String,
    backup_path: String,
    size_bytes: u64,
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
            for raw_path in extra_paths.split(',').map(str::trim).filter(|value| !value.is_empty()) {
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
            fs::copy(source_path, &target_path)
                .await
                .map_err(|err| {
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
        let manifest_json =
            serde_json::to_vec_pretty(&manifest).map_err(|err| format!("manifest json: {}", err))?;
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
