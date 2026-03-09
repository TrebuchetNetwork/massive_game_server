use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use warp::http::{header, HeaderName, HeaderValue, Uri};
use warp::{Filter, Reply};

static STATIC_HTML_CSP_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

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
        if let Some(csp) = static_content_security_policy_for_path(&requested_path) {
            if let Ok(csp_header) = HeaderValue::from_str(&csp) {
                headers.insert(
                    HeaderName::from_static("content-security-policy"),
                    csp_header,
                );
            }
        }

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
        Some("js") | Some("mjs") | Some("css") | Some("wasm") => {
            "public, max-age=300, must-revalidate"
        }
        Some("png") | Some("jpg") | Some("jpeg") | Some("webp") | Some("gif") | Some("svg")
        | Some("ico") | Some("woff") | Some("woff2") | Some("ttf") | Some("otf") | Some("mp3")
        | Some("ogg") | Some("wav") => "public, max-age=31536000, immutable",
        Some("json") | Some("map") => "public, max-age=300",
        _ => "public, max-age=3600",
    }
}

pub fn static_content_security_policy_for_path(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if file_name == "index.html" {
        return Some(
            "default-src 'self'; script-src 'self'; worker-src 'self'; connect-src 'self' ws: wss:; img-src 'self' data: blob:; style-src 'self'; font-src 'self' data:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
                .to_owned(),
        );
    }
    if file_name != "client.html" {
        return None;
    }

    let cache = STATIC_HTML_CSP_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache_key = path.to_string_lossy().into_owned();
    if let Some(existing) = cache.lock().ok()?.get(&cache_key).cloned() {
        return Some(existing);
    }

    let html = fs::read_to_string(path).ok()?;
    let script_hashes = inline_script_hashes(&html);
    if script_hashes.is_empty() {
        return None;
    }

    let hash_list = script_hashes
        .iter()
        .map(|hash| format!(" 'sha256-{}'", hash))
        .collect::<String>();
    let csp = format!(
        "default-src 'self'; script-src 'self' 'unsafe-eval' blob:{}; worker-src 'self' blob:; connect-src 'self' ws: wss:; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'",
        hash_list
    );
    if let Ok(mut guard) = cache.lock() {
        guard.insert(cache_key, csp.clone());
    }
    Some(csp)
}

fn inline_script_hashes(html: &str) -> Vec<String> {
    let mut hashes = Vec::new();
    let mut remaining = html;

    while let Some(start_idx) = remaining.find("<script") {
        let after_start = &remaining[start_idx..];
        let Some(tag_end_idx) = after_start.find('>') else {
            break;
        };
        let tag = &after_start[..=tag_end_idx];
        let tag_lower = tag.to_ascii_lowercase();
        let script_body_start = start_idx + tag_end_idx + 1;

        let Some(script_close_rel_idx) = remaining[script_body_start..].find("</script>") else {
            break;
        };
        let script_body_end = script_body_start + script_close_rel_idx;
        if !tag_lower.contains("src=") {
            let body = &remaining[script_body_start..script_body_end];
            if !body.trim().is_empty() {
                let digest = Sha256::digest(body.as_bytes());
                hashes.push(base64::engine::general_purpose::STANDARD.encode(digest));
            }
        }
        let next_start = script_body_end + "</script>".len();
        remaining = &remaining[next_start..];
    }

    hashes
}
