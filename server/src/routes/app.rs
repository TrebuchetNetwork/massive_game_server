use crate::routes::rejections::handle_route_rejection;

use tracing::info;
use warp::http::{HeaderName, HeaderValue};
use warp::Filter;

pub fn compose_http_routes(
    protected_routes: warp::filters::BoxedFilter<(warp::reply::Response,)>,
    public_routes: warp::filters::BoxedFilter<(warp::reply::Response,)>,
    allowed_cors_origins: Vec<String>,
    behind_tls_proxy: bool,
) -> warp::filters::BoxedFilter<(warp::reply::Response,)> {
    let base_routes = protected_routes.or(public_routes).boxed();
    let recovered_routes = base_routes.recover(handle_route_rejection);

    let routes = if allowed_cors_origins.is_empty() {
        info!(
            "No cross-origin API origins configured (set MGS_ALLOWED_ORIGINS for explicit allowlist)."
        );
        recovered_routes
            .map(warp::reply::Reply::into_response)
            .boxed()
    } else {
        for origin in &allowed_cors_origins {
            info!("Allowing API CORS origin: {}", origin);
        }
        recovered_routes
            .with(
                warp::cors()
                    .allow_origins(allowed_cors_origins.iter().map(String::as_str))
                    .allow_methods(vec!["GET", "POST", "OPTIONS"])
                    .allow_headers(vec![
                        "Content-Type",
                        "Authorization",
                        "User-Agent",
                        "Sec-WebSocket-Key",
                        "Sec-WebSocket-Version",
                        "Sec-WebSocket-Extensions",
                        "Upgrade",
                        "Connection",
                    ]),
            )
            .map(warp::reply::Reply::into_response)
            .boxed()
    };

    if behind_tls_proxy {
        routes
            .map(|mut response: warp::http::Response<warp::hyper::Body>| {
                let headers = response.headers_mut();
                headers.insert(
                    HeaderName::from_static("strict-transport-security"),
                    HeaderValue::from_static("max-age=63072000; includeSubDomains; preload"),
                );
                headers.insert(
                    HeaderName::from_static("x-content-type-options"),
                    HeaderValue::from_static("nosniff"),
                );
                headers.insert(
                    HeaderName::from_static("x-frame-options"),
                    HeaderValue::from_static("DENY"),
                );
                headers.insert(
                    HeaderName::from_static("referrer-policy"),
                    HeaderValue::from_static("strict-origin-when-cross-origin"),
                );
                response
            })
            .boxed()
    } else {
        routes
    }
}
