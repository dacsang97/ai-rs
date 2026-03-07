use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("Tool error ({tool}): {message}")]
    Tool { tool: String, message: String },

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("Request timeout after {0}s")]
    Timeout(u64),

    #[error("Request cancelled")]
    Cancelled,

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Invalid configuration: {0}")]
    Config(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AiError>;
