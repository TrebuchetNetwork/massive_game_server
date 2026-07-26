use crate::core::types::PlayerAoI;
use crate::network::connection_manager::shared_connection_manager;
use crate::network::quic::connected_quic_peer_count;
use crate::network::signaling::{
    cleanup_connection, current_webrtc_peer_state_label, ClientStatesMap, DataChannelsMap,
    PlayerManagerRef, SignalingPeers,
};
use crate::operational::arena::ArenaService;
use crate::operational::auth::AuthService;
use crate::operational::backup::BackupManager;
use crate::operational::bot_exhibition::ExhibitionRefreshOutcome;
use crate::operational::config::env_registry::ArenaWorkerEnv;
use crate::operational::diagnostics::heap_profiler;
use crate::operational::monitoring::{alerts as monitoring_alerts, metrics as monitoring_metrics};
use crate::operational::runtime_utils::{parse_u64_env, recent_frame_p95_ms};
use crate::server::instance::MassiveGameServer;

use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

const SIGNALING_RUNTIME_ORPHAN_GRACE: Duration = Duration::from_secs(15);

fn should_evict_signaling_peer(
    has_connection_record: bool,
    connected_for: Option<Duration>,
    has_data_channel: bool,
    has_client_state: bool,
    has_player_aoi: bool,
    has_player_state: bool,
    webrtc_state_label: Option<&str>,
) -> bool {
    if !has_connection_record {
        return true;
    }

    if matches!(webrtc_state_label, Some("closed" | "failed")) {
        return true;
    }

    if matches!(webrtc_state_label, Some("disconnected"))
        && connected_for.is_some_and(|age| age >= SIGNALING_RUNTIME_ORPHAN_GRACE)
    {
        return true;
    }

    let runtime_state_missing =
        !has_data_channel && !has_client_state && !has_player_aoi && !has_player_state;
    runtime_state_missing && connected_for.is_some_and(|age| age >= SIGNALING_RUNTIME_ORPHAN_GRACE)
}

pub fn spawn_backup_worker(backup_manager: BackupManager, server: Arc<MassiveGameServer>) {
    if backup_manager.enabled() {
        info!(
            "Automated backups enabled (interval={}s, dir from MGS_BACKUP_DIR).",
            backup_manager.interval_seconds()
        );
        let backup_manager_task = backup_manager.clone();
        let server_for_backup_task = server.clone();
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(Duration::from_secs(backup_manager_task.interval_seconds()));
            // Skip immediate tick to avoid backup spike during warm startup.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if server_for_backup_task.is_shutdown_requested() {
                    info!("Backup worker observed shutdown; stopping.");
                    break;
                }
                if let Err(err) = backup_manager_task.run_once("scheduled").await {
                    warn!("Scheduled backup failed: {}", err);
                }
            }
        });
    } else {
        info!("Automated backups are disabled (set MGS_BACKUP_ENABLED=1 to enable).");
    }
}

/// Keeps the live exhibition's prepared runtime pool full and atomically
/// rotates active fighters when a published rating snapshot or round changes.
/// WASM reads, validation, compilation, and instantiation all run on Tokio's
/// blocking pool, never on the authoritative frame loop.
pub fn spawn_arena_exhibition_refresh(server: Arc<MassiveGameServer>) {
    if !server.arena_exhibition.enabled() {
        info!("Arena exhibition refresher disabled.");
        return;
    }

    let interval_seconds = parse_u64_env("MGS_ARENA_EXHIBITION_REFRESH_SECONDS", 2).clamp(1, 300);
    info!(
        "Arena exhibition refresher enabled (interval={}s).",
        interval_seconds
    );
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_seconds));
        // Startup preparation already ran before the game loop. Avoid an
        // immediate duplicate compilation pass.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if server.is_shutdown_requested() {
                info!("Arena exhibition refresher observed shutdown; stopping.");
                break;
            }

            let exhibition = server.arena_exhibition.clone();
            match tokio::task::spawn_blocking(move || exhibition.refresh_if_needed()).await {
                Ok(ExhibitionRefreshOutcome::Refreshed) => {
                    info!("Arena exhibition runtime generation refreshed.");
                }
                Ok(
                    ExhibitionRefreshOutcome::Disabled
                    | ExhibitionRefreshOutcome::Busy
                    | ExhibitionRefreshOutcome::Unchanged,
                ) => {}
                Ok(ExhibitionRefreshOutcome::RetryRequired) => {
                    warn!("Arena exhibition refresh raced a bot population change; retrying.");
                }
                Ok(ExhibitionRefreshOutcome::InvalidSnapshot) => {
                    warn!("Arena exhibition ignored an invalid published ratings snapshot.");
                }
                Err(err) => warn!("Arena exhibition background refresh failed: {}", err),
            }
        }
    });
}

