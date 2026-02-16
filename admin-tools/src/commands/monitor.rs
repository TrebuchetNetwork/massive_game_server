use crate::commands::{get_json, post_json};
use anyhow::Result;
use serde_json::json;

pub async fn health_check(base_url: &str) -> Result<()> {
    let response = get_json(base_url, "/healthz", None).await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

pub async fn feature_flags(base_url: &str, admin_token: Option<&str>) -> Result<()> {
    let response = get_json(base_url, "/api/ops/feature-flags", admin_token).await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

pub async fn broadcast_message(
    base_url: &str,
    admin_token: Option<&str>,
    message: &str,
) -> Result<()> {
    let response = post_json(
        base_url,
        "/api/ops/admin/broadcast",
        admin_token,
        &json!({ "message": message }),
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}
