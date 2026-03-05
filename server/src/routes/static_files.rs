use std::path::Path;
use warp::http::{header, HeaderName, HeaderValue, Uri};
use warp::{Filter, Reply};

pub fn build_root_route(
) -> impl Filter<Extract = (warp::reply::Response,), Error = warp::Rejection> + Clone {
    warp::path::end()
        .and(warp::get())
        .map(|| warp::redirect::temporary(Uri::from_static("/index.html")))
        .map(warp::reply::Reply::into_response)
}

pub fn build_static_files_route(
    static_asset_allow_origin: Option<String>,
) -> impl Filter<Extract = (warp::reply::Response,), Error = warp::Rejection> + Clone {
    warp::fs::dir("static_client").map(move |reply: warp::filters::fs::File| {
        let requested_path = reply.path().to_path_buf();
        let cache_control = static_cache_control_for_path(&requested_path);
        let mut response = reply.into_response();
        let headers = response.headers_mut();

        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control),
        );
        headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
        headers.insert(
            HeaderName::from_static("timing-allow-origin"),
            HeaderValue::from_static("*"),
        );
        headers.insert(
            HeaderName::from_static("cross-origin-resource-policy"),
            HeaderValue::from_static("cross-origin"),
        );

        if let Some(origin) = static_asset_allow_origin.as_deref() {
            if let Ok(header_value) = HeaderValue::from_str(origin) {
                headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, header_value);
            }
        }

        response
    })
}

pub fn static_cache_control_for_path(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match extension.as_deref() {
        Some("html") => "no-cache, no-store, must-revalidate",
        Some("js") | Some("mjs") | Some("css") | Some("wasm") | Some("png") | Some("jpg")
        | Some("jpeg") | Some("webp") | Some("gif") | Some("svg") | Some("ico") | Some("woff")
        | Some("woff2") | Some("ttf") | Some("otf") | Some("mp3") | Some("ogg") | Some("wav") => {
            "public, max-age=31536000, immutable"
        }
        Some("json") | Some("map") => "public, max-age=300",
        _ => "public, max-age=3600",
    }
}
