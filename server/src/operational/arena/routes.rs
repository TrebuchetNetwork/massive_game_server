use super::ratings::{load_ratings_response, ratings_path_from_env};
use super::types::{
    ApiErrorBody, ApiResponse, ExecuteNextBody, LeaderboardQuery, ModelHeartbeatBody, PendingQuery,
    QueueMatchBody, QueueRoundRobinBody, RegisterModelBody, ReplayEventsQuery, ReplayQuery,
    ReplayStreamQuery, ReportMatchBody, SimulateTeamBattleBody, SimulateWorldBattleBody,
    UploadModelWasmBody,
};
use super::ArenaService;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::OnceLock;
use subtle::ConstantTimeEq;
use warp::{Filter, Reply};

pub(super) const DEFAULT_ARENA_LEADERBOARD_LIMIT: usize = 25;
pub(super) const DEFAULT_ARENA_PENDING_LIMIT: usize = 20;
pub(super) const DEFAULT_ARENA_RECENT_REPLAY_LIMIT: usize = 20;
pub(super) const DEFAULT_ARENA_REPLAY_EVENTS_LIMIT: usize = 256;

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
    service: ArenaService,
) -> impl Filter<Extract = (ArenaService,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || service.clone())
}

fn with_ratings_path(
    path: PathBuf,
) -> impl Filter<Extract = (PathBuf,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || path.clone())
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