pub fn spawn_alert_evaluator(server: Arc<MassiveGameServer>) {
    let alert_rules = monitoring_alerts::default_alert_rules_from_env();
    let alert_notifier = monitoring_alerts::AlertmanagerNotifier::new(
        monitoring_alerts::AlertmanagerConfig::from_env(),
    );
    if alert_rules.is_empty() {
        info!("Alert evaluator disabled (no threshold env vars configured).");
    } else {
        let alert_eval_interval_secs = parse_u64_env("MGS_ALERT_EVAL_INTERVAL_SECONDS", 15);
        info!(
            "Alert evaluator enabled (rules={}, interval={}s, alertmanager_webhook={}).",
            alert_rules.len(),
            alert_eval_interval_secs,
            if alert_notifier.enabled() {
                "configured"
            } else {
                "disabled"
            }
        );
        let server_for_alerts = server.clone();
        let rules_for_alerts = alert_rules.clone();
        let notifier_for_alerts = alert_notifier.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(alert_eval_interval_secs));
            loop {
                ticker.tick().await;
                if server_for_alerts.is_shutdown_requested() {
                    info!("Alert evaluator observed shutdown; stopping.");
                    break;
                }

                let connected_players = server_for_alerts
                    .player_manager
                    .player_count()
                    .saturating_add(connected_quic_peer_count());
                let heap_snapshot = heap_profiler::collect_heap_snapshot();
                monitoring_metrics::record_memory_usage(
                    heap_snapshot.resident_bytes,
                    heap_snapshot.allocated_bytes,
                );

                let mut snapshots = vec![monitoring_alerts::MetricSnapshot {
                    name: "game_players_connected".to_owned(),
                    value: connected_players as f64,
                }];
                if let Some(frame_p95_ms) = recent_frame_p95_ms(server_for_alerts.as_ref()) {
                    snapshots.push(monitoring_alerts::MetricSnapshot {
                        name: "game_frame_time_ms_p95".to_owned(),
                        value: frame_p95_ms,
                    });
                }
                if let Some(rss_bytes) = heap_snapshot.resident_bytes {
                    snapshots.push(monitoring_alerts::MetricSnapshot {
                        name: "game_memory_rss_bytes".to_owned(),
                        value: rss_bytes as f64,
                    });
                }

                let events = monitoring_alerts::evaluate_alerts(&rules_for_alerts, &snapshots);
                if !events.is_empty() {
                    warn!("Alert thresholds crossed: {:?}", events);
                    notifier_for_alerts.notify_events(&events).await;
                }
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_idle_connection_cleanup(
    signaling_peers: SignalingPeers,
    player_manager: PlayerManagerRef,
    data_channels: DataChannelsMap,
    client_states: ClientStatesMap,
    player_aois: Arc<DashMap<String, PlayerAoI>>,
    auth_service: AuthService,
    shutdown_flag: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        let stale_threshold = Duration::from_secs(120);
        loop {
            ticker.tick().await;
            if shutdown_flag.load(Ordering::Relaxed) {
                break;
            }
            let client_states_guard = client_states.read();
            let orphaned_signaling_ids: Vec<String> = signaling_peers
                .iter()
                .filter_map(|entry| {
                    let peer_id = entry.key();
                    let connection_info = shared_connection_manager().get(peer_id.as_str());
                    let connected_for = connection_info
                        .as_ref()
                        .map(|info| info.connected_at.elapsed());
                    let player_id_lookup: Arc<str> = Arc::from(peer_id.clone());
                    let has_player_state =
                        player_manager.get_player_state(&player_id_lookup).is_some();
                    let webrtc_state_label = current_webrtc_peer_state_label(peer_id.as_str());
                    should_evict_signaling_peer(
                        connection_info.is_some(),
                        connected_for,
                        data_channels.contains_key(peer_id.as_str()),
                        client_states_guard.contains_key(peer_id.as_str()),
                        player_aois.contains_key(peer_id.as_str()),
                        has_player_state,
                        webrtc_state_label,
                    )
                    .then(|| peer_id.clone())
                })
                .collect();
            drop(client_states_guard);
            let mut evict_ids = shared_connection_manager().stale_peer_ids(stale_threshold);
            for orphan_id in orphaned_signaling_ids {
                if !evict_ids.iter().any(|peer_id| peer_id == &orphan_id) {
                    evict_ids.push(orphan_id);
                }
            }
            if !evict_ids.is_empty() {
                info!(
                    "Idle connection cleanup: evicting {} stale/orphaned peer(s).",
                    evict_ids.len()
                );
                for peer_id in &evict_ids {
                    cleanup_connection(
                        peer_id,
                        &signaling_peers,
                        &player_manager,
                        &data_channels,
                        &client_states,
                        &player_aois,
                        &auth_service,
                    );
                }
            }
        }
    });
}

