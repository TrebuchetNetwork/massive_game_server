use crate::commands::get_json;
use anyhow::Result;

pub async fn worker_stats(base_url: &str, admin_token: Option<&str>) -> Result<()> {
    let response = get_json(base_url, "/api/arena/worker/stats", admin_token).await?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}
