use crate::operational::backup::BackupManager;
use crate::server::instance::{LiveReplayDisputeRequest, MassiveGameServer};

use serde::Deserialize;
use std::sync::Arc;
use warp::{Filter, Reply};

#[derive(Clone, Default, Deserialize)]
struct LiveReplayRecentQuery {
    limit: Option<usize>,
}

pub fn build_ops_admin_routes(
    server: Arc<MassiveGameServer>,
    live_replay_env_enabled: bool,
    backup_manager: BackupManager,
) -> warp::filters::BoxedFilter<(warp::reply::Response,)> {
    let server_for_join_stage_report = server.clone();
    let join_stage_report_route = warp::path!("api" / "ops" / "join-stages")
        .and(warp::get())
        .and(warp::any().map(move || server_for_join_stage_report.clone()))
        .map(|server_inst: Arc<MassiveGameServer>| {
            warp::reply::json(&server_inst.join_stage_report()).into_response()
        })
        .boxed();

    let server_for_join_stage_reset = server.clone();
    let join_stage_reset_route = warp::path!("api" / "ops" / "join-stages" / "reset")
        .and(warp::post())
        .and(warp::any().map(move || server_for_join_stage_reset.clone()))
        .map(|server_inst: Arc<MassiveGameServer>| {
            server_inst.reset_join_stage_report();
            warp::reply::json(&serde_json::json!({ "ok": true })).into_response()
        })
        .boxed();

    let server_for_live_replay_recent = server.clone();
    let live_replay_recent_route = warp::path!("api" / "ops" / "live-replay" / "recent")
        .and(warp::get())
        .and(
            warp::query::<LiveReplayRecentQuery>()
                .or(warp::any().map(LiveReplayRecentQuery::default))
                .unify(),
        )
        .and(warp::any().map(move || server_for_live_replay_recent.clone()))
        .map(
            move |query: LiveReplayRecentQuery, server_inst: Arc<MassiveGameServer>| {
                let limit = query.limit.unwrap_or(256).clamp(1, 4096);
                warp::reply::json(&serde_json::json!({
                    "enabled": !server_inst.recent_live_replay_frames(1).is_empty() || live_replay_env_enabled,
                    "frames": server_inst.recent_live_replay_frames(limit),
                    "limit": limit,
                }))
                .into_response()
            },
        )
        .boxed();

    let server_for_live_replay_dispute = server.clone();
    let live_replay_dispute_route = warp::path!("api" / "ops" / "live-replay" / "dispute")
        .and(warp::post())
        .and(warp::body::json::<LiveReplayDisputeRequest>())
        .and(warp::any().map(move || server_for_live_replay_dispute.clone()))
        .map(
            |request: LiveReplayDisputeRequest, server_inst: Arc<MassiveGameServer>| {
                warp::reply::json(&server_inst.build_live_replay_dispute_report(request))
                    .into_response()
            },
        )
        .boxed();

    let server_for_live_replay_dispute_recent = server.clone();
    let live_replay_dispute_recent_route =
        warp::path!("api" / "ops" / "live-replay" / "disputes" / "recent")
            .and(warp::get())
            .and(
                warp::query::<LiveReplayRecentQuery>()
                    .or(warp::any().map(LiveReplayRecentQuery::default))
                    .unify(),
            )
            .and(warp::any().map(move || server_for_live_replay_dispute_recent.clone()))
            .map(
                |query: LiveReplayRecentQuery, server_inst: Arc<MassiveGameServer>| {
                    let limit = query.limit.unwrap_or(128).clamp(1, 2048);
                    warp::reply::json(&serde_json::json!({
                        "ok": true,
                        "op": "live_replay_disputes_recent",
                        "audits": server_inst.recent_live_replay_dispute_audits(limit),
                        "limit": limit,
                    }))
                    .into_response()
                },
            )
            .boxed();

    let server_for_match_summary = server.clone();
    let match_summary_latest_route = warp::path!("api" / "ops" / "match-summary" / "latest")
        .and(warp::get())
        .and(warp::any().map(move || server_for_match_summary.clone()))
        .map(|server_inst: Arc<MassiveGameServer>| {
            warp::reply::json(&serde_json::json!({
                "ok": true,
                "summary": server_inst.latest_match_end_summary(),
            }))
            .into_response()
        })
        .boxed();

    let server_for_killcam = server.clone();
    let killcam_latest_route = warp::path!("api" / "ops" / "killcam" / String)
        .and(warp::get())
        .and(warp::any().map(move || server_for_killcam.clone()))
        .map(|player_id: String, server_inst: Arc<MassiveGameServer>| {
            warp::reply::json(&serde_json::json!({
                "ok": true,
                "player_id": player_id,
                "killcam": server_inst.latest_killcam_for_player(&player_id),
            }))
            .into_response()
        })
        .boxed();

    let server_for_match_type = server.clone();
    let match_type_route = warp::path!("api" / "ops" / "match-type")
        .and(warp::get())
        .and(warp::any().map(move || server_for_match_type.clone()))
        .map(|server_inst: Arc<MassiveGameServer>| {
            warp::reply::json(&serde_json::json!({
                "ok": true,
                "match_type": server_inst.match_type.label(),
                "max_players": server_inst.effective_max_players(),
                "match_duration_secs": server_inst.match_duration_secs,
                "bot_fill_delay_secs": server_inst.match_type.bot_fill_delay_secs(),
                "min_humans_for_bot_fill": server_inst.match_type.min_humans_for_bot_fill(),
            }))
            .into_response()
        })
        .boxed();

    let backup_latest_route = warp::path!("api" / "ops" / "backup" / "latest")
        .and(warp::get())
        .and(warp::any().map(move || backup_manager.clone()))
        .and_then(|backup_manager: BackupManager| async move {
            let response = match backup_manager.latest_backup_metadata().await {
                Ok(metadata) => warp::reply::json(&serde_json::json!({
                    "ok": true,
                    "backup": metadata,
                }))
                .into_response(),
                Err(err) => warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "ok": false,
                        "error": {
                            "code": "backup_metadata_unavailable",
                            "message": err,
                        }
                    })),
                    warp::http::StatusCode::SERVICE_UNAVAILABLE,
                )
                .into_response(),
            };
            Ok::<_, std::convert::Infallible>(response)
        })
        .boxed();

    join_stage_report_route
        .or(join_stage_reset_route)
        .unify()
        .or(live_replay_recent_route)
        .unify()
        .or(live_replay_dispute_route)
        .unify()
        .or(live_replay_dispute_recent_route)
        .unify()
        .or(match_summary_latest_route)
        .unify()
        .or(killcam_latest_route)
        .unify()
        .or(match_type_route)
        .unify()
        .or(backup_latest_route)
        .unify()
        .boxed()
}