pub fn spawn_arena_worker(
    server: Arc<MassiveGameServer>,
    arena_service: ArenaService,
    worker_config: &ArenaWorkerEnv,
) {
    if !worker_config.enabled {
        return;
    }

    let worker_interval_ms = worker_config.interval_ms;
    let worker_max_ticks = worker_config.max_ticks;
    let worker_shutdown_server = server.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(worker_interval_ms));
        info!(
            "Arena worker enabled (interval_ms={}, max_ticks={:?}).",
            worker_interval_ms, worker_max_ticks
        );
        loop {
            ticker.tick().await;
            if worker_shutdown_server.is_shutdown_requested() {
                info!("Arena worker shutdown requested; stopping worker loop.");
                break;
            }
            match arena_service.worker_execute_next(worker_max_ticks, None) {
                Ok(Some(executed)) => {
                    info!(
                        "Arena worker executed match '{}' mode={} (pending {} -> {}, draw={}, winner={:?}, objective {}:{}-{}, runtimes=({},{}) ).",
                        executed.report.match_id,
                        executed.sandbox.mode,
                        executed.pending_before,
                        executed.pending_after,
                        executed.sandbox.draw,
                        executed.sandbox.winner_model_id,
                        executed.sandbox.objective_label,
                        executed.sandbox.objective_a,
                        executed.sandbox.objective_b,
                        executed.sandbox.model_a_runtime,
                        executed.sandbox.model_b_runtime
                    );
                }
                Ok(None) => {}
                Err(err) => warn!("Arena worker execute_next failed: {}", err),
            }
        }
        info!("Arena worker stopped.");
    });
}

#[cfg(test)]
mod tests {
    use super::should_evict_signaling_peer;
    use std::time::Duration;

    #[test]
    fn evicts_signaling_peer_without_connection_record() {
        assert!(should_evict_signaling_peer(
            false, None, true, true, true, true, None
        ));
    }

    #[test]
    fn evicts_runtime_orphan_after_grace_period() {
        assert!(should_evict_signaling_peer(
            true,
            Some(Duration::from_secs(20)),
            false,
            false,
            false,
            false,
            None
        ));
    }

    #[test]
    fn preserves_recent_handshakes_without_runtime_state() {
        assert!(!should_evict_signaling_peer(
            true,
            Some(Duration::from_secs(5)),
            false,
            false,
            false,
            false,
            None
        ));
    }

    #[test]
    fn preserves_active_signaling_peer_with_runtime_state() {
        assert!(!should_evict_signaling_peer(
            true,
            Some(Duration::from_secs(60)),
            true,
            false,
            false,
            false,
            None
        ));
    }

    #[test]
    fn evicts_closed_webrtc_peer_even_with_runtime_state_leftovers() {
        assert!(should_evict_signaling_peer(
            true,
            Some(Duration::from_secs(2)),
            true,
            true,
            true,
            true,
            Some("closed")
        ));
    }

    #[test]
    fn evicts_disconnected_webrtc_peer_after_grace_period() {
        assert!(should_evict_signaling_peer(
            true,
            Some(Duration::from_secs(20)),
            true,
            true,
            false,
            false,
            Some("disconnected")
        ));
    }
}
