use crate::operational::admin_auth::AdminAuthRejection;
use crate::routes::ws_signaling::{
    ConnectionLimitRejection, OriginRejection, TransportSecurityRejection,
};

use std::convert::Infallible;
use tracing::error;
use warp::http::StatusCode;
use warp::{Rejection, Reply};

pub async fn handle_route_rejection(rejection: Rejection) -> Result<impl Reply, Infallible> {
    if rejection.find::<OriginRejection>().is_some() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "origin_not_allowed",
                "message": "WebSocket upgrade rejected: Origin not in allowlist."
            }
        }));
        return Ok(warp::reply::with_status(body, StatusCode::FORBIDDEN));
    }

    if rejection.find::<TransportSecurityRejection>().is_some() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "insecure_transport",
                "message": "WebSocket upgrade rejected: HTTPS/WSS transport required."
            }
        }));
        return Ok(warp::reply::with_status(body, StatusCode::FORBIDDEN));
    }

    if rejection.find::<ConnectionLimitRejection>().is_some() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "connection_limit_reached",
                "message": "Server connection limit reached. Please retry shortly."
            }
        }));
        return Ok(warp::reply::with_status(
            body,
            StatusCode::SERVICE_UNAVAILABLE,
        ));
    }

    if let Some(admin_rejection) = rejection.find::<AdminAuthRejection>() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": admin_rejection.code,
                "message": admin_rejection.message
            }
        }));
        return Ok(warp::reply::with_status(body, admin_rejection.status));
    }

    if rejection.is_not_found() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "not_found",
                "message": "Route not found."
            }
        }));
        return Ok(warp::reply::with_status(body, StatusCode::NOT_FOUND));
    }

    if rejection.find::<warp::reject::MethodNotAllowed>().is_some() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "method_not_allowed",
                "message": "Method not allowed."
            }
        }));
        return Ok(warp::reply::with_status(
            body,
            StatusCode::METHOD_NOT_ALLOWED,
        ));
    }

    if let Some(err) = rejection.find::<warp::filters::body::BodyDeserializeError>() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "invalid_json",
                "message": err.to_string()
            }
        }));
        return Ok(warp::reply::with_status(body, StatusCode::BAD_REQUEST));
    }

    if let Some(err) = rejection.find::<warp::reject::InvalidQuery>() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "invalid_query",
                "message": err.to_string()
            }
        }));
        return Ok(warp::reply::with_status(body, StatusCode::BAD_REQUEST));
    }

    error!("Unhandled route rejection: {:?}", rejection);
    let body = warp::reply::json(&serde_json::json!({
        "ok": false,
        "error": {
            "code": "internal_error",
            "message": "Unhandled server rejection."
        }
    }));
    Ok(warp::reply::with_status(
        body,
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}
