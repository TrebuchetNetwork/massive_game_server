use crate::server::instance::MassiveGameServer;
use std::sync::Arc;
use warp::Filter;

/// Public live-match scoreboard covering the FULL roster. The in-game Tab
/// scoreboard is otherwise limited to players inside the viewer's AoI
/// radius, so a 24-player match showed only the ~10 nearby combatants;
/// the client polls this endpoint while the scoreboard is open.
pub fn build_match_scoreboard_route(
    server: Arc<MassiveGameServer>,
) -> impl Filter<Extract = (warp::reply::Response,), Error = warp::Rejection> + Clone {
    warp::path!("api" / "public" / "match" / "scoreboard")
        .and(warp::get())
        .map(move || {
            let (game_mode, time_remaining, team_scores) = {
                let match_info = server.match_info.read();
                (
                    format!("{:?}", match_info.game_mode),
                    match_info.time_remaining,
                    match_info.team_scores.clone(),
                )
            };

            let mut players = Vec::new();
            server.player_manager.for_each_player(|player_id, state| {
                if state.is_spectator {
                    return;
                }
                players.push(serde_json::json!({
                    "player_id": player_id.as_ref(),
                    "username": state.username,
                    "team_id": state.team_id,
                    "kills": state.kills,
                    "deaths": state.deaths,
                    "score": state.score,
                    "is_bot": state.is_bot,
                    "alive": state.alive,
                }));
            });
            players.sort_by(|left, right| {
                right["score"]
                    .as_i64()
                    .unwrap_or(0)
                    .cmp(&left["score"].as_i64().unwrap_or(0))
            });

            let team_scores_json: Vec<serde_json::Value> = team_scores
                .iter()
                .map(|(team_id, score)| {
                    serde_json::json!({ "team_id": team_id, "score": score })
                })
                .collect();

            warp::reply::json(&serde_json::json!({
                "ok": true,
                "game_mode": game_mode,
                "time_remaining": time_remaining,
                "coop_gauntlet": crate::server::instance::coop_gauntlet_enabled(),
                "gauntlet_wave": crate::server::instance::gauntlet_status(),
                "team_scores": team_scores_json,
                "players": players,
            }))
        })
        .map(warp::reply::Reply::into_response)
}
