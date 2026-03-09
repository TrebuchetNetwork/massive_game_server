use super::types::{
    ApiErrorBody, ApiResponse, ExecuteNextBody, LeaderboardQuery, ModelHeartbeatBody, PendingQuery,
    QueueMatchBody, QueueRoundRobinBody, RegisterModelBody, ReplayEventsQuery, ReplayQuery,
    ReplayStreamQuery, ReportMatchBody, SimulateTeamBattleBody, UploadModelWasmBody,
};
use super::ArenaService;
use serde::Serialize;
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
