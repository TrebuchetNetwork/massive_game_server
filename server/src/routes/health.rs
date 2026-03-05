use crate::server::instance::MassiveGameServer;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use warp::http::StatusCode;
use warp::Filter;

pub fn build_healthz_route(
    server: Arc<MassiveGameServer>,
) -> impl Filter<Extract = (warp::reply::Response,), Error = warp::Rejection> + Clone {
    warp::path("healthz")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || {
            let last_tick = server
                .last_tick_epoch_ms
                .load(std::sync::atomic::Ordering::Relaxed);
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64;
            let tick_age_ms = now_ms.saturating_sub(last_tick);

            // Consider the game loop stalled if no tick completed in the last 2 seconds.
            // A last_tick of 0 means the loop hasn't started yet which is acceptable
            // during startup; the readyz endpoint covers that case.
            let game_loop_alive = last_tick == 0 || tick_age_ms <= 2_000;

            let match_degraded = server.is_match_degraded();

            if game_loop_alive {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "ok": true,
                        "service": "massive_game_server",
                        "last_tick_age_ms": tick_age_ms,
                        "match_degraded": match_degraded,
                    })),
                    StatusCode::OK,
                )
            } else {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "ok": false,
                        "service": "massive_game_server",
                        "error": "game_loop_stalled",
                        "last_tick_age_ms": tick_age_ms,
                    })),
                    StatusCode::SERVICE_UNAVAILABLE,
                )
            }
        })
        .map(warp::reply::Reply::into_response)
}

pub fn build_readyz_route(
    server: Arc<MassiveGameServer>,
) -> impl Filter<Extract = (warp::reply::Response,), Error = warp::Rejection> + Clone {
    warp::path("readyz")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || {
            let last_tick = server
                .last_tick_epoch_ms
                .load(std::sync::atomic::Ordering::Relaxed);
            let frame = server
                .frame_counter
                .load(std::sync::atomic::Ordering::Relaxed);

            // Server is ready once the game loop has completed at least one tick.
            let ready = last_tick > 0 && frame > 0;

            if ready {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "ok": true,
                        "service": "massive_game_server",
                        "frame": frame,
                    })),
                    StatusCode::OK,
                )
            } else {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "ok": false,
                        "service": "massive_game_server",
                        "error": "not_ready",
                        "frame": frame,
                    })),
                    StatusCode::SERVICE_UNAVAILABLE,
                )
            }
        })
        .map(warp::reply::Reply::into_response)
}
