use crate::commands::get_json;
use anyhow::Result;

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