pub fn build_arena_routes(
    service: ArenaService,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    // 64 KB body limit for standard JSON endpoints; upload_wasm uses a larger
    // limit since base64-encoded wasm can be up to ~2.7 MB (2 MB decoded).
    let json_body_limit = 1024 * 64;
    let wasm_upload_body_limit = 4 * 1024 * 1024; // 4 MB for base64-encoded wasm

    let register_model = warp::path!("api" / "arena" / "models" / "register")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: RegisterModelBody, arena: ArenaService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match arena.register_model(body) {
                    Ok(model) => ok_response(model),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let model_heartbeat = warp::path!("api" / "arena" / "models" / "heartbeat")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: ModelHeartbeatBody, arena: ArenaService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match arena.heartbeat_model(body) {
                    Ok(model) => ok_response(model),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let upload_model_wasm = warp::path!("api" / "arena" / "models" / "upload_wasm")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(wasm_upload_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: UploadModelWasmBody, arena: ArenaService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match arena.upload_model_wasm(body) {
                    Ok(result) => ok_response(result),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let list_models = warp::path!("api" / "arena" / "models")
        .and(warp::get())
        .and(
            warp::query::<LeaderboardQuery>()
                .or(warp::any().map(LeaderboardQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: LeaderboardQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_LEADERBOARD_LIMIT);
            let leaderboard = arena.leaderboard(limit);
            ok_response(leaderboard.models)
        });

    let queue_match = warp::path!("api" / "arena" / "matches" / "queue")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: QueueMatchBody, arena: ArenaService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match arena.queue_match(body) {
                    Ok(result) => ok_response(result),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let queue_round_robin = warp::path!("api" / "arena" / "matches" / "queue_round_robin")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: QueueRoundRobinBody, arena: ArenaService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match arena.queue_round_robin(body) {
                    Ok(result) => ok_response(result),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let claim_next = warp::path!("api" / "arena" / "matches" / "claim_next")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(with_service(service.clone()))
        .map(|authorization: Option<String>, arena: ArenaService| {
            if !inline_admin_authorized(authorization.as_deref()) {
                return error_response(
                    "admin_auth_required",
                    "Admin bearer token required.".to_owned(),
                );
            }
            ok_response(arena.claim_next_match())
        });

    let execute_next = warp::path!("api" / "arena" / "matches" / "execute_next")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(
            warp::body::content_length_limit(json_body_limit)
                .and(warp::body::json::<ExecuteNextBody>())
                .or(warp::any().map(ExecuteNextBody::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: ExecuteNextBody, arena: ArenaService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match arena.execute_next_match(body) {
                    Ok(result) => ok_response(result),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let simulate_team_battle = warp::path!("api" / "arena" / "matches" / "simulate_team_battle")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(
            warp::body::content_length_limit(json_body_limit)
                .and(warp::body::json::<SimulateTeamBattleBody>())
                .or(warp::any().map(SimulateTeamBattleBody::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: SimulateTeamBattleBody, arena: ArenaService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match arena.simulate_team_battle(body) {
                    Ok(result) => ok_response(result),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let simulate_world_battle = warp::path!("api" / "arena" / "matches" / "simulate_world_battle")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(
            warp::body::content_length_limit(json_body_limit)
                .and(warp::body::json::<SimulateWorldBattleBody>())
                .or(warp::any().map(SimulateWorldBattleBody::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: SimulateWorldBattleBody, arena: ArenaService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match arena.simulate_world_battle(body) {
                    Ok(result) => ok_response(result),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let list_pending = warp::path!("api" / "arena" / "matches" / "pending")
        .and(warp::get())
        .and(
            warp::query::<PendingQuery>()
                .or(warp::any().map(PendingQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: PendingQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_PENDING_LIMIT);
            ok_response(arena.list_pending_matches(limit))
        });

    let recent_replays = warp::path!("api" / "arena" / "replays" / "recent")
        .and(warp::get())
        .and(
            warp::query::<ReplayQuery>()
                .or(warp::any().map(ReplayQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: ReplayQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_RECENT_REPLAY_LIMIT);
            ok_response(arena.recent_replays(limit))
        });

    let replay_events_recent = warp::path!("api" / "arena" / "replays" / "events" / "recent")
        .and(warp::get())
        .and(
            warp::query::<ReplayEventsQuery>()
                .or(warp::any().map(ReplayEventsQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: ReplayEventsQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_REPLAY_EVENTS_LIMIT);
            ok_response(arena.recent_replay_events(limit, query.after_sequence))
        });

    let replay_events_for_match = warp::path!("api" / "arena" / "replays" / String / "events")
        .and(warp::get())
        .and(
            warp::query::<ReplayEventsQuery>()
                .or(warp::any().map(ReplayEventsQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(
            |match_id: String, query: ReplayEventsQuery, arena: ArenaService| {
                let limit = query.limit.unwrap_or(DEFAULT_ARENA_REPLAY_EVENTS_LIMIT);
                match arena.replay_events_for_match(&match_id, limit, query.after_sequence) {
                    Ok(response) => ok_response(response),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let replay_stream = warp::path!("api" / "arena" / "replays" / "stream")
        .and(warp::get())
        .and(
            warp::query::<ReplayStreamQuery>()
                .or(warp::any().map(ReplayStreamQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: ReplayStreamQuery, arena: ArenaService| arena.replay_stream(query));

    let report_match = warp::path!("api" / "arena" / "matches" / "report")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |authorization: Option<String>, body: ReportMatchBody, arena: ArenaService| {
                if !inline_admin_authorized(authorization.as_deref()) {
                    return error_response(
                        "admin_auth_required",
                        "Admin bearer token required.".to_owned(),
                    );
                }
                match arena.report_match(body) {
                    Ok(result) => ok_response(result),
                    Err(err) => error_response(err.code(), err.message()),
                }
            },
        );

    let leaderboard = warp::path!("api" / "arena" / "leaderboard")
        .and(warp::get())
        .and(
            warp::query::<LeaderboardQuery>()
                .or(warp::any().map(LeaderboardQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: LeaderboardQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_LEADERBOARD_LIMIT);
            ok_response(arena.leaderboard(limit))
        });

    let overview = warp::path!("api" / "arena" / "overview")
        .and(warp::get())
        .and(with_service(service.clone()))
        .map(|arena: ArenaService| ok_response(arena.overview()));

    let worker_stats = warp::path!("api" / "arena" / "worker" / "stats")
        .and(warp::get())
        .and(with_service(service))
        .map(|arena: ArenaService| ok_response(arena.worker_stats()));

    register_model
        .or(model_heartbeat)
        .or(upload_model_wasm)
        .or(list_models)
        .or(queue_match)
        .or(queue_round_robin)
        .or(claim_next)
        .or(execute_next)
        .or(simulate_team_battle)
        .or(simulate_world_battle)
        .or(list_pending)
        .or(recent_replays)
        .or(replay_events_recent)
        .or(replay_events_for_match)
        .or(replay_stream)
        .or(report_match)
        .or(leaderboard)
        .or(overview)
        .or(worker_stats)
        .map(warp::reply::Reply::into_response)
        .boxed()
}

/// Read-only arena telemetry intended for the public evolution surface.
/// Mutation, source, worker, and replay-event endpoints remain admin-only.
pub fn build_public_arena_routes(
    service: ArenaService,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    build_public_arena_routes_with_ratings_path(service, ratings_path_from_env())
}

fn build_public_arena_routes_with_ratings_path(
    service: ArenaService,
    ratings_path: PathBuf,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    let overview = warp::path!("api" / "public" / "arena" / "overview")
        .and(warp::get())
        .and(with_service(service.clone()))
        .map(|arena: ArenaService| ok_response(arena.overview()));

    let leaderboard = warp::path!("api" / "public" / "arena" / "leaderboard")
        .and(warp::get())
        .and(
            warp::query::<LeaderboardQuery>()
                .or(warp::any().map(LeaderboardQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: LeaderboardQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_LEADERBOARD_LIMIT);
            ok_response(arena.leaderboard(limit))
        });

    let recent_replays = warp::path!("api" / "public" / "arena" / "replays" / "recent")
        .and(warp::get())
        .and(
            warp::query::<ReplayQuery>()
                .or(warp::any().map(ReplayQuery::default))
                .unify(),
        )
        .and(with_service(service))
        .map(|query: ReplayQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_RECENT_REPLAY_LIMIT);
            ok_response(arena.recent_replays(limit))
        });

    let ratings = warp::path!("api" / "public" / "arena" / "ratings")
        .and(warp::get())
        .and(with_ratings_path(ratings_path))
        .map(|path: PathBuf| ok_response(load_ratings_response(&path)));

    overview
        .or(leaderboard)
        .or(recent_replays)
        .or(ratings)
        .map(warp::reply::Reply::into_response)
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use warp::http::StatusCode;

    #[tokio::test]
    async fn public_arena_routes_expose_telemetry_only() {
        let routes = build_public_arena_routes(ArenaService::new_from_env());

        let overview = warp::test::request()
            .method("GET")
            .path("/api/public/arena/overview")
            .reply(&routes)
            .await;
        assert_eq!(overview.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(overview.body()).expect("overview should be valid JSON");
        assert_eq!(body["ok"], true);

        let mutation = warp::test::request()
            .method("POST")
            .path("/api/public/arena/models/register")
            .reply(&routes)
            .await;
        assert_eq!(mutation.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn world_battle_route_exists_and_requires_admin_auth() {
        let routes = build_arena_routes(ArenaService::new_from_env());
        let response = warp::test::request()
            .method("POST")
            .path("/api/arena/matches/simulate_world_battle")
            .json(&serde_json::json!({
                "model_ids": ["model_a", "model_b"],
                "squad_size": 3,
                "rounds": 1,
                "max_ticks": 120,
                "seed": 104729
            }))
            .reply(&routes)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(response.body()).expect("world auth error should be JSON");
        assert_eq!(body["ok"], false);
        assert_eq!(body["error"]["code"], "admin_auth_required");
    }

    #[tokio::test]
    async fn public_ratings_route_returns_validated_snapshot_in_ok_envelope() {
        let path = std::env::temp_dir().join(format!(
            "mgs-public-arena-ratings-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let fixture = serde_json::json!({
            "schema_version": 1,
            "active": true,
            "season_id": "weekly-2026-07-23",
            "generated_at": "2026-07-23T12:00:00Z",
            "ranking": {
                "source": "https://openrouter.ai/api/v1/models?sort=top-weekly",
                "window": "top-weekly",
                "retrieved_at": "2026-07-23T11:55:00Z"
            },
            "methodology": {
                "prompt_sha256": "a".repeat(64),
                "source_limit_bytes": 51200,
                "modes": ["arena", "ctf"],
                "seed_sets": [1001, 2002],
                "team_size": 10,
                "rounds": 2,
                "personal_weight": 0.4,
                "team_weight": 0.35,
                "collaboration_weight": 0.25,
                "collaboration_kind": "team_context_v2"
            },
            "roster": [{
                "rank": 1,
                "provider_rank": 1,
                "model_id": "model_one",
                "model_name": "Model One",
                "provider_model": "provider/model-one",
                "personal_rating": 92.5,
                "team_rating": 88.0,
                "collaboration_rating": 84.25,
                "overall_rating": 88.86,
                "source_bytes": 2048,
                "source_limit_bytes": 51200,
                "source_sha256": "b".repeat(64),
                "compiled": true,
                "wasm_bytes": 4096,
                "wasm_sha256": "c".repeat(64),
                "compile_attempts": 1,
                "simulated": false,
                "wins": 7,
                "losses": 2,
                "draws": 1,
                "matches_played": 10,
                "evaluation_engagements": 100,
                "integrity_status": "verified_wasm"
            }]
        });
        fs::write(&path, fixture.to_string()).expect("write ratings fixture");
        let routes =
            build_public_arena_routes_with_ratings_path(ArenaService::new_from_env(), path.clone());

        let response = warp::test::request()
            .method("GET")
            .path("/api/public/arena/ratings")
            .reply(&routes)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(response.body()).expect("ratings should be valid JSON");
        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["active"], true);
        assert_eq!(body["data"]["season_id"], "weekly-2026-07-23");
        assert_eq!(body["data"]["roster"][0]["overall_rating"], 88.86);
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn public_ratings_route_hides_missing_artifact_details() {
        let missing_path = std::env::temp_dir().join(format!(
            "mgs-missing-public-arena-ratings-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let routes = build_public_arena_routes_with_ratings_path(
            ArenaService::new_from_env(),
            missing_path.clone(),
        );

        let response = warp::test::request()
            .method("GET")
            .path("/api/public/arena/ratings")
            .reply(&routes)
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(response.body()).expect("ratings should be valid JSON");
        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["active"], false);
        assert_eq!(body["data"]["status"], "no_active_season");
        assert_eq!(body["data"]["roster"], serde_json::json!([]));
        assert!(!response
            .body()
            .windows(missing_path.to_string_lossy().len())
            .any(|window| window == missing_path.to_string_lossy().as_bytes()));
    }
}
