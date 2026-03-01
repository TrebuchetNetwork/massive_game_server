use crate::server::instance::MassiveGameServer;
use serde::Serialize;
use std::collections::BTreeMap;
use std::future::pending;
use std::path::PathBuf;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

pub fn request_shutdown(server: &Arc<MassiveGameServer>) {
    server.request_shutdown();
}

pub fn is_shutdown_requested(server: &MassiveGameServer) -> bool {
    server.is_shutdown_requested()
}

pub async fn wait_for_shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(stream) => Some(stream),
            Err(err) => {
                warn!("Failed to register SIGTERM listener: {}", err);
                None
            }
        };
        let mut interrupt = match signal(SignalKind::interrupt()) {
            Ok(stream) => Some(stream),
            Err(err) => {
                warn!("Failed to register SIGINT listener: {}", err);
                None
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "ctrl_c",
            _ = async {
                if let Some(sig) = terminate.as_mut() {
                    let _ = sig.recv().await;
                } else {
                    pending::<()>().await;
                }
            } => "sigterm",
            _ = async {
                if let Some(sig) = interrupt.as_mut() {
                    let _ = sig.recv().await;
                } else {
                    pending::<()>().await;
                }
            } => "sigint",
        }
    }

    #[cfg(not(unix))]
    {
        match tokio::signal::ctrl_c().await {
            Ok(()) => "ctrl_c",
            Err(err) => {
                warn!("Failed to listen for shutdown signal: {}", err);
                "signal_error"
            }
        }
    }
}

pub async fn request_shutdown_on_signal(server: Arc<MassiveGameServer>) {
    let signal = wait_for_shutdown_signal().await;
    info!("Shutdown signal received via {}.", signal);
    match persist_shutdown_state_if_configured(server.as_ref()).await {
        Ok(Some(path)) => info!("Persisted shutdown state snapshot to '{}'.", path),
        Ok(None) => {}
        Err(err) => warn!("Failed to persist shutdown state snapshot: {}", err),
    }
    request_shutdown(&server);
}

pub async fn drain_game_loop_with_timeout(
    game_loop_handle: &mut JoinHandle<()>,
    timeout: Duration,
) {
    match tokio::time::timeout(timeout, &mut *game_loop_handle).await {
        Ok(join_result) => {
            if let Err(err) = join_result {
                error!("Game loop task join failed: {}", err);
            }
        }
        Err(_) => {
            warn!(
                "Game loop did not stop within {}s; aborting task.",
                timeout.as_secs()
            );
            game_loop_handle.abort();
            match game_loop_handle.await {
                Ok(()) => {}
                Err(err) => warn!("Game loop abort join result: {}", err),
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct ShutdownStateSnapshot {
    recorded_at_unix_ms: u64,
    frame: u64,
    map_name: String,
    match_summary: ShutdownMatchSummary,
    population: ShutdownPopulationSummary,
    entities: ShutdownEntitySummary,
    players: Vec<ShutdownPlayerSummary>,
}

#[derive(Debug, Serialize)]
struct ShutdownMatchSummary {
    state: String,
    mode: String,
    time_remaining: f32,
    team_scores: BTreeMap<u8, i32>,
}

#[derive(Debug, Serialize)]
struct ShutdownPopulationSummary {
    connected_clients: usize,
    total_players: usize,
    alive_players: usize,
    team_counts: BTreeMap<u8, usize>,
}

#[derive(Debug, Serialize)]
struct ShutdownEntitySummary {
    projectiles_total: usize,
    pickups_total: usize,
    pickups_active: usize,
}

#[derive(Debug, Serialize)]
struct ShutdownPlayerSummary {
    id: String,
    username: String,
    team_id: u8,
    alive: bool,
    x: f32,
    y: f32,
    health: i32,
    score: i32,
    kills: i32,
    deaths: i32,
}

pub async fn persist_shutdown_state_if_configured(
    server: &MassiveGameServer,
) -> Result<Option<String>, String> {
    let output_path_raw = std::env::var("MGS_SHUTDOWN_STATE_PATH")
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty());
    let Some(output_path_raw) = output_path_raw else {
        return Ok(None);
    };

    let output_path = PathBuf::from(output_path_raw);
    let frame = server.frame_counter.load(AtomicOrdering::Relaxed);
    let recorded_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let (match_summary, map_name) = {
        let match_info = server.match_info.read();
        let mut team_scores = BTreeMap::new();
        for (team_id, score) in &match_info.team_scores {
            team_scores.insert(*team_id, *score);
        }
        (
            ShutdownMatchSummary {
                state: format!("{:?}", match_info.match_state),
                mode: format!("{:?}", match_info.game_mode),
                time_remaining: match_info.time_remaining,
                team_scores,
            },
            server.map_name.clone(),
        )
    };

    let mut players = Vec::new();
    let mut team_counts = BTreeMap::new();
    let mut alive_players = 0usize;
    server
        .player_manager
        .for_each_player(|player_id, player_state| {
            if player_state.alive {
                alive_players += 1;
            }
            *team_counts.entry(player_state.team_id).or_insert(0) += 1;
            players.push(ShutdownPlayerSummary {
                id: player_id.as_ref().to_owned(),
                username: player_state.username.clone(),
                team_id: player_state.team_id,
                alive: player_state.alive,
                x: player_state.x,
                y: player_state.y,
                health: player_state.health,
                score: player_state.score,
                kills: player_state.kills,
                deaths: player_state.deaths,
            });
        });
    players.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.username.cmp(&right.username))
    });

    let projectiles_total = server.projectiles.read().len();
    let (pickups_total, pickups_active) = {
        let pickups = server.pickups.read();
        let total = pickups.len();
        let active = pickups.iter().filter(|pickup| pickup.is_active).count();
        (total, active)
    };

    let snapshot = ShutdownStateSnapshot {
        recorded_at_unix_ms,
        frame,
        map_name,
        match_summary,
        population: ShutdownPopulationSummary {
            connected_clients: server.data_channels_map.len(),
            total_players: players.len(),
            alive_players,
            team_counts,
        },
        entities: ShutdownEntitySummary {
            projectiles_total,
            pickups_total,
            pickups_active,
        },
        players,
    };

    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| format!("failed creating shutdown-state directory: {}", err))?;
    }

    let payload = serde_json::to_vec_pretty(&snapshot)
        .map_err(|err| format!("failed serializing shutdown-state snapshot: {}", err))?;
    tokio::fs::write(&output_path, payload)
        .await
        .map_err(|err| format!("failed writing shutdown-state snapshot: {}", err))?;

    Ok(Some(output_path.to_string_lossy().to_string()))
}
