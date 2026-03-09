use super::rate_limiting::{check_otp_ip_rate_limit, try_acquire_token_validation_token};
use super::types::{
    ApiErrorBody, ApiResponse, AuthError, AuthMeResult, AuthService, LeaderboardQuery,
    LeaderboardResult, RequestCodeBody, TokenQuery, VerifyCodeBody,
};
use super::{DEFAULT_LEADERBOARD_LIMIT, MAX_LEADERBOARD_LIMIT};
use crate::operational::monitoring::metrics;
use serde::Serialize;
use std::convert::Infallible;
use std::net::SocketAddr;
use warp::http::StatusCode;
use warp::{Filter, Reply};

pub fn build_auth_routes(
    auth_service: AuthService,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    // 64 KB body limit for all JSON endpoints to prevent resource exhaustion
    let json_body_limit = 1024 * 64;

    let request_code = warp::path!("auth" / "phone" / "request-code")
        .and(warp::post())
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json::<RequestCodeBody>())
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_request_code);

    let verify_code = warp::path!("auth" / "phone" / "verify-code")
        .and(warp::post())
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json::<VerifyCodeBody>())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_verify_code);

    let me = warp::path!("auth" / "me")
        .and(warp::get())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::optional::<String>("cookie"))
        .and(
            warp::query::<TokenQuery>()
                .or(warp::any().map(TokenQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_auth_me);

    let logout = warp::path!("auth" / "logout")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::optional::<String>("cookie"))
        .and(
            warp::query::<TokenQuery>()
                .or(warp::any().map(TokenQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_auth_logout);

    let leaderboard = warp::path!("auth" / "leaderboard")
        .and(warp::get())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::optional::<String>("cookie"))
        .and(
            warp::query::<LeaderboardQuery>()
                .or(warp::any().map(LeaderboardQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_auth_leaderboard);

    let delete_account = warp::path!("auth" / "delete-account")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::optional::<String>("cookie"))
        .and(
            warp::query::<TokenQuery>()
                .or(warp::any().map(TokenQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_delete_account);

    let cancel_deletion = warp::path!("auth" / "cancel-deletion")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::optional::<String>("cookie"))
        .and(
            warp::query::<TokenQuery>()
                .or(warp::any().map(TokenQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service))
        .and_then(handle_cancel_deletion);

    request_code
        .or(verify_code)
        .or(me)
        .or(logout)
        .or(leaderboard)
        .or(delete_account)
        .or(cancel_deletion)
}

fn with_auth_service(
    auth_service: AuthService,
) -> impl Filter<Extract = (AuthService,), Error = Infallible> + Clone {
    warp::any().map(move || auth_service.clone())
}

async fn handle_request_code(
    body: RequestCodeBody,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<impl Reply, Infallible> {
    let client_ip = remote_addr.map(|addr| addr.ip());

    // Per-IP OTP rate limiting: reject before any phone-level logic.
    if let Err(retry_after) = check_otp_ip_rate_limit(client_ip) {
        metrics::record_auth_attempt("request_code", "ip_rate_limited");
        return Ok(error_response(AuthError::OtpIpRateLimited {
            retry_after_seconds: retry_after,
        }));
    }

    let phone_number = body.phone_number;
    let auth_for_task = auth_service.clone();
    let request_result =
        tokio::task::spawn_blocking(move || auth_for_task.request_phone_code(&phone_number))
            .await
            .unwrap_or_else(|join_err| {
                Err(AuthError::DeliveryFailed(format!(
                    "OTP delivery worker failed: {}",
                    join_err
                )))
            });

    let reply = match request_result {
        Ok(result) => ok_response(result),
        Err(error) => error_response(error),
    };
    Ok(reply)
}

pub(super) async fn handle_verify_code(
    body: VerifyCodeBody,
    auth_service: AuthService,
) -> Result<warp::reply::Response, Infallible> {
    match auth_service.verify_phone_code(&body.phone_number, &body.code) {
        Ok(mut result) => {
            if auth_service.use_auth_cookies() {
                let Some(session_token) = result.token.clone() else {
                    return Ok(error_response(AuthError::Internal(
                        "verify-code succeeded but session token was missing".to_owned(),
                    ))
                    .into_response());
                };
                // Set the session token as an HttpOnly, SameSite=Strict cookie
                // (Secure when TLS proxy mode is enabled) so that JS never
                // needs to touch it.
                let cookie_value = build_session_cookie_header(
                    &session_token,
                    auth_service.session_ttl_seconds(),
                    auth_service.auth_cookie_secure(),
                );
                // Cookie mode must not expose bearer tokens in JSON payloads.
                result.token = None;
                let json_reply = ok_response(result);
                Ok(
                    warp::reply::with_header(json_reply, "Set-Cookie", cookie_value)
                        .into_response(),
                )
            } else {
                Ok(ok_response(result).into_response())
            }
        }
        Err(error) => Ok(error_response(error).into_response()),
    }
}

async fn handle_auth_me(
    authorization_header: Option<String>,
    cookie_header: Option<String>,
    query: TokenQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<warp::reply::Response, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        })
        .into_response());
    }
    let token = resolve_token_with_cookie(
        authorization_header.as_deref(),
        &query,
        cookie_header.as_deref(),
    );
    let reply = match token {
        Some(token_value) => match auth_service.profile_from_token(&token_value) {
            Some((profile, token_expires_at)) => ok_response(AuthMeResult {
                token_expires_at,
                profile,
            }),
            None => error_response(AuthError::SessionInvalid),
        },
        None => error_response(AuthError::SessionInvalid),
    };
    Ok(warp::reply::with_header(
        reply,
        "X-Data-Retention-Policy",
        format!(
            "{}h-grace-period",
            auth_service.inner.deletion_grace_period_hours
        ),
    )
    .into_response())
}

pub(super) async fn handle_auth_logout(
    authorization_header: Option<String>,
    cookie_header: Option<String>,
    query: TokenQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<warp::reply::Response, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        })
        .into_response());
    }
    let token = resolve_token_with_cookie(
        authorization_header.as_deref(),
        &query,
        cookie_header.as_deref(),
    );
    let revoked = token
        .as_deref()
        .map(|value| auth_service.revoke_session_token(value))
        .unwrap_or(false);
    let reply = ok_response(serde_json::json!({ "revoked": revoked }));
    if auth_service.use_auth_cookies() {
        return Ok(warp::reply::with_header(
            reply,
            "Set-Cookie",
            clear_session_cookie_header(auth_service.auth_cookie_secure()),
        )
        .into_response());
    }
    Ok(reply.into_response())
}

