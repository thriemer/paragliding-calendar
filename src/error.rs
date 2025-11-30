//! Error types and handling for `TravelAI` application

use std::collections::HashMap;
use thiserror::Error;

/// Error codes for programmatic handling
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCode {
    /// Configuration errors
    ConfigMissingApiKey,
    ConfigInvalidFormat,
    ConfigFileNotFound,
    
    /// API errors
    ApiUnauthorized,
    ApiRateLimit,
    ApiNetworkError,
    ApiInvalidResponse,
    ApiLocationNotFound,
    
    /// Validation errors
    ValidationInvalidCoordinates,
    ValidationEmptyInput,
    ValidationInvalidFormat,
    
    /// Cache errors
    CacheInitFailed,
    CacheWriteFailed,
    CacheReadFailed,
    
    /// I/O errors
    IoFileNotFound,
    IoPermissionDenied,
    IoGeneral,
    
    /// Paragliding-specific errors
    ParaglidingParseError,
    ParaglidingApiError,
    ParaglidingFileError,
    
    /// General errors
    Unknown,
}

impl ErrorCode {
    /// Get string representation of error code
    #[must_use] 
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::ConfigMissingApiKey => "CONFIG_MISSING_API_KEY",
            ErrorCode::ConfigInvalidFormat => "CONFIG_INVALID_FORMAT", 
            ErrorCode::ConfigFileNotFound => "CONFIG_FILE_NOT_FOUND",
            ErrorCode::ApiUnauthorized => "API_UNAUTHORIZED",
            ErrorCode::ApiRateLimit => "API_RATE_LIMIT",
            ErrorCode::ApiNetworkError => "API_NETWORK_ERROR",
            ErrorCode::ApiInvalidResponse => "API_INVALID_RESPONSE",
            ErrorCode::ApiLocationNotFound => "API_LOCATION_NOT_FOUND",
            ErrorCode::ValidationInvalidCoordinates => "VALIDATION_INVALID_COORDINATES",
            ErrorCode::ValidationEmptyInput => "VALIDATION_EMPTY_INPUT",
            ErrorCode::ValidationInvalidFormat => "VALIDATION_INVALID_FORMAT",
            ErrorCode::CacheInitFailed => "CACHE_INIT_FAILED",
            ErrorCode::CacheWriteFailed => "CACHE_WRITE_FAILED",
            ErrorCode::CacheReadFailed => "CACHE_READ_FAILED",
            ErrorCode::IoFileNotFound => "IO_FILE_NOT_FOUND",
            ErrorCode::IoPermissionDenied => "IO_PERMISSION_DENIED",
            ErrorCode::IoGeneral => "IO_GENERAL",
            ErrorCode::ParaglidingParseError => "PARAGLIDING_PARSE_ERROR",
            ErrorCode::ParaglidingApiError => "PARAGLIDING_API_ERROR",
            ErrorCode::ParaglidingFileError => "PARAGLIDING_FILE_ERROR",
            ErrorCode::Unknown => "UNKNOWN",
        }
    }
}

/// Main error type for the `TravelAI` application
#[derive(Error, Debug)]
pub enum TravelAiError {
    /// Configuration-related errors
    #[error("Configuration error: {message}")]
    Config { 
        message: String,
        code: ErrorCode,
        context: HashMap<String, String>,
    },

    /// API communication errors
    #[error("API error: {message}")]
    Api { 
        message: String,
        code: ErrorCode,
        context: HashMap<String, String>,
    },

    /// Input validation errors
    #[error("Invalid input: {message}")]
    Validation { 
        message: String,
        code: ErrorCode,
        context: HashMap<String, String>,
    },

    /// Cache operation errors
    #[error("Cache error: {message}")]
    Cache { 
        message: String,
        code: ErrorCode,
        context: HashMap<String, String>,
    },

    /// I/O operation errors
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    /// General application errors
    #[error("Application error: {message}")]
    General { 
        message: String,
        code: ErrorCode,
        context: HashMap<String, String>,
    },
}

impl TravelAiError {
    /// Create a new configuration error
    pub fn config<S: Into<String>>(message: S) -> Self {
        Self::Config {
            message: message.into(),
            code: ErrorCode::ConfigInvalidFormat,
            context: HashMap::new(),
        }
    }
    
    /// Create a configuration error with specific code and context
    pub fn config_with_context<S: Into<String>>(
        message: S, 
        code: ErrorCode, 
        context: HashMap<String, String>
    ) -> Self {
        Self::Config {
            message: message.into(),
            code,
            context,
        }
    }

    /// Create a new API error
    pub fn api<S: Into<String>>(message: S) -> Self {
        Self::Api {
            message: message.into(),
            code: ErrorCode::ApiNetworkError,
            context: HashMap::new(),
        }
    }
    
    /// Create an API error with specific code and context
    pub fn api_with_context<S: Into<String>>(
        message: S, 
        code: ErrorCode, 
        context: HashMap<String, String>
    ) -> Self {
        Self::Api {
            message: message.into(),
            code,
            context,
        }
    }

