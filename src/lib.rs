//! `TravelAI` - Intelligent paragliding and outdoor adventure travel planning
//!
//! This library provides the core functionality for weather analysis,
//! paragliding site evaluation, and travel planning recommendations.

pub mod api;
pub mod cache;
pub mod config;
pub mod error;
pub mod models;
pub mod paragliding;
pub mod paragliding_forecast;
pub mod weather;
pub mod wind_analysis;

// Re-export core types for public API
pub use api::{GeocodingResult, LocationInput, LocationParser, WeatherApiClient};
pub use cache::Cache;
pub use config::TravelAiConfig;
pub use error::{ErrorCode, TravelAiError};
pub use models::{Location, WeatherData, WeatherForecast};
pub use paragliding::{DHVParser, GeographicSearch, ParaglidingEarthClient, ParaglidingSite};
pub use paragliding_forecast::{ParaglidingForecast, ParaglidingForecastService};
pub use wind_analysis::{FlyabilityAnalysis, WindDirectionAnalysis, WindSpeedAnalysis};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Core result type used throughout the library
pub type Result<T> = std::result::Result<T, TravelAiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_is_set() {
        assert!(!VERSION.is_empty());
    }
}