async fn handle_auth_leaderboard(
    authorization_header: Option<String>,
    cookie_header: Option<String>,
    query: LeaderboardQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<impl Reply, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        }));
    }
    let token_query = TokenQuery::default();
    let token = resolve_token_with_cookie(
        authorization_header.as_deref(),
        &token_query,
        cookie_header.as_deref(),
    );
    let Some(token_value) = token else {
        return Ok(error_response(AuthError::SessionInvalid));
    };
    if auth_service.profile_from_token(&token_value).is_none() {
        return Ok(error_response(AuthError::SessionInvalid));
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_LEADERBOARD_LIMIT)
        .clamp(1, MAX_LEADERBOARD_LIMIT);
    let players = auth_service.leaderboard(limit);
    Ok(ok_response(LeaderboardResult { players }))
}

async fn handle_delete_account(
    authorization_header: Option<String>,
    cookie_header: Option<String>,
    query: TokenQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<warp::reply::Response, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        })
        .into_response());
    }
    let token = resolve_token_with_cookie(
        authorization_header.as_deref(),
        &query,
        cookie_header.as_deref(),
    );
    let Some(token_value) = token else {
        return Ok(error_response(AuthError::SessionInvalid).into_response());
    };
    let Some(user_id) = auth_service.resolve_user_id_from_token(&token_value) else {
        return Ok(error_response(AuthError::SessionInvalid).into_response());
    };

    match auth_service.request_account_deletion(&user_id) {
        Ok(result) => Ok(warp::reply::with_header(
            ok_response(result),
            "X-Data-Retention-Policy",
            format!(
                "{}h-grace-period",
                auth_service.inner.deletion_grace_period_hours
            ),
        )
        .into_response()),
        Err(error) => Ok(error_response(error).into_response()),
    }
}

