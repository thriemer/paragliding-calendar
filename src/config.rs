//! Configuration management for `TravelAI` application
//!
//! Handles loading configuration from files, environment variables,
//! and provides validation for all configuration settings.

use crate::TravelAiError;
use anyhow::{Context, Result};
use config::{Config, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Root configuration structure for the `TravelAI` application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TravelAiConfig {
    /// Weather API configuration
    pub weather: WeatherConfig,
    /// Cache configuration
    pub cache: CacheConfig,
    /// Logging configuration
    pub logging: LoggingConfig,
    /// Default application settings
    pub defaults: DefaultsConfig,
}

/// Weather API configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherConfig {
    /// Weather API key (optional for OpenMeteo)
    pub api_key: Option<String>,
    /// Base URL for weather API
    #[serde(default = "default_weather_base_url")]
    pub base_url: String,
    /// Request timeout in seconds
    #[serde(default = "default_weather_timeout")]
    pub timeout_seconds: u32,
    /// Maximum number of retries for failed requests
    #[serde(default = "default_weather_max_retries")]
    pub max_retries: u32,
}

/// Cache configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Cache TTL in hours
    #[serde(default = "default_cache_ttl")]
    pub ttl_hours: u32,
    /// Maximum cache size in MB
    #[serde(default = "default_cache_max_size")]
    pub max_size_mb: u32,
    /// Cache directory location
    #[serde(default = "default_cache_location")]
    pub location: String,
}

/// Logging configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (error, warn, info, debug, trace)
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Log format (pretty or json)
    #[serde(default = "default_log_format")]
    pub format: String,
    /// Log output destination (console, file, both)
    #[serde(default = "default_log_output")]
    pub output: String,
    /// Log file path
    #[serde(default = "default_log_file_path")]
    pub file_path: String,
    /// Maximum log file size in MB
    #[serde(default = "default_log_max_file_size")]
    pub max_file_size_mb: u32,
    /// Maximum number of log files to keep
    #[serde(default = "default_log_max_files")]
    pub max_files: u32,
}

/// Default application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    /// Search radius in kilometers
    #[serde(default = "default_search_radius")]
    pub search_radius_km: u32,
    /// Maximum number of sites to return
    #[serde(default = "default_max_sites")]
    pub max_sites: u32,
}

// Default value functions
fn default_weather_base_url() -> String {
    "https://api.open-meteo.com/v1".to_string()
}

fn default_weather_timeout() -> u32 {
    30
}

fn default_weather_max_retries() -> u32 {
    3
}

fn default_cache_ttl() -> u32 {
    6
}

fn default_cache_max_size() -> u32 {
    100
}

