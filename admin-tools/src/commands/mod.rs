pub mod monitor;
pub mod performance;
pub mod players;

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;

fn build_headers(admin_token: Option<&str>) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(token) = admin_token.map(str::trim).filter(|value| !value.is_empty()) {
        let rendered = format!("Bearer {}", token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&rendered).context("invalid admin bearer token")?,
        );
    }
    Ok(headers)
}

pub async fn get_json(base_url: &str, path: &str, admin_token: Option<&str>) -> Result<Value> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let response = client
        .get(url)
        .headers(build_headers(admin_token)?)
        .send()
        .await
        .context("request failed")?
        .error_for_status()
        .context("request failed with non-success status")?;
    response
        .json::<Value>()
        .await
        .context("invalid json response")
}