async fn handle_cancel_deletion(
    authorization_header: Option<String>,
    cookie_header: Option<String>,
    query: TokenQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<warp::reply::Response, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        })
        .into_response());
    }
    let token = resolve_token_with_cookie(
        authorization_header.as_deref(),
        &query,
        cookie_header.as_deref(),
    );
    let Some(token_value) = token else {
        return Ok(error_response(AuthError::SessionInvalid).into_response());
    };
    let Some(user_id) = auth_service.resolve_user_id_from_token(&token_value) else {
        return Ok(error_response(AuthError::SessionInvalid).into_response());
    };

    match auth_service.cancel_account_deletion(&user_id) {
        Ok(result) => Ok(warp::reply::with_header(
            ok_response(result),
            "X-Data-Retention-Policy",
            format!(
                "{}h-grace-period",
                auth_service.inner.deletion_grace_period_hours
            ),
        )
        .into_response()),
        Err(error) => Ok(error_response(error).into_response()),
    }
}

pub(super) fn ok_response<T: Serialize>(data: T) -> warp::reply::WithStatus<warp::reply::Json> {
    let body = ApiResponse::<T> {
        ok: true,
        data: Some(data),
        error: None,
    };
    warp::reply::with_status(warp::reply::json(&body), StatusCode::OK)
}

pub(super) fn error_response(error: AuthError) -> warp::reply::WithStatus<warp::reply::Json> {
    let (status, code, message, retry_after_seconds, remaining_attempts) = error.to_http();
    let body = ApiResponse::<serde_json::Value> {
        ok: false,
        data: None,
        error: Some(ApiErrorBody {
            code,
            message,
            retry_after_seconds,
            remaining_attempts,
        }),
    };
    warp::reply::with_status(warp::reply::json(&body), status)
}

pub(super) fn build_session_cookie_header(token: &str, max_age: u64, secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "mgs_session={}; HttpOnly{}; SameSite=Strict; Path=/; Max-Age={}",
        token, secure_attr, max_age
    )
}

pub(super) fn clear_session_cookie_header(secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "mgs_session=; HttpOnly{}; SameSite=Strict; Path=/; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        secure_attr
    )
}

pub(super) fn resolve_token_with_cookie(
    authorization_header: Option<&str>,
    _query: &TokenQuery,
    cookie_header: Option<&str>,
) -> Option<String> {
    // Priority: Authorization header > Cookie. Query parameter is strictly ignored to prevent leaking tokens in URLs.
    if let Some(token) = parse_bearer_token(authorization_header) {
        return Some(token);
    }
    if let Some(token) = parse_session_cookie(cookie_header) {
        return Some(token);
    }
    None
}

/// Extracts the `mgs_session` cookie value from a Cookie header string.
pub(super) fn parse_session_cookie(cookie_header: Option<&str>) -> Option<String> {
    let header = cookie_header?.trim();
    if header.is_empty() {
        return None;
    }
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix("mgs_session=") {
            let token = value.trim();
            if !token.is_empty() {
                return Some(token.to_owned());
            }
        }
    }
    None
}

fn parse_bearer_token(authorization_header: Option<&str>) -> Option<String> {
    let raw = authorization_header?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(stripped) = raw.strip_prefix("Bearer ") {
        let token = stripped.trim();
        if !token.is_empty() {
            return Some(token.to_owned());
        }
    }
    None
}
