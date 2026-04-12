use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("File system error: {0}")]
    FileSystem(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Keyring error: {0}")]
    Keyring(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, AgentError>;

// Convert to String for Tauri commands
impl From<AgentError> for String {
    fn from(err: AgentError) -> String {
        err.to_string()
    }
}
