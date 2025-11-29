//! Error types and handling for `TravelAI` application

use thiserror::Error;

/// Main error type for the `TravelAI` application
#[derive(Error, Debug)]
pub enum TravelAiError {
    /// Configuration-related errors
    #[error("Configuration error: {message}")]
    Config { message: String },

    /// API communication errors
    #[error("API error: {message}")]
    Api { message: String },

    /// Input validation errors
    #[error("Invalid input: {message}")]
    Validation { message: String },

    /// Cache operation errors
    #[error("Cache error: {message}")]
    Cache { message: String },

    /// I/O operation errors
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    /// General application errors
    #[error("Application error: {message}")]
    General { message: String },
}

impl TravelAiError {
    /// Create a new configuration error
    pub fn config<S: Into<String>>(message: S) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    /// Create a new API error
    pub fn api<S: Into<String>>(message: S) -> Self {
        Self::Api {
            message: message.into(),
        }
    }

    /// Create a new validation error
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }

    /// Create a new cache error
    pub fn cache<S: Into<String>>(message: S) -> Self {
        Self::Cache {
            message: message.into(),
        }
    }

    /// Create a new general error
    pub fn general<S: Into<String>>(message: S) -> Self {
        Self::General {
            message: message.into(),
        }
    }

    /// Get a user-friendly error message
    #[must_use]
    pub fn user_message(&self) -> String {
        match self {
            TravelAiError::Config { .. } => {
                "Configuration error. Please check your config file and API keys.".to_string()
            }
            TravelAiError::Api { .. } => {
                "Unable to connect to external services. Please check your internet connection."
                    .to_string()
            }
            TravelAiError::Validation { message } => {
                format!("Invalid input: {message}")
            }
            TravelAiError::Cache { .. } => {
                "Cache operation failed. You may need to clear your cache.".to_string()
            }
            TravelAiError::Io { .. } => {
                "File operation failed. Please check file permissions.".to_string()
            }
            TravelAiError::General { message } => message.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let config_err = TravelAiError::config("missing API key");
        assert!(matches!(config_err, TravelAiError::Config { .. }));

        let api_err = TravelAiError::api("connection failed");
        assert!(matches!(api_err, TravelAiError::Api { .. }));

        let validation_err = TravelAiError::validation("invalid coordinates");
        assert!(matches!(validation_err, TravelAiError::Validation { .. }));
    }

    #[test]
    fn test_user_messages() {
        let config_err = TravelAiError::config("test");
        assert!(config_err.user_message().contains("Configuration error"));

        let api_err = TravelAiError::api("test");
        assert!(api_err.user_message().contains("Unable to connect"));

        let validation_err = TravelAiError::validation("test input");
        assert!(validation_err.user_message().contains("test input"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let travel_err: TravelAiError = io_err.into();
        assert!(matches!(travel_err, TravelAiError::Io { .. }));
    }
}
