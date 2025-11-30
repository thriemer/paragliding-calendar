use thiserror::Error;

/// Simplified error type for paragliding module
#[derive(Error, Debug)]
pub enum TravelAIError {
    #[error("Parse error: {0}")]
    ParseError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("API error: {0}")]
    ApiError(String),
    
    #[error("Authentication error: {0}")]
    AuthenticationError(String),
    
    #[error("Rate limit error: {0}")]
    RateLimitError(String),
    
    #[error("File not found: {0}")]
    FileNotFound(String),
    
    #[error("IO error: {0}")]
    IoError(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
}

impl From<anyhow::Error> for TravelAIError {
    fn from(err: anyhow::Error) -> Self {
        TravelAIError::CacheError(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, TravelAIError>;