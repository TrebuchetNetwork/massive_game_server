// massive_game_server/server/src/operational/config/mod.rs

pub mod loader;
pub mod validation;

use crate::core::config::ServerConfig;
use anyhow::Result;

pub fn load_validated_server_config() -> Result<ServerConfig> {
    let config = loader::load_server_config_from_env_and_file()?;
    validation::validate_server_config(&config)?;
    Ok(config)
}
