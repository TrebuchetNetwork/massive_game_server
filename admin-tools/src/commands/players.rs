use crate::commands::{get_json, post_json};
use anyhow::Result;
use serde_json::json;

pub async fn list_recent_players(
    base_url: &str,
    admin_token: Option<&str>,
    limit: usize,
) -> Result<()> {
    let path = format!("/api/ops/live-replay/recent?limit={}", limit.clamp(1, 128));
    let response = get_json(base_url, &path, admin_token).await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

pub async fn kick_player(base_url: &str, admin_token: Option<&str>, peer_id: &str) -> Result<()> {
    let response = post_json(
        base_url,
        "/api/ops/admin/kick",
        admin_token,
        &json!({ "peer_id": peer_id }),
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}