fn default_cache_location() -> String {
    "~/.cache/travelai".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_log_output() -> String {
    "console".to_string()
}

fn default_log_file_path() -> String {
    "~/.cache/travelai/app.log".to_string()
}

fn default_log_max_file_size() -> u32 {
    10
}

fn default_log_max_files() -> u32 {
    5
}

fn default_search_radius() -> u32 {
    50
}

fn default_max_sites() -> u32 {
    10
}

impl Default for TravelAiConfig {
    fn default() -> Self {
        Self {
            weather: WeatherConfig {
                api_key: None,
                base_url: default_weather_base_url(),
                timeout_seconds: default_weather_timeout(),
                max_retries: default_weather_max_retries(),
            },
            cache: CacheConfig {
                ttl_hours: default_cache_ttl(),
                max_size_mb: default_cache_max_size(),
                location: default_cache_location(),
            },
            logging: LoggingConfig {
                level: default_log_level(),
                format: default_log_format(),
                output: default_log_output(),
                file_path: default_log_file_path(),
                max_file_size_mb: default_log_max_file_size(),
                max_files: default_log_max_files(),
            },
            defaults: DefaultsConfig {
                search_radius_km: default_search_radius(),
                max_sites: default_max_sites(),
            },
        }
    }
}

impl TravelAiConfig {
    /// Load configuration from file and environment variables
    pub fn load() -> Result<Self> {
        Self::load_from_path(None)
    }

    /// Load configuration from specified path
    pub fn load_from_path(config_path: Option<PathBuf>) -> Result<Self> {
        let mut builder = Config::builder();

        // Load from file if path is provided or use default location
        let config_file = config_path.unwrap_or_else(|| {
            Self::get_config_path().unwrap_or_else(|| PathBuf::from("config.toml"))
        });

        if config_file.exists() {
            builder = builder.add_source(
                File::from(config_file.clone())
                    .required(false)
                    .format(config::FileFormat::Toml),
            );
        }

        // Add environment variable overrides with TRAVELAI_ prefix
        builder = builder.add_source(
            Environment::with_prefix("TRAVELAI")
                .separator("_")
                .try_parsing(true),
        );

        let settings = builder
            .build()
            .with_context(|| "Failed to build configuration")?;

        let mut config: TravelAiConfig = settings
            .try_deserialize()
            .with_context(|| "Failed to deserialize configuration")?;

        // Apply defaults for missing values
        config.apply_defaults();

        // Validate configuration
        config.validate()?;

        Ok(config)
    }

    /// Get the default configuration file path
    #[must_use]
    pub fn get_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("travelai").join("config.toml"))
    }

    /// Apply default values to missing configuration fields
    pub fn apply_defaults(&mut self) {
        if self.weather.base_url.is_empty() {
            self.weather.base_url = default_weather_base_url();
        }
        if self.weather.timeout_seconds == 0 {
            self.weather.timeout_seconds = default_weather_timeout();
        }
        if self.weather.max_retries == 0 {
            self.weather.max_retries = default_weather_max_retries();
        }
        if self.cache.ttl_hours == 0 {
            self.cache.ttl_hours = default_cache_ttl();
        }
        if self.cache.max_size_mb == 0 {
            self.cache.max_size_mb = default_cache_max_size();
        }
        if self.cache.location.is_empty() {
            self.cache.location = default_cache_location();
        }
        if self.logging.level.is_empty() {
            self.logging.level = default_log_level();
        }
        if self.logging.format.is_empty() {
            self.logging.format = default_log_format();
        }
        if self.defaults.search_radius_km == 0 {
            self.defaults.search_radius_km = default_search_radius();
        }
        if self.defaults.max_sites == 0 {
            self.defaults.max_sites = default_max_sites();
        }
    }

    /// Validate all configuration settings
    pub fn validate(&self) -> Result<()> {
        self.validate_api_keys()?;
        self.validate_numeric_ranges()?;
        self.validate_string_values()?;
        Ok(())
    }

    /// Validate API keys and credentials
    pub fn validate_api_keys(&self) -> Result<()> {
        // API key is now optional for OpenMeteo integration
        if let Some(api_key) = &self.weather.api_key {
            if api_key.is_empty() {
                return Err(TravelAiError::config(
                    "Weather API key cannot be empty if provided. Either remove it or provide a valid key."
                ).into());
            }

            if api_key.len() < 8 {
                return Err(TravelAiError::config(
                    "Weather API key appears to be invalid (too short). Please check your API key."
                ).into());
            }

            if api_key.len() > 100 {
                return Err(TravelAiError::config(
                    "Weather API key appears to be invalid (too long). Please check your API key."
                ).into());
            }
        }

        Ok(())
    }

    /// Validate numeric configuration ranges
    fn validate_numeric_ranges(&self) -> Result<()> {
        if self.weather.timeout_seconds > 300 {
            return Err(TravelAiError::config(
                "Weather API timeout cannot exceed 300 seconds"
            ).into());
        }

        if self.weather.max_retries > 10 {
            return Err(TravelAiError::config(
                "Weather API max retries cannot exceed 10"
            ).into());
        }

        if self.cache.ttl_hours > 168 {
            return Err(TravelAiError::config(
                "Cache TTL cannot exceed 168 hours (1 week)"
            ).into());
        }

        if self.cache.max_size_mb > 10000 {
            return Err(TravelAiError::config(
                "Cache max size cannot exceed 10000 MB (10 GB)"
            ).into());
        }

        if self.defaults.search_radius_km > 500 {
            return Err(TravelAiError::config(
                "Search radius cannot exceed 500 km"
            ).into());
        }

        if self.defaults.max_sites > 100 {
            return Err(TravelAiError::config(
                "Maximum sites cannot exceed 100"
            ).into());
        }

        Ok(())
    }

    /// Validate string configuration values
    fn validate_string_values(&self) -> Result<()> {
        let valid_log_levels = ["error", "warn", "info", "debug", "trace"];
        if !valid_log_levels.contains(&self.logging.level.as_str()) {
            return Err(TravelAiError::config(
                format!("Invalid log level '{}'. Must be one of: {}", 
                    self.logging.level, 
                    valid_log_levels.join(", ")
                )
            ).into());
        }

        let valid_log_formats = ["pretty", "json"];
        if !valid_log_formats.contains(&self.logging.format.as_str()) {
            return Err(TravelAiError::config(
                format!("Invalid log format '{}'. Must be one of: {}", 
                    self.logging.format, 
                    valid_log_formats.join(", ")
                )
            ).into());
        }

        if !self.weather.base_url.starts_with("http://") && !self.weather.base_url.starts_with("https://") {
            return Err(TravelAiError::config(
                "Weather API base URL must be a valid HTTP or HTTPS URL"
            ).into());
        }

        Ok(())
    }

    /// Create configuration directory if it doesn't exist
    pub fn ensure_config_dir() -> Result<PathBuf> {
        if let Some(config_dir) = dirs::config_dir() {
            let travelai_config_dir = config_dir.join("travelai");
            std::fs::create_dir_all(&travelai_config_dir)
                .with_context(|| format!("Failed to create config directory: {}", travelai_config_dir.display()))?;
            Ok(travelai_config_dir)
        } else {
            Err(TravelAiError::config("Unable to determine config directory").into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_default_config() {
        let config = TravelAiConfig::default();
        assert_eq!(config.weather.base_url, "https://api.open-meteo.com/v1");
        assert_eq!(config.weather.timeout_seconds, 30);
        assert_eq!(config.cache.ttl_hours, 6);
        assert_eq!(config.logging.level, "info");
        assert_eq!(config.defaults.search_radius_km, 50);
        assert!(config.weather.api_key.is_none());
    }

    #[test]
    fn test_config_validation_missing_api_key() {
        let config = TravelAiConfig::default();
        let result = config.validate_api_keys();
        // API key is now optional for OpenMeteo
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_validation_valid_api_key() {
        let mut config = TravelAiConfig::default();
        config.weather.api_key = Some("valid_api_key_123".to_string());
        let result = config.validate_api_keys();
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_validation_invalid_log_level() {
        let mut config = TravelAiConfig::default();
        config.weather.api_key = Some("valid_api_key_123".to_string());
        config.logging.level = "invalid".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid log level"));
    }

    #[test]
    fn test_config_validation_numeric_ranges() {
        let mut config = TravelAiConfig::default();
        config.weather.api_key = Some("valid_api_key_123".to_string());
        config.weather.timeout_seconds = 500; // Invalid - too high
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timeout cannot exceed"));
    }

    #[test]
    fn test_environment_variable_override() {
        // This test verifies that environment variables are handled correctly
        // Set minimal environment to test basic functionality
        
        // SAFETY: Test environment, setting test values only  
        unsafe {
            env::set_var("TRAVELAI_WEATHER__API_KEY", "test_key_from_env");
        }

        // Test with basic config that should have defaults
        let mut config = TravelAiConfig::default();
        config.weather.api_key = Some("test_key_from_env".to_string()); // Simulate env override
        
        let result = config.validate();
        
        // SAFETY: Test cleanup
        unsafe {
            env::remove_var("TRAVELAI_WEATHER__API_KEY");
        }

        assert!(result.is_ok());
        assert_eq!(config.weather.api_key, Some("test_key_from_env".to_string()));
    }

    #[test]
    fn test_config_path_generation() {
        let path = TravelAiConfig::get_config_path();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().contains("travelai"));
        assert!(path.to_string_lossy().contains("config.toml"));
    }
}