    /// Create a new validation error
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::Validation {
            message: message.into(),
            code: ErrorCode::ValidationInvalidFormat,
            context: HashMap::new(),
        }
    }
    
    /// Create a validation error with specific code and context
    pub fn validation_with_context<S: Into<String>>(
        message: S, 
        code: ErrorCode, 
        context: HashMap<String, String>
    ) -> Self {
        Self::Validation {
            message: message.into(),
            code,
            context,
        }
    }

    /// Create a new cache error
    pub fn cache<S: Into<String>>(message: S) -> Self {
        Self::Cache {
            message: message.into(),
            code: ErrorCode::CacheInitFailed,
            context: HashMap::new(),
        }
    }
    
    /// Create a cache error with specific code and context
    pub fn cache_with_context<S: Into<String>>(
        message: S, 
        code: ErrorCode, 
        context: HashMap<String, String>
    ) -> Self {
        Self::Cache {
            message: message.into(),
            code,
            context,
        }
    }

    /// Create a new general error
    pub fn general<S: Into<String>>(message: S) -> Self {
        Self::General {
            message: message.into(),
            code: ErrorCode::Unknown,
            context: HashMap::new(),
        }
    }
    
    /// Create a general error with specific code and context
    pub fn general_with_context<S: Into<String>>(
        message: S, 
        code: ErrorCode, 
        context: HashMap<String, String>
    ) -> Self {
        Self::General {
            message: message.into(),
            code,
            context,
        }
    }

    /// Get the error code
    #[must_use] 
    pub fn code(&self) -> &ErrorCode {
        match self {
            TravelAiError::Config { code, .. } | TravelAiError::Api { code, .. } | TravelAiError::Validation { code, .. } | TravelAiError::Cache { code, .. } | TravelAiError::General { code, .. } => code,
            TravelAiError::Io { source } => {
                match source.kind() {
                    std::io::ErrorKind::NotFound => &ErrorCode::IoFileNotFound,
                    std::io::ErrorKind::PermissionDenied => &ErrorCode::IoPermissionDenied,
                    _ => &ErrorCode::IoGeneral,
                }
            }
        }
    }

    /// Get the error context
    #[must_use] 
    pub fn context(&self) -> HashMap<String, String> {
        match self {
            TravelAiError::Config { context, .. } | TravelAiError::Api { context, .. } | TravelAiError::Validation { context, .. } | TravelAiError::Cache { context, .. } | TravelAiError::General { context, .. } => context.clone(),
            TravelAiError::Io { source } => {
                let mut ctx = HashMap::new();
                ctx.insert("kind".to_string(), format!("{:?}", source.kind()));
                ctx
            }
        }
    }

    /// Add context to the error
    #[must_use]
    pub fn with_context<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        match &mut self {
            TravelAiError::Config { context, .. } | TravelAiError::Api { context, .. } | TravelAiError::Validation { context, .. } | TravelAiError::Cache { context, .. } | TravelAiError::General { context, .. } => {
                context.insert(key.into(), value.into());
            }
            TravelAiError::Io { .. } => {
                // Cannot add context to I/O errors as they are not mutable
            }
        }
        self
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
            TravelAiError::Validation { message, .. } => {
                format!("Invalid input: {message}")
            }
            TravelAiError::Cache { .. } => {
                "Cache operation failed. You may need to clear your cache.".to_string()
            }
            TravelAiError::Io { .. } => {
                "File operation failed. Please check file permissions.".to_string()
            }
            TravelAiError::General { message, .. } => message.clone(),
        }
    }
    
    /// Get detailed error message with context (for debug/verbose mode)
    #[must_use] 
    pub fn detailed_message(&self) -> String {
        let base_message = self.to_string();
        let code = self.code().as_str();
        let context = self.context();
        
        let mut detailed = format!("{base_message} [{code}]");
        
        if !context.is_empty() {
            detailed.push_str("\nContext:");
            for (key, value) in context {
                use std::fmt::Write;
                let _ = write!(detailed, "\n  {key}: {value}");
            }
        }
        
        detailed
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
    fn test_error_codes() {
        let config_err = TravelAiError::config("test");
        assert_eq!(config_err.code(), &ErrorCode::ConfigInvalidFormat);

        let api_err = TravelAiError::api("test");
        assert_eq!(api_err.code(), &ErrorCode::ApiNetworkError);

        let validation_err = TravelAiError::validation("test");
        assert_eq!(validation_err.code(), &ErrorCode::ValidationInvalidFormat);
    }

    #[test]
    fn test_error_with_context() {
        let mut ctx = HashMap::new();
        ctx.insert("location".to_string(), "Chamonix".to_string());
        
        let api_err = TravelAiError::api_with_context(
            "Location not found",
            ErrorCode::ApiLocationNotFound,
            ctx
        );
        
        assert_eq!(api_err.code(), &ErrorCode::ApiLocationNotFound);
        let context = api_err.context();
        assert_eq!(context.get("location"), Some(&"Chamonix".to_string()));
    }

    #[test]
    fn test_error_context_chaining() {
        let err = TravelAiError::api("test")
            .with_context("operation", "geocoding")
            .with_context("location", "test location");
            
        let context = err.context();
        assert_eq!(context.get("operation"), Some(&"geocoding".to_string()));
        assert_eq!(context.get("location"), Some(&"test location".to_string()));
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
    fn test_detailed_message() {
        let err = TravelAiError::api("connection timeout")
            .with_context("url", "api.example.com")
            .with_context("timeout", "30s");
            
        let detailed = err.detailed_message();
        assert!(detailed.contains("API_NETWORK_ERROR"));
        assert!(detailed.contains("url: api.example.com"));
        assert!(detailed.contains("timeout: 30s"));
    }

    #[test]
    fn test_error_code_string_representation() {
        assert_eq!(ErrorCode::ConfigMissingApiKey.as_str(), "CONFIG_MISSING_API_KEY");
        assert_eq!(ErrorCode::ApiUnauthorized.as_str(), "API_UNAUTHORIZED");
        assert_eq!(ErrorCode::ValidationInvalidCoordinates.as_str(), "VALIDATION_INVALID_COORDINATES");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let travel_err: TravelAiError = io_err.into();
        assert!(matches!(travel_err, TravelAiError::Io { .. }));
        assert_eq!(travel_err.code(), &ErrorCode::IoFileNotFound);
    }
}
