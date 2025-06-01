// massive_game_server/server/src/core/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Join error: {0}")]
    JoinError(String), 

    #[error("Game logic error: {0}")]
    LogicError(String),

    #[error("Threading error: {0}")]
    ThreadingError(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Initialization failed: {0}")]
    InitializationFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type ServerResult<T> = Result<T, ServerError>;